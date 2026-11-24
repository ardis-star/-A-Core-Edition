//! Assembly code generation: the -A Core Edition backend surface.
//!
//! Emits x86-64 Intel-syntax assembly (described in `docs/BACKEND.md`) from a
//! checked [`CheckedProgram`]. Everything below this module is target-specific;
//! the front end knows nothing about registers or calling conventions.
//!
//! Value representation:
//!
//! | type            | representation                                  |
//! |-----------------|-------------------------------------------------|
//! | `int`, `bool`   | `rax` (raw i64)                                 |
//! | `double`        | `xmm0` (f64)                                    |
//! | `string`        | `rax` = `char*`                                 |
//! | `[T]`           | `rax` = block: `[len][cap][data]`               |
//! | struct          | `rax` = block: `[count][pad][tag,value]*`       |
//! | enum            | `rax` = static object: `[value][name]`          |
//! | `ptr<T>`        | `rax` = raw address                             |
//! | `result<T>`     | `rax` = block: `[ok][tag][value][error]`        |
//!
//! Everything the program can allocate goes through the runtime allocator, so
//! automatic release, manual `@free` and the exit sweep all agree on what
//! exists.

use crate::ast::*;
use crate::semantic::{CheckedProgram, ReuseInstance, ScheduledBlock};
use crate::types::Type;
use std::collections::HashMap;

const TAG_INT: i64 = 0;
const TAG_DOUBLE: i64 = 1;
const TAG_STRING: i64 = 2;
const TAG_BOOL: i64 = 3;
const TAG_ARRAY: i64 = 4;
const TAG_STRUCT: i64 = 5;
const TAG_ENUM: i64 = 6;
const TAG_PTR: i64 = 7;
const TAG_RESULT: i64 = 8;

fn tag_of(ty: &Type) -> i64 {
    match ty {
        Type::Int => TAG_INT,
        Type::Double => TAG_DOUBLE,
        Type::String => TAG_STRING,
        Type::Bool => TAG_BOOL,
        Type::Array(_) => TAG_ARRAY,
        Type::Struct { .. } => TAG_STRUCT,
        Type::Enum(_) => TAG_ENUM,
        Type::Ptr(_) => TAG_PTR,
        Type::Result(_) => TAG_RESULT,
        Type::Mixed | Type::Unknown | Type::Param(_) => TAG_INT,
        Type::Void => TAG_INT,
    }
}

fn escape_asm_string(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\{:03o}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn format_double(v: f64) -> String {
    if v.is_nan() {
        "0".to_string()
    } else if v.is_infinite() {
        if v > 0.0 {
            "1e308".to_string()
        } else {
            "-1e308".to_string()
        }
    } else {
        format!("{:?}", v)
    }
}

fn type_symbol(ty: &Type) -> String {
    let mut out = String::new();
    for ch in ty.name().chars() {
        match ch {
            '.' | '<' | '>' | '[' | ']' | ' ' | ',' => out.push('_'),
            c => out.push(c),
        }
    }
    out
}

#[derive(Debug, Clone)]
struct Slot {
    offset: usize,
    ty: Type,
}

struct Frame {
    slots: HashMap<String, Slot>,
    /// Ownership: local names that must be released on exit.
    owned: Vec<(String, Type)>,
    /// Highest byte offset used in this frame.
    max_offset: usize,
    return_label: String,
    /// Temp slot holding the preserved return value.
    ret_slot: usize,
    ret_slot_hi: usize,
}

impl Frame {
    fn new(return_label: String) -> Frame {
        Frame {
            slots: HashMap::new(),
            owned: Vec::new(),
            max_offset: 16,
            return_label,
            ret_slot: 8,
            ret_slot_hi: 16,
        }
    }

    fn alloc(&mut self, name: &str, ty: Type) -> usize {
        let offset = self.max_offset + 16;
        self.max_offset = offset;
        self.slots.insert(name.to_string(), Slot { offset, ty });
        offset
    }

    fn offset_of(&self, name: &str) -> Option<usize> {
        self.slots.get(name).map(|s| s.offset)
    }

    fn ty_of(&self, name: &str) -> Option<Type> {
        self.slots.get(name).map(|s| s.ty.clone())
    }
}

pub struct Gen<'a> {
    out: String,
    body: Option<String>,
    strings: Vec<(String, String)>,
    doubles: Vec<(String, f64)>,
    label_counter: usize,
    checked: &'a CheckedProgram,
    /// Struct instances that need a print helper emitted.
    struct_prints: Vec<Type>,
    /// Module path prefix of the function currently being generated.
    module: String,
    /// Stack of fallibility flags, innermost function last.
    fallible_stack: Vec<bool>,
}

impl<'a> Gen<'a> {
    fn emit(&mut self, line: &str) {
        match &mut self.body {
            Some(body) => {
                body.push_str(line);
                body.push('\n');
            }
            None => {
                self.out.push_str(line);
                self.out.push('\n');
            }
        }
    }

    fn emitf(&mut self, args: std::fmt::Arguments<'_>) {
        let line = args.to_string();
        self.emit(&line);
    }

    fn begin_body(&mut self) {
        self.body = Some(String::new());
    }

    fn take_body(&mut self) -> String {
        self.body.take().unwrap_or_default()
    }

    fn fresh_label(&mut self, prefix: &str) -> String {
        self.label_counter += 1;
        format!(".L{}_{}", prefix, self.label_counter)
    }

    fn string_label(&mut self, value: &str) -> String {
        if let Some((label, _)) = self.strings.iter().find(|(_, v)| v == value) {
            return label.clone();
        }
        self.label_counter += 1;
        let label = format!(".Lstr{}", self.label_counter);
        self.strings.push((label.clone(), value.to_string()));
        label
    }

    fn double_label(&mut self, value: f64) -> String {
        self.label_counter += 1;
        let label = format!(".Ld{}", self.label_counter);
        self.doubles.push((label.clone(), value));
        label
    }

    fn global_label(name: &str) -> String {
        let mut out = String::from("__a_g_");
        for ch in name.chars() {
            match ch {
                '.' => out.push('_'),
                c => out.push(c),
            }
        }
        out
    }

    fn is_global(&self, name: &str) -> bool {
        self.checked.globals.contains_key(name)
    }

    // -----------------------------------------------------------------
    // Value movement
    // -----------------------------------------------------------------

    fn push_rax(&mut self) {
        self.emit("    sub rsp, 16");
        self.emit("    mov QWORD PTR [rsp], rax");
    }

    fn pop_to(&mut self, reg: &str) {
        self.emitf(format_args!("    mov {}, QWORD PTR [rsp]", reg));
        self.emit("    add rsp, 16");
    }

    fn push_xmm0(&mut self) {
        self.emit("    sub rsp, 16");
        self.emit("    movsd QWORD PTR [rsp], xmm0");
    }

    fn pop_xmm(&mut self, reg: &str) {
        self.emitf(format_args!("    movsd {}, QWORD PTR [rsp]", reg));
        self.emit("    add rsp, 16");
    }

    fn load_var(&mut self, frame: &Frame, name: &str, ty: &Type) {
        if let Some(offset) = frame.offset_of(name) {
            if matches!(ty, Type::Double) {
                self.emitf(format_args!("    movsd xmm0, QWORD PTR [rbp - {}]", offset));
            } else {
                self.emitf(format_args!("    mov rax, QWORD PTR [rbp - {}]", offset));
            }
            return;
        }
        let label = Self::global_label(name);
        if matches!(ty, Type::Double) {
            self.emitf(format_args!("    movsd xmm0, QWORD PTR [rip + {}]", label));
        } else {
            self.emitf(format_args!("    mov rax, QWORD PTR [rip + {}]", label));
        }
    }

    fn store_var(&mut self, frame: &Frame, name: &str, ty: &Type) {
        if let Some(offset) = frame.offset_of(name) {
            if matches!(ty, Type::Double) {
                self.emitf(format_args!("    movsd QWORD PTR [rbp - {}], xmm0", offset));
            } else {
                self.emitf(format_args!("    mov QWORD PTR [rbp - {}], rax", offset));
            }
            return;
        }
        let label = Self::global_label(name);
        if matches!(ty, Type::Double) {
            self.emitf(format_args!("    movsd QWORD PTR [rip + {}], xmm0", label));
        } else {
            self.emitf(format_args!("    mov QWORD PTR [rip + {}], rax", label));
        }
    }

    // -----------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------

    fn emit_expr(&mut self, frame: &mut Frame, expr: &Expr) {
        let ty = expr.ty.clone().unwrap_or(Type::Unknown);
        match &expr.kind {
            ExprKind::Int(v) => self.emitf(format_args!("    mov rax, {}", v)),
            ExprKind::Bool(v) => {
                self.emitf(format_args!("    mov rax, {}", if *v { 1 } else { 0 }))
            }
            ExprKind::Double(v) => {
                if v.fract() == 0.0 && v.abs() < 1e15 {
                    let bits = v.to_bits();
                    self.emitf(format_args!("    mov rax, {}", bits));
                    self.emit("    movq xmm0, rax");
                } else {
                    let label = self.double_label(*v);
                    self.emitf(format_args!("    movsd xmm0, QWORD PTR [rip + {}]", label));
                }
            }
            ExprKind::Str(value) => {
                let label = self.string_label(value);
                self.emitf(format_args!("    lea rax, [rip + {}]", label));
            }
            ExprKind::Var(name) => {
                let slot_ty = frame.ty_of(name).unwrap_or_else(|| ty.clone());
                self.load_var(frame, name, &slot_ty);
            }
            ExprKind::ArrayLit(items) => self.emit_array_lit(frame, items),
            ExprKind::Index(base, index) => self.emit_index(frame, base, index, &ty),
            ExprKind::Field(base, name) => self.emit_field(frame, base, name, &ty),
            ExprKind::MethodCall(base, name, args) => {
                self.emit_call(frame, base, Some(name), args, &ty, true)
            }
            ExprKind::Call(name, args) => {
                if name == "random" {
                    self.emit_args(frame, args);
                    // Arguments were pushed left to right, so the last one
                    // (max) sits on top of the stack.
                    self.emit("    mov rdi, QWORD PTR [rsp + 16]");
                    self.emit("    mov rsi, QWORD PTR [rsp]");
                    self.emit("    add rsp, 32");
                    self.emit("    call __a_random_range");
                } else {
                    let base = Expr::new(ExprKind::Var(name.clone()), expr.span);
                    self.emit_call(frame, &base, None, args, &ty, false);
                }
            }
            ExprKind::StructLit(name, fields) => self.emit_struct_lit(frame, name, fields, &ty),
            ExprKind::Intrinsic(name, args) => self.emit_intrinsic(frame, name, args, &ty),
            ExprKind::Binary(op, left, right) => self.emit_binary(frame, *op, left, right, &ty),
            ExprKind::UnaryMinus(inner) => {
                let inner_ty = inner.ty.clone().unwrap_or(Type::Unknown);
                self.emit_expr(frame, inner);
                if matches!(inner_ty, Type::Double) {
                    self.emit("    mov rax, QWORD PTR [rip + .La_neg_zero]");
                    self.emit("    movq xmm1, rax");
                    self.emit("    xorpd xmm0, xmm1");
                } else {
                    self.emit("    neg rax");
                }
            }
            ExprKind::AddressOf(inner) => self.emit_address_of(frame, inner, &ty),
            ExprKind::Deref(inner) => {
                let inner_ty = inner.ty.clone().unwrap_or(Type::Unknown);
                self.emit_expr(frame, inner);
                let target = match inner_ty {
                    Type::Ptr(t) => *t,
                    _ => Type::Unknown,
                };
                if matches!(target, Type::Double) {
                    self.emit("    movsd xmm0, QWORD PTR [rax]");
                } else {
                    self.emit("    mov rax, QWORD PTR [rax]");
                }
            }
            ExprKind::Try(inner) => {
                // Unwrap `result`, or return the failure from this reuse.
                let ok_label = self.fresh_label("try_ok");
                self.emit_expr(frame, inner);
                self.emit("    mov r10, rax");
                self.emit("    mov rcx, QWORD PTR [rax]");
                self.emit("    test rcx, rcx");
                self.emitf(format_args!("    jne {}", ok_label));
                // Propagate: the failure object becomes this call's result.
                self.emit("    mov rax, r10");
                self.emit(&format!("    jmp {}", frame.return_label.clone()));
                self.emitf(format_args!("{}:", ok_label));
                self.emit("    mov rax, QWORD PTR [r10 + 16]");
                if matches!(ty, Type::Double) {
                    self.emit("    movq xmm0, rax");
                }
            }
        }
    }

    fn emit_args(&mut self, frame: &mut Frame, args: &[Expr]) {
        for arg in args {
            let arg_ty = arg.ty.clone().unwrap_or(Type::Unknown);
            self.emit_expr(frame, arg);
            if matches!(arg_ty, Type::Double) {
                self.push_xmm0();
            } else {
                self.push_rax();
            }
        }
    }

    /// Emit a call. Arguments are pushed, then popped into the System V
    /// registers in reverse order, so each lands in the right place without
    /// extra frame slots.
    fn emit_call(
        &mut self,
        frame: &mut Frame,
        base: &Expr,
        method: Option<&str>,
        args: &[Expr],
        ty: &Type,
        is_method: bool,
    ) {
        let mut all_args: Vec<Expr> = Vec::new();
        if is_method {
            all_args.push(base.clone());
        }
        all_args.extend(args.iter().cloned());

        // Resolve the symbol from the checked program.
        let symbol = self.resolve_symbol(frame, base, method, &all_args, ty);
        self.emit_args(frame, &all_args);

        let mut int_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"].iter();
        let mut sse_count = 0usize;
        let mut plan: Vec<String> = Vec::new();
        for arg in &all_args {
            let arg_ty = arg.ty.clone().unwrap_or(Type::Unknown);
            if matches!(arg_ty, Type::Double) {
                plan.push(format!("xmm{}", sse_count));
                sse_count += 1;
            } else {
                plan.push(int_regs.next().unwrap_or(&"r9").to_string());
            }
        }
        for reg in plan.iter().rev() {
            if reg.starts_with("xmm") {
                self.pop_xmm(reg);
            } else {
                self.pop_to(reg);
            }
        }
        self.emitf(format_args!("    call {}", symbol));
        if matches!(ty, Type::Double) {
            // value already in xmm0
        }
    }

    fn resolve_symbol(
        &mut self,
        _frame: &mut Frame,
        base: &Expr,
        method: Option<&str>,
        args: &[Expr],
        ty: &Type,
    ) -> String {
        let arg_types: Vec<Type> = args
            .iter()
            .map(|a| a.ty.clone().unwrap_or(Type::Unknown))
            .collect();
        if let Some(method_name) = method {
            let base_ty = base.ty.clone().unwrap_or(Type::Unknown);
            let type_name = match &base_ty {
                Type::Struct { name, .. } => name.clone(),
                Type::Enum(name) => name.clone(),
                Type::Ptr(inner) => match &**inner {
                    Type::Struct { name, .. } => name.clone(),
                    other => other.name(),
                },
                other => other.name(),
            };
            if let Some(key) = self.lookup_impl_instance(&type_name, method_name) {
                return self.checked.instance(&key).symbol.clone();
            }
        }
        let name = match &base.kind {
            ExprKind::Var(n) => n.clone(),
            _ => String::new(),
        };
        let canonical = self.canonical_of(&name, &arg_types);
        match self.find_instance_by_name(&canonical, &arg_types) {
            Some(inst) => inst.symbol.clone(),
            None => {
                let _ = ty;
                format!("__a_missing_{}", name)
            }
        }
    }

    fn lookup_impl_instance(&self, type_name: &str, method: &str) -> Option<String> {
        for ((_trait, ty), methods) in &self.checked.impls {
            if ty == type_name {
                if let Some(key) = methods.get(method) {
                    return Some(key.clone());
                }
            }
        }
        None
    }

    fn canonical_of(&self, name: &str, _arg_types: &[Type]) -> String {
        name.to_string()
    }

    fn find_instance_by_name(&self, name: &str, arg_types: &[Type]) -> Option<&ReuseInstance> {
        let mut best: Option<&ReuseInstance> = None;
        for inst in &self.checked.instances {
            if inst.display_name == name
                || inst.display_name.ends_with(&format!(".{}", name))
                || inst.display_name.ends_with(&format!("|{}", name))
            {
                let param_count = inst.params.len();
                if param_count == arg_types.len() {
                    best = Some(inst);
                    if inst
                        .params
                        .iter()
                        .zip(arg_types.iter())
                        .all(|(p, a)| p.compatible_with(a))
                    {
                        return Some(inst);
                    }
                }
            }
        }
        best
    }

    fn emit_array_lit(&mut self, frame: &mut Frame, items: &[Expr]) {
        self.emit("    call __a_array_create");
        for item in items {
            let item_ty = item.ty.clone().unwrap_or(Type::Unknown);
            self.emit("    mov rdi, rax");
            self.emit("    sub rsp, 16");
            self.emit("    mov QWORD PTR [rsp], rdi");
            self.emit_expr(frame, item);
            self.emitf(format_args!("    mov rsi, {}", tag_of(&item_ty)));
            if matches!(item_ty, Type::Double) {
                self.emit("    movq rdx, xmm0");
            } else {
                self.emit("    mov rdx, rax");
            }
            self.emit("    mov rdi, QWORD PTR [rsp]");
            self.emit("    add rsp, 16");
            self.emit("    call __a_array_append");
        }
    }

    fn emit_index(&mut self, frame: &mut Frame, base: &Expr, index: &Expr, ty: &Type) {
        let base_ty = base.ty.clone().unwrap_or(Type::Unknown);
        self.emit_expr(frame, base);
        self.push_rax();
        self.emit_expr(frame, index);
        self.pop_to("rdi");
        self.emit("    mov rsi, rax");
        self.emit("    call __a_array_get");
        match ty {
            Type::Double => self.emit("    movsd xmm0, QWORD PTR [rax + 8]"),
            // An element of a mixed array carries its own runtime tag; keep the
            // cell pointer so printing and @unbox see the tag.
            Type::Mixed | Type::Unknown | Type::Param(_) => {}
            _ => self.emit("    mov rax, QWORD PTR [rax + 8]"),
        }
        let _ = base_ty;
    }

    /// Load the value of a struct field (or a synthetic field such as
    /// `.length`, `.ok`, `.value`, `.error`).
    fn emit_field(&mut self, frame: &mut Frame, base: &Expr, name: &str, ty: &Type) {
        let base_ty = base.ty.clone().unwrap_or(Type::Unknown);
        match &base_ty {
            Type::Struct { .. } => self.emit_struct_field(frame, base, name, ty),
            Type::Ptr(inner) => match &**inner {
                Type::Struct { .. } => self.emit_struct_field(frame, base, name, ty),
                other => {
                    let _ = other;
                    self.emit_expr(frame, base);
                }
            },
            Type::Enum(_) => {
                if name == "value" {
                    self.emit_expr(frame, base);
                    self.emit("    mov rax, QWORD PTR [rax]");
                } else {
                    // Enum variant constant: `Color.red`
                    self.emit_enum_constant(base, name);
                }
            }
            Type::Array(_) => {
                self.emit_expr(frame, base);
                self.emit("    mov rax, QWORD PTR [rax]");
            }
            Type::String => {
                self.emit_expr(frame, base);
                self.emit("    mov rdi, rax");
                self.emit("    call __a_str_len");
            }
            Type::Result(_) => {
                let label = self.fresh_label("res");
                self.emit_expr(frame, base);
                self.emit("    mov r10, rax");
                match name {
                    "ok" => {
                        self.emit("    mov rax, QWORD PTR [r10]");
                    }
                    "value" => {
                        self.emit("    mov rax, QWORD PTR [r10 + 16]");
                        if matches!(ty, Type::Double) {
                            self.emit("    movq xmm0, rax");
                        }
                    }
                    "error" => {
                        self.emit("    mov rax, QWORD PTR [r10 + 24]");
                    }
                    _ => {}
                }
                let _ = label;
            }
            _ => {
                // Unknown base (e.g. a variant of an enum reached through a
                // module path). Fall back to the constant form.
                self.emit_enum_constant(base, name);
            }
        }
    }

    fn emit_struct_field(&mut self, frame: &mut Frame, base: &Expr, name: &str, ty: &Type) {
        let base_ty = base.ty.clone().unwrap_or(Type::Unknown);
        let struct_ty = match &base_ty {
            Type::Ptr(inner) => (**inner).clone(),
            other => other.clone(),
        };
        let index = self
            .checked
            .struct_fields
            .get(&struct_ty.name())
            .and_then(|fields| fields.iter().position(|(f, _)| f == name))
            .unwrap_or(0);
        self.emit_expr(frame, base);
        let offset = 16 + index * 16;
        if matches!(ty, Type::Double) {
            self.emitf(format_args!(
                "    movsd xmm0, QWORD PTR [rax + {}]",
                offset + 8
            ));
        } else {
            self.emitf(format_args!(
                "    mov rax, QWORD PTR [rax + {}]",
                offset + 8
            ));
        }
    }

    fn emit_enum_constant(&mut self, base: &Expr, variant: &str) {
        let mut path: Vec<String> = Vec::new();
        let mut current = base;
        loop {
            match &current.kind {
                ExprKind::Var(n) => {
                    path.insert(0, n.clone());
                    break;
                }
                ExprKind::Field(inner, n) => {
                    path.insert(0, n.clone());
                    current = inner;
                }
                _ => break,
            }
        }
        let enum_name = path.join(".");
        let resolved = if self.checked.enums.contains_key(&enum_name) {
            enum_name
        } else {
            self.checked
                .enums
                .keys()
                .find(|k| k.ends_with(&format!(".{}", enum_name)) || *k == &enum_name)
                .cloned()
                .unwrap_or(enum_name)
        };
        let label = format!(
            "__a_enum_{}_{}",
            type_symbol(&Type::Enum(resolved.clone())),
            variant
        );
        self.emitf(format_args!("    lea rax, [rip + {}]", label));
    }

    fn emit_struct_lit(
        &mut self,
        frame: &mut Frame,
        name: &str,
        fields: &[(String, Expr)],
        ty: &Type,
    ) {
        let struct_name = match ty {
            Type::Struct { name, .. } => name.clone(),
            _ => name.to_string(),
        };
        let layout: Vec<(String, Type)> = self
            .checked
            .struct_fields
            .get(&ty.name())
            .or_else(|| self.checked.struct_fields.get(&struct_name))
            .cloned()
            .unwrap_or_default();
        self.emitf(format_args!("    mov rdi, {}", layout.len()));
        self.emit("    call __a_struct_new");
        for (index, (field_name, _field_ty)) in layout.iter().enumerate() {
            let value = match fields.iter().find(|(n, _)| n == field_name) {
                Some((_, e)) => e,
                None => continue,
            };
            let value_ty = value.ty.clone().unwrap_or(Type::Unknown);
            self.push_rax();
            self.emit_expr(frame, value);
            if !matches!(value_ty, Type::Double) {
                self.emit("    mov rdx, rax");
            } else {
                self.emit("    movq rdx, xmm0");
            }
            self.emit("    mov rdi, QWORD PTR [rsp]");
            self.emitf(format_args!("    mov rsi, {}", tag_of(&value_ty)));
            self.emitf(format_args!("    mov rcx, {}", index));
            self.emit("    call __a_struct_set");
            self.emit("    mov rax, QWORD PTR [rsp]");
            self.emit("    add rsp, 16");
        }
    }

    fn emit_intrinsic(&mut self, frame: &mut Frame, name: &str, args: &[Expr], ty: &Type) {
        match name {
            "alloc" => {
                self.emit_args(frame, args);
                self.emit("    mov rdi, QWORD PTR [rsp]");
                self.emit("    add rsp, 16");
                self.emit("    call __a_alloc");
            }
            "free" => {
                self.emit_args(frame, args);
                self.emit("    mov rdi, QWORD PTR [rsp]");
                self.emit("    add rsp, 16");
                self.emit("    call __a_release");
            }
            "cell" => {
                // One 8-byte slot holding the value.
                self.emitf(format_args!("    mov rdi, 16"));
                self.emit("    call __a_alloc");
                self.push_rax();
                self.emit_expr(frame, &args[0]);
                if !matches!(args[0].ty.clone().unwrap_or(Type::Unknown), Type::Double) {
                    self.emit("    mov rdx, rax");
                } else {
                    self.emit("    movq rdx, xmm0");
                }
                self.emit("    mov rdi, QWORD PTR [rsp]");
                self.emit("    add rsp, 16");
                self.emit("    mov QWORD PTR [rdi], rdx");
                self.emit("    mov rax, rdi");
            }
            "unbox" => {
                self.emit_expr(frame, &args[0]);
                if matches!(ty, Type::Double) {
                    self.emit("    movsd xmm0, QWORD PTR [rax]");
                } else {
                    self.emit("    mov rax, QWORD PTR [rax]");
                }
            }
            "mem_copy" => {
                self.emit_args(frame, args);
                self.pop_to("rdx");
                self.pop_to("rsi");
                self.pop_to("rdi");
                self.emit("    call memcpy");
            }
            "mem_set" => {
                self.emit_args(frame, args);
                self.pop_to("rdx");
                self.pop_to("rsi");
                self.pop_to("rdi");
                self.emit("    call memset");
            }
            "mem_read8" => {
                self.emit_expr(frame, &args[0]);
                self.emit("    movzx rax, BYTE PTR [rax]");
            }
            "mem_write8" => {
                self.emit_args(frame, args);
                self.pop_to("rsi");
                self.pop_to("rdi");
                self.emit("    mov BYTE PTR [rdi], sil");
            }
            "mem_read64" => {
                self.emit_expr(frame, &args[0]);
                self.emit("    mov rax, QWORD PTR [rax]");
            }
            "mem_write64" => {
                self.emit_args(frame, args);
                self.pop_to("rsi");
                self.pop_to("rdi");
                self.emit("    mov QWORD PTR [rdi], rsi");
            }
            "live_allocs" => self.emit("    call __a_live_allocs"),
            "popcount" => {
                self.emit_expr(frame, &args[0]);
                self.emit("    popcnt rax, rax");
            }
            "clz" => {
                self.emit_expr(frame, &args[0]);
                self.emit("    mov rdi, rax");
                self.emit("    call __a_clz");
            }
            "ctz" => {
                self.emit_expr(frame, &args[0]);
                self.emit("    mov rdi, rax");
                self.emit("    call __a_ctz");
            }
            "bswap" => {
                self.emit_expr(frame, &args[0]);
                self.emit("    bswap rax");
            }
            "rotl" | "rotr" => {
                self.emit_args(frame, args);
                self.pop_to("rsi");
                self.pop_to("rdi");
                self.emit("    mov rax, rdi");
                self.emit("    mov ecx, esi");
                if name == "rotl" {
                    self.emit("    rol rax, cl");
                } else {
                    self.emit("    ror rax, cl");
                }
            }
            "atomic_add" | "atomic_sub" => {
                self.emit_args(frame, args);
                self.pop_to("rsi");
                self.pop_to("rdi");
                if name == "atomic_sub" {
                    self.emit("    neg rsi");
                }
                self.emit("    mov rax, rsi");
                self.emit("    lock xadd QWORD PTR [rdi], rax");
            }
            "atomic_swap" => {
                self.emit_args(frame, args);
                self.pop_to("rsi");
                self.pop_to("rdi");
                self.emit("    mov rax, rsi");
                self.emit("    xchg QWORD PTR [rdi], rax");
            }
            "atomic_cas" => {
                self.emit_args(frame, args);
                self.pop_to("rdx");
                self.pop_to("rsi");
                self.pop_to("rdi");
                self.emit("    mov rax, rsi");
                self.emit("    lock cmpxchg QWORD PTR [rdi], rdx");
                self.emit("    sete cl");
                self.emit("    movzx rax, cl");
            }
            "fence" => self.emit("    mfence"),
            "arch" => {
                let label = self.string_label(crate::runtime::ARCH);
                self.emitf(format_args!("    lea rax, [rip + {}]", label));
            }
            "os" => {
                let label = self.string_label(crate::runtime::OS);
                self.emitf(format_args!("    lea rax, [rip + {}]", label));
            }
            "pointer_bits" => self.emit("    mov rax, 64"),
            "has_atomics" => self.emit("    mov rax, 1"),
            other => {
                self.emitf(format_args!("    # unsupported intrinsic @{}", other));
                self.emit("    mov rax, 0");
            }
        }
    }

    fn emit_binary(&mut self, frame: &mut Frame, op: BinOp, left: &Expr, right: &Expr, ty: &Type) {
        let left_ty = left.ty.clone().unwrap_or(Type::Unknown);
        let right_ty = right.ty.clone().unwrap_or(Type::Unknown);
        let is_double = matches!(ty, Type::Double)
            || matches!(left_ty, Type::Double)
            || matches!(right_ty, Type::Double);

        // String equality needs a content comparison.
        if matches!((&left_ty, &right_ty), (Type::String, Type::String))
            && matches!(op, BinOp::Eq | BinOp::Ne)
        {
            self.emit_expr(frame, left);
            self.push_rax();
            self.emit_expr(frame, right);
            self.pop_to("rdi");
            self.emit("    mov rsi, rax");
            self.emit("    call __a_str_eq");
            if op == BinOp::Ne {
                self.emit("    xor rax, 1");
            }
            return;
        }

        self.emit_expr(frame, left);
        if is_double {
            if matches!(left_ty, Type::Double) {
                self.push_xmm0();
            } else {
                self.emit("    cvtsi2sd xmm0, rax");
                self.push_xmm0();
            }
        } else {
            self.push_rax();
        }
        self.emit_expr(frame, right);

        match op {
            BinOp::And => {
                self.pop_to("rdi");
                self.emit("    and rax, rdi");
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                if is_double {
                    if matches!(right_ty, Type::Double) {
                        // xmm0 holds right
                    } else {
                        self.emit("    cvtsi2sd xmm0, rax");
                    }
                    self.pop_xmm("xmm1");
                    self.emit("    ucomisd xmm1, xmm0");
                    let cc = match op {
                        BinOp::Lt => "setb",
                        BinOp::Gt => "seta",
                        BinOp::Le => "setbe",
                        BinOp::Ge => "setae",
                        BinOp::Eq => "sete",
                        _ => "setne",
                    };
                    self.emitf(format_args!("    {} al", cc));
                    self.emit("    movzx rax, al");
                } else {
                    self.pop_to("rdi");
                    self.emit("    cmp rdi, rax");
                    let cc = match op {
                        BinOp::Lt => "setl",
                        BinOp::Gt => "setg",
                        BinOp::Le => "setle",
                        BinOp::Ge => "setge",
                        BinOp::Eq => "sete",
                        _ => "setne",
                    };
                    self.emitf(format_args!("    {} al", cc));
                    self.emit("    movzx rax, al");
                }
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                if is_double {
                    if !matches!(right_ty, Type::Double) {
                        self.emit("    cvtsi2sd xmm0, rax");
                    }
                    self.pop_xmm("xmm1");
                    match op {
                        BinOp::Add => self.emit("    addsd xmm1, xmm0"),
                        BinOp::Sub => self.emit("    subsd xmm1, xmm0"),
                        BinOp::Mul => self.emit("    mulsd xmm1, xmm0"),
                        BinOp::Div => {
                            self.emit("    xorpd xmm2, xmm2");
                            self.emit("    ucomisd xmm0, xmm2");
                            self.emit("    je __a_error_div_zero");
                            self.emit("    divsd xmm1, xmm0");
                        }
                        BinOp::Mod => {
                            // Integer-only.
                        }
                        _ => {}
                    }
                    self.emit("    movsd xmm0, xmm1");
                } else {
                    self.pop_to("rdi");
                    match op {
                        BinOp::Add => self.emit("    add rax, rdi"),
                        BinOp::Sub => {
                            self.emit("    sub rdi, rax");
                            self.emit("    mov rax, rdi");
                        }
                        BinOp::Mul => self.emit("    imul rax, rdi"),
                        BinOp::Div => {
                            self.emit("    mov r8, rax");
                            self.emit("    test r8, r8");
                            self.emit("    je __a_error_div_zero");
                            self.emit("    mov rax, rdi");
                            self.emit("    cqo");
                            self.emit("    idiv r8");
                        }
                        BinOp::Mod => {
                            self.emit("    mov r8, rax");
                            self.emit("    test r8, r8");
                            self.emit("    je __a_error_div_zero");
                            self.emit("    mov rax, rdi");
                            self.emit("    cqo");
                            self.emit("    idiv r8");
                            self.emit("    mov rax, rdx");
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn emit_address_of(&mut self, frame: &mut Frame, inner: &Expr, ty: &Type) {
        let _ = ty;
        match &inner.kind {
            ExprKind::Var(name) => {
                if self.is_global(name) {
                    let label = Self::global_label(name);
                    self.emitf(format_args!("    lea rax, [rip + {}]", label));
                } else if let Some(offset) = frame.offset_of(name) {
                    self.emitf(format_args!("    lea rax, [rbp - {}]", offset));
                } else {
                    self.emit("    xor rax, rax");
                }
            }
            ExprKind::Field(base, name) => {
                self.emit_lvalue_addr(frame, base, Some(name));
            }
            ExprKind::Index(base, index) => {
                let element = match base.ty.clone().unwrap_or(Type::Unknown) {
                    Type::Array(elem) => *elem,
                    _ => Type::Unknown,
                };
                self.emit_expr(frame, base);
                self.push_rax();
                self.emit_expr(frame, index);
                self.pop_to("rdi");
                self.emit("    mov rsi, rax");
                self.emit("    call __a_array_get");
                let _ = element;
            }
            ExprKind::Deref(p) => {
                self.emit_expr(frame, p);
            }
            _ => {
                // Address of a temporary: materialise it into a fresh cell.
                self.emitf(format_args!("    mov rdi, 16"));
                self.emit("    call __a_alloc");
                self.push_rax();
                self.emit_expr(frame, inner);
                if !matches!(inner.ty.clone().unwrap_or(Type::Unknown), Type::Double) {
                    self.emit("    mov rdx, rax");
                } else {
                    self.emit("    movq rdx, xmm0");
                }
                self.emit("    pop rax");
                self.emit("    mov QWORD PTR [rax], rdx");
            }
        }
    }

    /// Address of a struct field (used for both loading and storing).
    fn emit_lvalue_addr(&mut self, frame: &mut Frame, base: &Expr, field: Option<&str>) {
        let base_ty = base.ty.clone().unwrap_or(Type::Unknown);
        let struct_ty = match &base_ty {
            Type::Ptr(inner) => (**inner).clone(),
            other => other.clone(),
        };
        let field_name = field.unwrap_or_default();
        let index = self
            .checked
            .struct_fields
            .get(&struct_ty.name())
            .and_then(|fields| fields.iter().position(|(f, _)| f == field_name))
            .unwrap_or(0);
        self.emit_expr(frame, base);
        self.emitf(format_args!("    add rax, {}", 16 + index * 16 + 8));
    }

    // -----------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------

    fn emit_stmts(&mut self, frame: &mut Frame, stmts: &[Stmt]) {
        for stmt in stmts {
            self.emit_stmt(frame, stmt);
        }
    }

    fn emit_stmt(&mut self, frame: &mut Frame, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Set { name, expr, .. } => {
                let ty = expr.ty.clone().unwrap_or(Type::Unknown);
                if !self.is_global(name) {
                    frame.alloc(name, ty.clone());
                }
                self.emit_expr(frame, expr);
                self.store_var(frame, name, &ty);
            }
            StmtKind::Assign { name, expr } => {
                let ty = frame.ty_of(name).unwrap_or_else(|| {
                    self.checked
                        .globals
                        .get(name)
                        .map(|g| g.ty.clone())
                        .unwrap_or(Type::Unknown)
                });
                self.emit_expr(frame, expr);
                self.store_var(frame, name, &ty);
            }
            StmtKind::AssignTo { target, expr } => {
                let value_ty = expr.ty.clone().unwrap_or(Type::Unknown);
                match &target.kind {
                    ExprKind::Var(name) => {
                        let ty = frame.ty_of(name).unwrap_or(value_ty.clone());
                        self.emit_expr(frame, expr);
                        self.store_var(frame, name, &ty);
                    }
                    ExprKind::Index(base, index) => {
                        let elem_ty = match base.ty.clone().unwrap_or(Type::Unknown) {
                            Type::Array(elem) => *elem,
                            _ => value_ty.clone(),
                        };
                        self.emit_expr(frame, base);
                        self.push_rax();
                        self.emit_expr(frame, index);
                        self.push_rax();
                        self.emit_expr(frame, expr);
                        if matches!(value_ty, Type::Double) {
                            self.emit("    movq rcx, xmm0");
                        } else {
                            self.emit("    mov rcx, rax");
                        }
                        self.pop_to("rsi");
                        self.pop_to("rdi");
                        self.emitf(format_args!("    mov rdx, {}", tag_of(&value_ty)));
                        self.emit("    call __a_array_set");
                        let _ = elem_ty;
                    }
                    ExprKind::Field(base, name) => {
                        self.emit_lvalue_addr(frame, base, Some(name));
                        self.push_rax();
                        self.emit_expr(frame, expr);
                        if matches!(value_ty, Type::Double) {
                            self.emit("    movq rcx, xmm0");
                        } else {
                            self.emit("    mov rcx, rax");
                        }
                        self.pop_to("rdi");
                        self.emit("    mov QWORD PTR [rdi], rcx");
                    }
                    ExprKind::Deref(pointer) => {
                        self.emit_expr(frame, pointer);
                        self.push_rax();
                        self.emit_expr(frame, expr);
                        if matches!(value_ty, Type::Double) {
                            self.emit("    movq rcx, xmm0");
                            self.pop_to("rdi");
                            self.emit("    mov QWORD PTR [rdi], rcx");
                        } else {
                            self.emit("    mov rcx, rax");
                            self.pop_to("rdi");
                            self.emit("    mov QWORD PTR [rdi], rcx");
                        }
                    }
                    _ => {}
                }
            }
            StmtKind::Insert { name, op, expr } => {
                let var_ty = frame
                    .ty_of(name)
                    .or_else(|| self.checked.globals.get(name).map(|g| g.ty.clone()))
                    .unwrap_or(Type::Unknown);
                match &var_ty {
                    Type::Array(elem) => {
                        self.load_var(frame, name, &var_ty);
                        self.push_rax();
                        self.emit_expr(frame, expr);
                        let value_ty = expr.ty.clone().unwrap_or(Type::Unknown);
                        if matches!(value_ty, Type::Double) {
                            self.emit("    movq rdx, xmm0");
                        } else {
                            self.emit("    mov rdx, rax");
                        }
                        self.emitf(format_args!("    mov rsi, {}", tag_of(&value_ty)));
                        self.pop_to("rdi");
                        self.emit("    call __a_array_append");
                        let _ = elem;
                    }
                    ty => {
                        let is_double = matches!(ty, Type::Double);
                        self.load_var(frame, name, ty);
                        if is_double {
                            self.push_xmm0();
                        } else {
                            self.push_rax();
                        }
                        self.emit_expr(frame, expr);
                        if is_double {
                            if !matches!(expr.ty.clone().unwrap_or(Type::Unknown), Type::Double) {
                                self.emit("    cvtsi2sd xmm0, rax");
                            }
                            self.pop_xmm("xmm1");
                            match op {
                                InsertOp::Add => self.emit("    addsd xmm1, xmm0"),
                                InsertOp::Sub => self.emit("    subsd xmm1, xmm0"),
                                InsertOp::Mul => self.emit("    mulsd xmm1, xmm0"),
                                InsertOp::Div => {
                                    self.emit("    xorpd xmm2, xmm2");
                                    self.emit("    ucomisd xmm0, xmm2");
                                    self.emit("    je __a_error_div_zero");
                                    self.emit("    divsd xmm1, xmm0");
                                }
                            }
                            self.emit("    movsd xmm0, xmm1");
                        } else {
                            self.pop_to("rdi");
                            match op {
                                InsertOp::Add => self.emit("    add rax, rdi"),
                                InsertOp::Sub => {
                                    self.emit("    sub rdi, rax");
                                    self.emit("    mov rax, rdi");
                                }
                                InsertOp::Mul => self.emit("    imul rax, rdi"),
                                InsertOp::Div => {
                                    self.emit("    mov r8, rax");
                                    self.emit("    test r8, r8");
                                    self.emit("    je __a_error_div_zero");
                                    self.emit("    mov rax, rdi");
                                    self.emit("    cqo");
                                    self.emit("    idiv r8");
                                }
                            }
                        }
                        self.store_var(frame, name, ty);
                    }
                }
            }
            StmtKind::Remove { name, index } => {
                let var_ty = frame
                    .ty_of(name)
                    .or_else(|| self.checked.globals.get(name).map(|g| g.ty.clone()))
                    .unwrap_or(Type::Unknown);
                self.load_var(frame, name, &var_ty);
                self.push_rax();
                self.emit_expr(frame, index);
                self.pop_to("rdi");
                self.emit("    mov rsi, rax");
                self.emit("    call __a_array_remove");
            }
            StmtKind::Aout { args } => {
                self.emit("    call __a_print_lock_acquire");
                for arg in args {
                    match arg {
                        AoutArg::Literal(text, _) => {
                            let label = self.string_label(text);
                            self.emitf(format_args!("    lea rdi, [rip + {}]", label));
                            self.emit("    call __a_print_string");
                        }
                        AoutArg::Evaluated(expr) => {
                            let ty = expr.ty.clone().unwrap_or(Type::Unknown);
                            self.emit_expr(frame, expr);
                            self.emit_print_value(&ty);
                        }
                    }
                }
                self.emit("    call __a_print_newline");
                self.emit("    call __a_print_lock_release");
            }
            StmtKind::When {
                branches,
                else_body,
            } => {
                let end = self.fresh_label("when_end");
                for (cond, body) in branches {
                    let next = self.fresh_label("when_next");
                    self.emit_expr(frame, cond);
                    self.emit("    test rax, rax");
                    self.emitf(format_args!("    je {}", next));
                    self.emit_stmts(frame, body);
                    self.emitf(format_args!("    jmp {}", end));
                    self.emitf(format_args!("{}:", next));
                }
                if let Some(body) = else_body {
                    self.emit_stmts(frame, body);
                }
                self.emitf(format_args!("{}:", end));
            }
            StmtKind::Loop { body, count } => match count {
                Some(count_expr) => {
                    let index_slot = frame.max_offset + 8;
                    let limit_slot = index_slot + 8;
                    frame.max_offset = limit_slot;
                    self.emit_expr(frame, count_expr);
                    self.emitf(format_args!(
                        "    mov QWORD PTR [rbp - {}], rax",
                        limit_slot
                    ));
                    self.emitf(format_args!("    mov QWORD PTR [rbp - {}], 0", index_slot));
                    let start = self.fresh_label("loop");
                    let end = self.fresh_label("loop_end");
                    self.emitf(format_args!("{}:", start));
                    self.emitf(format_args!(
                        "    mov rax, QWORD PTR [rbp - {}]",
                        index_slot
                    ));
                    self.emitf(format_args!(
                        "    cmp rax, QWORD PTR [rbp - {}]",
                        limit_slot
                    ));
                    self.emitf(format_args!("    jae {}", end));
                    self.emit_stmts(frame, body);
                    self.emitf(format_args!("    inc QWORD PTR [rbp - {}]", index_slot));
                    self.emitf(format_args!("    jmp {}", start));
                    self.emitf(format_args!("{}:", end));
                }
                None => self.emit_stmts(frame, body),
            },
            StmtKind::While {
                cond,
                body,
                cleanup,
            } => {
                let start = self.fresh_label("while");
                let done = self.fresh_label("while_done");
                self.emitf(format_args!("{}:", start));
                self.emit_expr(frame, cond);
                self.emit("    test rax, rax");
                self.emitf(format_args!("    je {}", done));
                self.emit_stmts(frame, body);
                self.emitf(format_args!("    jmp {}", start));
                self.emitf(format_args!("{}:", done));
                self.emit_stmts(frame, cleanup);
            }
            StmtKind::Each {
                var,
                iterable,
                body,
            } => {
                let elem_ty = match iterable.ty.clone().unwrap_or(Type::Unknown) {
                    Type::Array(elem) => *elem,
                    _ => Type::Unknown,
                };
                frame.alloc(var, elem_ty.clone());
                let array_slot = frame.max_offset + 8;
                let index_slot = array_slot + 8;
                frame.max_offset = index_slot;
                self.emit_expr(frame, iterable);
                self.emitf(format_args!(
                    "    mov QWORD PTR [rbp - {}], rax",
                    array_slot
                ));
                self.emitf(format_args!("    mov QWORD PTR [rbp - {}], 1", index_slot));
                let start = self.fresh_label("each");
                let end = self.fresh_label("each_end");
                self.emitf(format_args!("{}:", start));
                self.emitf(format_args!(
                    "    mov rcx, QWORD PTR [rbp - {}]",
                    index_slot
                ));
                self.emitf(format_args!(
                    "    mov rax, QWORD PTR [rbp - {}]",
                    array_slot
                ));
                self.emit("    cmp rcx, QWORD PTR [rax]");
                self.emitf(format_args!("    jg {}", end));
                self.emit("    mov rdx, QWORD PTR [rax + 16]");
                self.emit("    dec rcx");
                self.emit("    shl rcx, 4");
                self.emit("    add rdx, rcx");
                if matches!(elem_ty, Type::Double) {
                    self.emit("    movsd xmm0, QWORD PTR [rdx + 8]");
                } else {
                    self.emit("    mov rax, QWORD PTR [rdx + 8]");
                }
                self.store_var(frame, var, &elem_ty);
                self.emit_stmts(frame, body);
                self.emitf(format_args!("    inc QWORD PTR [rbp - {}]", index_slot));
                self.emitf(format_args!("    jmp {}", start));
                self.emitf(format_args!("{}:", end));
            }
            StmtKind::Return { expr } => {
                if self.in_fallible() {
                    match expr {
                        Some(e) => {
                            let ty = e.ty.clone().unwrap_or(Type::Unknown);
                            self.emit_expr(frame, e);
                            if matches!(ty, Type::Double) {
                                self.emit("    movq rsi, xmm0");
                            } else {
                                self.emit("    mov rsi, rax");
                            }
                            self.emitf(format_args!("    mov rdi, {}", tag_of(&ty)));
                            self.emit("    call __a_result_ok");
                        }
                        None => {
                            self.emit("    mov rdi, 0");
                            self.emit("    xor rsi, rsi");
                            self.emit("    call __a_result_ok");
                        }
                    }
                } else if let Some(e) = expr {
                    self.emit_expr(frame, e);
                }
                let label = frame.return_label.clone();
                self.emitf(format_args!("    jmp {}", label));
            }
            StmtKind::Fail { expr } => {
                let ty = expr.ty.clone().unwrap_or(Type::Unknown);
                self.emit_expr(frame, expr);
                if self.in_fallible() {
                    if matches!(ty, Type::Int) {
                        self.emit("    mov rdi, rax");
                        self.emit("    call __a_int_to_string");
                    } else if matches!(ty, Type::Enum(_)) {
                        self.emit("    mov rax, QWORD PTR [rax + 8]");
                    }
                    self.emit("    mov rdi, rax");
                    self.emit("    call __a_result_err");
                    let label = frame.return_label.clone();
                    self.emitf(format_args!("    jmp {}", label));
                } else {
                    if matches!(ty, Type::Enum(_)) {
                        self.emit("    mov rax, QWORD PTR [rax + 8]");
                    } else if matches!(ty, Type::Int) {
                        self.emit("    mov rdi, rax");
                        self.emit("    call __a_int_to_string");
                    }
                    self.emit("    mov rdi, rax");
                    self.emit("    call __a_fail_exit");
                }
            }
            StmtKind::ExprStmt { expr } => self.emit_expr(frame, expr),
        }
    }

    fn in_fallible(&self) -> bool {
        self.fallible_stack.last().copied().unwrap_or(false)
    }

    fn emit_print_value(&mut self, ty: &Type) {
        match ty {
            Type::Int => {
                self.emit("    mov rdi, rax");
                self.emit("    call __a_print_int");
            }
            Type::Bool => {
                self.emit("    mov rdi, rax");
                self.emit("    call __a_print_bool");
            }
            Type::Double => self.emit("    call __a_print_double"),
            Type::String => {
                self.emit("    mov rdi, rax");
                self.emit("    call __a_print_string");
            }
            Type::Array(_) => {
                self.emit("    mov rdi, rax");
                self.emit("    call __a_print_array");
            }
            Type::Struct { .. } => {
                let key = ty.name();
                let helper = format!("__a_print_struct_{}", type_symbol(ty));
                if !self.struct_prints.iter().any(|t| t.name() == key) {
                    self.struct_prints.push(ty.clone());
                }
                self.emit("    mov rdi, rax");
                self.emitf(format_args!("    call {}", helper));
            }
            Type::Enum(_) => {
                self.emit("    mov rax, QWORD PTR [rax + 8]");
                self.emit("    mov rdi, rax");
                self.emit("    call __a_print_string");
            }
            Type::Ptr(_) => {
                self.emit("    mov rdi, rax");
                self.emit("    call __a_print_ptr");
            }
            Type::Result(_) => {
                self.emit("    mov rdi, rax");
                self.emit("    call __a_print_result");
            }
            Type::Void => {
                let label = self.string_label("void");
                self.emitf(format_args!("    lea rdi, [rip + {}]", label));
                self.emit("    call __a_print_string");
            }
            Type::Mixed | Type::Unknown | Type::Param(_) => {
                self.emit("    mov rdi, rax");
                self.emit("    call __a_print_cell");
            }
        }
    }

    // -----------------------------------------------------------------
    // Function generation
    // -----------------------------------------------------------------

    fn emit_function(
        &mut self,
        symbol: &str,
        params: &[(String, Type)],
        fallible: bool,
        owned: Vec<(String, Type)>,
        body: impl FnOnce(&mut Gen<'a>, &mut Frame),
    ) {
        let return_label = self.fresh_label("ret");
        let mut frame = Frame::new(return_label.clone());
        for (name, ty) in params {
            frame.alloc(name, ty.clone());
        }
        frame.owned = owned;

        self.begin_body();
        // Store incoming parameters.
        let mut int_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"].iter();
        let mut sse = 0usize;
        let param_regs: Vec<String> = params
            .iter()
            .map(|(_, ty)| {
                if matches!(ty, Type::Double) {
                    let r = format!("xmm{}", sse);
                    sse += 1;
                    r
                } else {
                    int_regs.next().unwrap_or(&"r9").to_string()
                }
            })
            .collect();
        for ((name, ty), reg) in params.iter().zip(param_regs.iter()) {
            if matches!(ty, Type::Double) {
                self.emitf(format_args!(
                    "    movsd QWORD PTR [rbp - {}], {}",
                    frame.offset_of(name).unwrap(),
                    reg
                ));
            } else {
                self.emitf(format_args!(
                    "    mov QWORD PTR [rbp - {}], {}",
                    frame.offset_of(name).unwrap(),
                    reg
                ));
            }
        }
        self.fallible_stack.push(fallible);
        body(self, &mut frame);
        self.fallible_stack.pop();

        // Epilogue: preserve the return value, release owned locals, return.
        let owned = frame.owned.clone();
        self.emitf(format_args!("{}:", return_label));
        if !owned.is_empty() || fallible {
            self.emitf(format_args!(
                "    mov QWORD PTR [rbp - {}], rax",
                frame.ret_slot
            ));
            self.emitf(format_args!(
                "    movsd QWORD PTR [rbp - {}], xmm0",
                frame.ret_slot_hi
            ));
            for (name, ty) in &owned {
                if let Some(offset) = frame.offset_of(name) {
                    self.emitf(format_args!("    mov rdi, QWORD PTR [rbp - {}]", offset));
                    self.emit("    test rdi, rdi");
                    self.emit("    je 1f");
                    // Arrays own a second allocation for their elements.
                    let helper = if matches!(ty, Type::Array(_)) {
                        "__a_array_free"
                    } else {
                        "__a_release"
                    };
                    self.emitf(format_args!("    call {}", helper));
                    self.emit("1:");
                }
            }
            self.emitf(format_args!(
                "    mov rax, QWORD PTR [rbp - {}]",
                frame.ret_slot
            ));
            self.emitf(format_args!(
                "    movsd xmm0, QWORD PTR [rbp - {}]",
                frame.ret_slot_hi
            ));
        }
        self.emit("    leave");
        self.emit("    ret");
        let body_text = self.take_body();

        let frame_size = ((frame.max_offset + 16) / 16) * 16 + 16;
        self.emitf(format_args!(".type {}, @function", symbol));
        self.emitf(format_args!("{}:", symbol));
        self.emit("    push rbp");
        self.emit("    mov rbp, rsp");
        self.emitf(format_args!("    sub rsp, {}", frame_size));
        // Neutralise the frame so stale data is never read.
        self.emit("    xor eax, eax");
        self.emitf(format_args!(
            "    mov QWORD PTR [rbp - {}], rax",
            frame.ret_slot
        ));
        self.emitf(format_args!(
            "    mov QWORD PTR [rbp - {}], rax",
            frame.ret_slot_hi
        ));
        self.out.push_str(&body_text);
    }

    fn gen_instances(&mut self) {
        let instances = self.checked.instances.clone();
        for inst in &instances {
            self.gen_instance(inst);
        }
        // Struct print helpers may be requested while generating instances.
        let mut done = 0;
        while done < self.struct_prints.len() {
            let ty = self.struct_prints[done].clone();
            done += 1;
            self.gen_struct_printer(&ty);
        }
    }

    fn gen_instance(&mut self, inst: &ReuseInstance) {
        let params: Vec<(String, Type)> = inst
            .decl
            .params
            .iter()
            .zip(inst.params.iter())
            .map(|(p, t)| (p.name.clone(), t.clone()))
            .collect();
        let owned = self.owned_of(&inst.decl.body);
        let module = inst.decl.module.clone();
        let prev_module = std::mem::replace(&mut self.module, module);
        let symbol = inst.symbol.clone();
        let body = inst.decl.body.clone();
        let fallible = inst.fallible;
        self.emit_function(&symbol, &params, fallible, owned, |gen, frame| {
            gen.emit_stmts(frame, &body);
            // A reuse that falls off the end returns its zero value.
            if fallible {
                gen.emit("    mov rdi, 0");
                gen.emit("    xor rsi, rsi");
                gen.emit("    call __a_result_ok");
            } else {
                gen.emit("    xor eax, eax");
            }
            let label = frame.return_label.clone();
            gen.emitf(format_args!("    jmp {}", label));
        });
        self.module = prev_module;
    }

    fn gen_struct_printer(&mut self, ty: &Type) {
        let fields = self
            .checked
            .struct_fields
            .get(&ty.name())
            .cloned()
            .unwrap_or_default();
        let symbol = format!("__a_print_struct_{}", type_symbol(ty));
        let name = match ty {
            Type::Struct { name, .. } => name.clone(),
            _ => String::new(),
        };
        let header = self.string_label(&format!("{} {{", name));
        let mut field_strings = Vec::new();
        for (index, (field, _)) in fields.iter().enumerate() {
            let prefix = if index == 0 {
                format!("{} = ", field)
            } else {
                format!(", {} = ", field)
            };
            field_strings.push(self.string_label(&prefix));
        }
        let close = self.string_label("}");

        let mut frame = Frame::new(self.fresh_label("ret"));
        frame.alloc("self", ty.clone());
        self.begin_body();
        self.emit("    mov QWORD PTR [rbp - 8], rdi");
        self.emitf(format_args!("    lea rdi, [rip + {}]", header));
        self.emit("    call __a_print_string");
        for (index, (_, field_ty)) in fields.iter().enumerate() {
            self.emitf(format_args!(
                "    lea rdi, [rip + {}]",
                field_strings[index]
            ));
            self.emit("    call __a_print_string");
            self.emit("    mov rax, QWORD PTR [rbp - 8]");
            let offset = 16 + index * 16 + 8;
            if matches!(field_ty, Type::Double) {
                self.emitf(format_args!("    movsd xmm0, QWORD PTR [rax + {}]", offset));
            } else {
                self.emitf(format_args!("    mov rax, QWORD PTR [rax + {}]", offset));
            }
            self.emit_print_value(field_ty);
        }
        self.emitf(format_args!("    lea rdi, [rip + {}]", close));
        self.emit("    call __a_print_string");
        self.emit("    leave");
        self.emit("    ret");
        let body_text = self.take_body();
        self.emitf(format_args!(".type {}, @function", symbol));
        self.emitf(format_args!("{}:", symbol));
        self.emit("    push rbp");
        self.emit("    mov rbp, rsp");
        self.emit("    sub rsp, 32");
        self.out.push_str(&body_text);
    }

    fn owned_of(&self, stmts: &[Stmt]) -> Vec<(String, Type)> {
        // The checker already computed ownership; the instance body carries
        // the annotations, so recompute the conservative set here.
        let mut candidates = Vec::new();
        collect_owned_local(stmts, &mut candidates);
        let mut escaped = std::collections::HashSet::new();
        scan_escapes_local(stmts, &mut escaped);
        candidates
            .into_iter()
            .filter(|(n, _)| !escaped.contains(n))
            .collect()
    }

    fn gen_block(&mut self, block: &ScheduledBlock) {
        let mut params: Vec<(String, Type)> = Vec::new();
        for attach in &block.attaches {
            params.push((attach.binding.clone(), attach.ty.clone()));
        }
        let owned = block.owned.clone();
        let symbol = block.func_name.clone();
        let attaches = block.attaches.clone();
        let body = block.decl.body.clone();
        let module = block.decl.module.clone();
        let prev_module = std::mem::replace(&mut self.module, module);
        self.emit_function(&symbol, &params, false, owned, |gen, frame| {
            for attach in &attaches {
                let inst = gen.checked.instance(&attach.instance_key);
                let inst_symbol = inst.symbol.clone();
                let arg_types: Vec<Type> = attach
                    .args
                    .iter()
                    .map(|a| a.ty.clone().unwrap_or(Type::Unknown))
                    .collect();
                gen.emit_args(frame, &attach.args);
                let mut int_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"].iter();
                let mut sse = 0usize;
                let plan: Vec<String> = arg_types
                    .iter()
                    .map(|t| {
                        if matches!(t, Type::Double) {
                            let r = format!("xmm{}", sse);
                            sse += 1;
                            r
                        } else {
                            int_regs.next().unwrap_or(&"r9").to_string()
                        }
                    })
                    .collect();
                for reg in plan.iter().rev() {
                    if reg.starts_with("xmm") {
                        gen.pop_xmm(reg);
                    } else {
                        gen.pop_to(reg);
                    }
                }
                gen.emitf(format_args!("    call {}", inst_symbol));
                let ty = attach.ty.clone();
                gen.store_var(frame, &attach.binding, &ty);
            }
            gen.emit_stmts(frame, &body);
            gen.emit("    xor eax, eax");
            let label = frame.return_label.clone();
            gen.emitf(format_args!("    jmp {}", label));
        });
        self.module = prev_module;
    }

    fn gen_main(&mut self) {
        let max_group = self
            .checked
            .blocks_sorted
            .iter()
            .map(|_| 1usize)
            .max()
            .unwrap_or(1);
        let _ = max_group;
        // Group blocks into execution segments. Blocks that declare a parallel
        // group (`(N:N)`, `(N:NxK)`) run concurrently; the others run in
        // declaration order.
        let mut groups: Vec<(u32, Vec<&ScheduledBlock>)> = Vec::new();
        let mut group_parallel: Vec<bool> = Vec::new();
        for block in &self.checked.blocks_sorted {
            let q = block.decl.queue.queue;
            let parallel = block.decl.queue.group_size.is_some();
            if let (Some((last_q, members)), Some(last_par)) =
                (groups.last_mut(), group_parallel.last())
            {
                if *last_q == q && *last_par == parallel {
                    members.push(block);
                    continue;
                }
            }
            groups.push((q, vec![block]));
            group_parallel.push(parallel);
        }
        let slots = groups.iter().map(|(_, m)| m.len()).max().unwrap_or(1);
        // Each parallel member needs two frame slots: the pthread handle and its
        // output buffer. Sequential segments use the first area only.
        let frame_size = ((slots * 16 + 32) / 16) * 16 + 16;

        // One trampoline per parallel-group member: it receives the member's
        // output buffer from pthread_create and parks it in r15 for the body.
        for (_queue, members) in groups.iter() {
            if members.len() < 2 {
                continue;
            }
            for member in members.iter() {
                self.emit(".text");
                self.emitf(format_args!(
                    ".type {}, @function",
                    worker_name(&member.func_name)
                ));
                self.emitf(format_args!("{}:", worker_name(&member.func_name)));
                self.emit("    push rbp");
                self.emit("    mov rbp, rsp");
                self.emit("    mov r15, rdi");
                self.emit("    xor edi, edi");
                self.emitf(format_args!("    call {}", member.func_name));
                self.emit("    xor r15, r15");
                self.emit("    leave");
                self.emit("    ret");
            }
        }

        self.emit(".text");
        self.emit(".globl main");
        self.emit(".type main, @function");
        self.emit("main:");
        self.emit("    push rbp");
        self.emit("    mov rbp, rsp");
        self.emitf(format_args!("    sub rsp, {}", frame_size));
        // `r15` is the "current output buffer" register: zero here means the
        // main thread (and therefore every sequential block) writes straight
        // through to stdout, while a parallel-group worker points it at its own
        // buffer.
        self.emit("    xor r15, r15");
        self.emit("    call __a_init");

        // Module initialisers, then the root's top-level statements.
        let module_tops = self.checked.module_top_level.clone();
        for (module, stmts) in &module_tops {
            let prev = std::mem::replace(&mut self.module, module.clone());
            let mut frame = Frame::new(self.fresh_label("mlret"));
            let stmts = stmts.clone();
            self.emit_stmts(&mut frame, &stmts);
            self.module = prev;
        }
        {
            let mut frame = Frame::new(self.fresh_label("tlret"));
            frame.owned = self.checked.top_level_owned.clone();
            let stmts = self.checked.top_level.clone();
            self.emit_stmts(&mut frame, &stmts);
            for (name, ty) in frame.owned.clone() {
                if let Some(offset) = frame.offset_of(&name) {
                    self.emitf(format_args!("    mov rdi, QWORD PTR [rbp - {}]", offset));
                    self.emit("    test rdi, rdi");
                    self.emit("    je 1f");
                    let helper = if matches!(ty, Type::Array(_)) {
                        "__a_array_free"
                    } else {
                        "__a_release"
                    };
                    self.emitf(format_args!("    call {}", helper));
                    self.emit("1:");
                }
            }
        }

        for (index, (_queue, members)) in groups.iter().enumerate() {
            if members.len() == 1 || !group_parallel[index] {
                for member in members.iter() {
                    self.emitf(format_args!("    call {}", member.func_name));
                }
            } else {
                // Deterministic parallel output: every member writes into its
                // own buffer, all members run at the same time, and once the
                // whole group has been joined the buffers are flushed in source
                // declaration order. `members` is already in declaration order
                // (the scheduler sorts by queue with a stable sort), so walking
                // it here is the guarantee. Nothing about the concurrency
                // changes: the threads are still created up front and joined
                // only after every member has been started.
                for slot in 0..members.len() {
                    self.emit("    call __a_buf_new");
                    self.emitf(format_args!(
                        "    mov QWORD PTR [rbp - {}], rax",
                        buffer_offset(slot)
                    ));
                }
                for (slot, member) in members.iter().enumerate() {
                    self.emitf(format_args!(
                        "    lea rdi, [rbp - {}]",
                        thread_offset(slot, members.len())
                    ));
                    self.emit("    xor rsi, rsi");
                    self.emitf(format_args!(
                        "    lea rdx, [rip + {}]",
                        worker_name(&member.func_name)
                    ));
                    self.emitf(format_args!(
                        "    mov rcx, QWORD PTR [rbp - {}]",
                        buffer_offset(slot)
                    ));
                    self.emit("    call pthread_create");
                }
                for slot in 0..members.len() {
                    self.emitf(format_args!(
                        "    mov rdi, QWORD PTR [rbp - {}]",
                        thread_offset(slot, members.len())
                    ));
                    self.emit("    xor rsi, rsi");
                    self.emit("    call pthread_join");
                }
                for slot in 0..members.len() {
                    self.emitf(format_args!(
                        "    mov rdi, QWORD PTR [rbp - {}]",
                        buffer_offset(slot)
                    ));
                    self.emit("    call __a_buf_flush");
                }
                let _ = index;
            }
        }

        self.emit("    call __a_runtime_shutdown");
        self.emit("    xor eax, eax");
        self.emit("    leave");
        self.emit("    ret");
    }
}

/// Frame slot (1-based) holding a parallel member's output buffer.
fn buffer_offset(slot: usize) -> usize {
    (slot + 1) * 8
}

/// Frame slot (1-based) holding a parallel member's pthread handle. Kept after
/// the buffer slots so the two areas never overlap.
fn thread_offset(slot: usize, members: usize) -> usize {
    (members + slot + 1) * 8
}

/// Symbol of the trampoline that runs a block inside a parallel group.
fn worker_name(block: &str) -> String {
    format!("{}_worker", block)
}

// Ownership helpers shared with the checker's conservative analysis.
fn collect_owned_local(stmts: &[Stmt], out: &mut Vec<(String, Type)>) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Set { name, expr, .. } => {
                let ty = expr.ty.clone().unwrap_or(Type::Unknown);
                if matches!(ty, Type::Array(_) | Type::Struct { .. } | Type::Result(_)) {
                    out.push((name.clone(), ty));
                }
            }
            StmtKind::When {
                branches,
                else_body,
            } => {
                for (_, body) in branches {
                    collect_owned_local(body, out);
                }
                if let Some(body) = else_body {
                    collect_owned_local(body, out);
                }
            }
            StmtKind::Loop { body, .. } => collect_owned_local(body, out),
            StmtKind::While { body, cleanup, .. } => {
                collect_owned_local(body, out);
                collect_owned_local(cleanup, out);
            }
            StmtKind::Each { body, .. } => collect_owned_local(body, out),
            _ => {}
        }
    }
}

fn scan_escapes_local(stmts: &[Stmt], escaped: &mut std::collections::HashSet<String>) {
    fn scan(expr: &Expr, escaped: &mut std::collections::HashSet<String>, escapes: bool) {
        match &expr.kind {
            ExprKind::Var(name) => {
                if escapes {
                    escaped.insert(name.clone());
                }
            }
            ExprKind::ArrayLit(items) => {
                for item in items {
                    scan(item, escaped, true);
                }
            }
            ExprKind::Index(a, b) => {
                scan(a, escaped, false);
                scan(b, escaped, false);
            }
            ExprKind::Field(a, _) => scan(a, escaped, false),
            ExprKind::MethodCall(a, _, args) => {
                scan(a, escaped, true);
                for arg in args {
                    scan(arg, escaped, true);
                }
            }
            ExprKind::Call(_, args) => {
                for arg in args {
                    scan(arg, escaped, true);
                }
            }
            ExprKind::StructLit(_, fields) => {
                for (_, e) in fields {
                    scan(e, escaped, true);
                }
            }
            ExprKind::Intrinsic(_, args) => {
                for arg in args {
                    scan(arg, escaped, true);
                }
            }
            ExprKind::Binary(_, a, b) => {
                scan(a, escaped, escapes);
                scan(b, escaped, escapes);
            }
            ExprKind::UnaryMinus(a) => scan(a, escaped, escapes),
            ExprKind::AddressOf(a) => scan(a, escaped, true),
            ExprKind::Deref(a) => scan(a, escaped, escapes),
            ExprKind::Try(a) => scan(a, escaped, escapes),
            _ => {}
        }
    }
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Set { expr, .. } | StmtKind::Assign { expr, .. } => {
                scan(expr, escaped, false)
            }
            StmtKind::AssignTo { target, expr } => {
                scan(target, escaped, false);
                scan(expr, escaped, false);
            }
            StmtKind::Insert { expr, .. } => scan(expr, escaped, true),
            StmtKind::Remove { index, .. } => scan(index, escaped, false),
            StmtKind::Aout { args } => {
                for arg in args {
                    if let AoutArg::Evaluated(e) = arg {
                        scan(e, escaped, false);
                    }
                }
            }
            StmtKind::When {
                branches,
                else_body,
            } => {
                for (cond, body) in branches {
                    scan(cond, escaped, false);
                    scan_escapes_local(body, escaped);
                }
                if let Some(body) = else_body {
                    scan_escapes_local(body, escaped);
                }
            }
            StmtKind::Loop { body, count } => {
                scan_escapes_local(body, escaped);
                if let Some(c) = count {
                    scan(c, escaped, false);
                }
            }
            StmtKind::While {
                cond,
                body,
                cleanup,
            } => {
                scan(cond, escaped, false);
                scan_escapes_local(body, escaped);
                scan_escapes_local(cleanup, escaped);
            }
            StmtKind::Each { iterable, body, .. } => {
                scan(iterable, escaped, true);
                scan_escapes_local(body, escaped);
            }
            StmtKind::Return { expr } => {
                if let Some(e) = expr {
                    scan(e, escaped, true);
                }
            }
            StmtKind::Fail { expr } => scan(expr, escaped, true),
            StmtKind::ExprStmt { expr } => scan(expr, escaped, false),
        }
    }
}

/// Compile a checked program into x86-64 assembly.
pub fn generate(checked: &CheckedProgram) -> String {
    let mut gen = Gen {
        out: String::new(),
        body: None,
        strings: Vec::new(),
        doubles: Vec::new(),
        label_counter: 0,
        checked,
        struct_prints: Vec::new(),
        module: String::new(),
        fallible_stack: Vec::new(),
    };
    gen.fallible_stack.push(false);

    gen.emit("# -A Core Edition: generated x86-64 assembly (Intel syntax)");
    gen.emit("# Do not edit by hand. Regenerate with: minus-a compile <input.ma>");
    gen.emit(".intel_syntax noprefix");
    gen.gen_main();

    let blocks = checked.blocks_sorted.clone();
    for block in &blocks {
        gen.gen_block(block);
    }
    gen.gen_instances();

    gen.out.push_str(crate::runtime::HELPERS);
    gen.out.push_str("\n.section .rodata\n");
    let strings = gen.strings.clone();
    for (label, value) in &strings {
        gen.out.push_str(&format!(
            "{}: .asciz \"{}\"\n",
            label,
            escape_asm_string(value)
        ));
    }
    let doubles = gen.doubles.clone();
    for (label, value) in &doubles {
        gen.out
            .push_str(&format!("{}: .double {}\n", label, format_double(*value)));
    }
    gen.out.push_str(crate::runtime::RODATA);

    // Enum values are static objects: [value][name]. They hold a pointer to the
    // variant name, so they live in .data rather than .rodata (a relocation in
    // rodata would force text relocations in the linked program).
    if !checked.enums.is_empty() {
        gen.out.push_str(".section .data\n");
    }
    let mut enum_names: Vec<&String> = checked.enums.keys().collect();
    enum_names.sort();
    for name in enum_names {
        let info = &checked.enums[name];
        for (variant, value) in &info.variants {
            let label = format!(
                "__a_enum_{}_{}",
                type_symbol(&Type::Enum(name.clone())),
                variant
            );
            gen.out.push_str(&format!(".balign 8\n{}:\n", label));
            gen.out.push_str(&format!("    .quad {}\n", value));
            let name_label = gen.string_label(&format!("{}.{}", name, variant));
            gen.out.push_str(&format!("    .quad {}\n", name_label));
        }
    }
    // Enum name strings may have been added after the rodata block.
    let strings2 = gen.strings.clone();
    if strings2.len() > strings.len() {
        gen.out.push_str(".section .rodata\n");
        for (label, value) in strings2.iter().skip(strings.len()) {
            gen.out.push_str(&format!(
                "{}: .asciz \"{}\"\n",
                label,
                escape_asm_string(value)
            ));
        }
    }

    gen.out.push_str(".section .bss\n");
    gen.out.push_str(".balign 8\n");
    let mut global_names: Vec<&String> = checked.globals.keys().collect();
    global_names.sort();
    for name in global_names {
        gen.out
            .push_str(&format!("{}: .skip 8\n", Gen::global_label(name)));
    }
    gen.out.push_str(".balign 8\n");
    gen.out.push_str("__a_random_state: .skip 8\n");
    gen.out.push_str("__a_alloc_head: .skip 8\n");
    gen.out.push_str("__a_alloc_count: .skip 8\n");
    gen.out.push_str(".balign 16\n");
    gen.out.push_str("__a_print_lock: .skip 40\n");
    gen.out.push_str("__a_int_buf: .skip 32\n");
    gen.out
        .push_str(".section .note.GNU-stack,\"\",@progbits\n");
    gen.out
}

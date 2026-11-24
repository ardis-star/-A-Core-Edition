//! Semantic analysis: name resolution, type checking, module resolution,
//! queue validation, reuse instantiation and execution scheduling.
//!
//! The output is a [`CheckedProgram`] holding everything code generation needs:
//! fully annotated expression trees, resolved struct/enum/trait tables, the
//! monomorphized reuse instances, and the execution schedule.

use crate::ast::*;
use crate::diagnostics::{
    queue_error, semantic_error, type_error, CompileResult, Diagnostic, Span,
};
use crate::types::Type;
use std::collections::{BTreeMap, HashMap, HashSet};

// ---------------------------------------------------------------------------
// Public data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct VarInfo {
    pub ty: Type,
    pub mutable: bool,
    pub is_global: bool,
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    /// Canonical name, e.g. `math.Circle`.
    pub name: String,
    pub generics: Vec<String>,
    pub fields: Vec<(String, TypeExpr)>,
    pub module: String,
    /// `pub struct`: visible outside its module.
    pub is_pub: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub name: String,
    pub variants: Vec<(String, i64)>,
    pub module: String,
    /// `pub enum`: visible outside its module.
    pub is_pub: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TraitInfo {
    pub name: String,
    pub methods: Vec<MethodSig>,
    pub module: String,
    /// `pub trait`: visible outside its module.
    pub is_pub: bool,
    pub span: Span,
}

/// One checked, monomorphized reuse (or trait-impl method).
#[derive(Debug, Clone)]
pub struct ReuseInstance {
    /// Unique key: canonical name plus concrete type arguments.
    pub key: String,
    /// Mangled assembly symbol.
    pub symbol: String,
    pub display_name: String,
    pub params: Vec<Type>,
    /// Return type of the *value* (not wrapped in `result`).
    pub ret: Type,
    pub fallible: bool,
    /// Type-annotated body.
    pub decl: ReuseDecl,
    pub module: String,
}

/// A reuse attached to a scheduled block.
#[derive(Debug, Clone)]
pub struct AttachInfo {
    pub binding: String,
    pub instance_key: String,
    /// Argument expressions, already type-annotated.
    pub args: Vec<Expr>,
    /// Type of the bound variable: `result<T>` when the reuse can fail.
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct ScheduledBlock {
    pub func_name: String,
    pub decl: BlockDecl,
    pub attaches: Vec<AttachInfo>,
    /// Locals owned by this block that are automatically released on exit.
    pub owned: Vec<(String, Type)>,
}

/// The result of semantic analysis.
#[derive(Debug, Clone)]
pub struct CheckedProgram {
    /// Root program (top-level statements and block declarations).
    pub program: Program,
    pub modules: Vec<Program>,
    pub structs: HashMap<String, StructInfo>,
    /// Resolved field lists per struct instantiation.
    pub struct_fields: HashMap<String, Vec<(String, Type)>>,
    pub enums: HashMap<String, EnumInfo>,
    pub traits: HashMap<String, TraitInfo>,
    /// `(trait, type)` -> method name -> instance key.
    pub impls: HashMap<(String, String), HashMap<String, String>>,
    pub instances: Vec<ReuseInstance>,
    pub instance_index: HashMap<String, usize>,
    pub blocks_sorted: Vec<ScheduledBlock>,
    pub globals: HashMap<String, VarInfo>,
    /// Globals initialised from imported modules before the root's own code.
    pub module_top_level: Vec<(String, Vec<Stmt>)>,
    /// Locals owned by the top-level unit.
    pub top_level_owned: Vec<(String, Type)>,
    /// Module paths that were resolved, for diagnostics and docs.
    pub module_paths: Vec<String>,
    /// Checked top-level statements of the root program.
    pub top_level: Vec<Stmt>,
}

impl CheckedProgram {
    pub fn instance(&self, key: &str) -> &ReuseInstance {
        let idx = *self
            .instance_index
            .get(key)
            .unwrap_or_else(|| panic!("unknown reuse instance '{}'", key));
        &self.instances[idx]
    }
}

// ---------------------------------------------------------------------------
// Checker state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FuncKind {
    TopLevel,
    Block,
    Reuse,
}

type Env = HashMap<String, VarInfo>;

struct FuncCtx {
    kind: FuncKind,
    /// Return type of the value (inner type when `fallible`).
    ret: Type,
    fallible: bool,
    /// Declared parameter types for the reuse being checked.
    params: Vec<Type>,
    scopes: Vec<Env>,
    module: String,
    self_ty: Option<Type>,
    /// Names of heap-owning locals declared in this function body. Ownership is
    /// computed from the checked body (`owned_locals`) so code generation can
    /// release them on the way out.
    #[allow(dead_code)]
    owned: Vec<(String, Type)>,
}

impl FuncCtx {
    fn new(kind: FuncKind, module: &str) -> FuncCtx {
        FuncCtx {
            kind,
            ret: Type::Void,
            fallible: false,
            params: Vec::new(),
            scopes: vec![HashMap::new()],
            module: module.to_string(),
            self_ty: None,
            owned: Vec::new(),
        }
    }

    fn declare(&mut self, name: &str, info: VarInfo) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), info);
        }
    }

    fn lookup(&self, name: &str) -> Option<&VarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info);
            }
        }
        None
    }

    #[allow(dead_code)]
    fn lookup_mut(&mut self, name: &str) -> Option<&mut VarInfo> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                return Some(info);
            }
        }
        None
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn in_function(&self) -> bool {
        matches!(self.kind, FuncKind::Reuse)
    }
}

pub struct Checker {
    structs: HashMap<String, StructInfo>,
    struct_fields: HashMap<String, Vec<(String, Type)>>,
    enums: HashMap<String, EnumInfo>,
    traits: HashMap<String, TraitInfo>,
    /// Canonical reuse name -> declaration.
    reuses: HashMap<String, ReuseDecl>,
    /// `(trait, type)` -> method name -> declaration.
    impls: HashMap<(String, String), HashMap<String, ReuseDecl>>,
    /// `(trait, type)` -> method name -> instance key (filled while checking).
    impl_instances: HashMap<(String, String), HashMap<String, String>>,
    instances: HashMap<String, ReuseInstance>,
    instance_order: Vec<String>,
    globals: HashMap<String, VarInfo>,
    module_paths: Vec<String>,
    current_module: String,
    /// Innermost function context; `None` while walking declarations.
    #[allow(dead_code)]
    func: FuncCtx,
}

impl Default for Checker {
    fn default() -> Self {
        Self::new()
    }
}

impl Checker {
    pub fn new() -> Checker {
        Checker {
            structs: HashMap::new(),
            struct_fields: HashMap::new(),
            enums: HashMap::new(),
            traits: HashMap::new(),
            reuses: HashMap::new(),
            impls: HashMap::new(),
            impl_instances: HashMap::new(),
            instances: HashMap::new(),
            instance_order: Vec::new(),
            globals: HashMap::new(),
            module_paths: Vec::new(),
            current_module: String::new(),
            func: FuncCtx::new(FuncKind::TopLevel, ""),
        }
    }

    // -----------------------------------------------------------------
    // Registration
    // -----------------------------------------------------------------

    fn canonical(&self, module: &str, name: &str) -> String {
        if module.is_empty() {
            name.to_string()
        } else {
            format!("{}.{}", module, name)
        }
    }

    fn register_program(&mut self, program: &mut Program) -> CompileResult<()> {
        let module = program.module_name();
        self.current_module = module.clone();
        if !module.is_empty() {
            self.module_paths.push(module.clone());
        }
        for decl in program.decls.iter_mut() {
            match decl {
                Decl::Struct(s) => {
                    s.module = module.clone();
                    let canonical = self.canonical(&module, &s.name);
                    if self.structs.contains_key(&canonical) {
                        return Err(semantic_error(
                            format!("duplicate struct '{}'", canonical),
                            s.span,
                        ));
                    }
                    self.structs.insert(
                        canonical.clone(),
                        StructInfo {
                            name: canonical,
                            generics: s.generics.clone(),
                            fields: s.fields.clone(),
                            module: module.clone(),
                            is_pub: s.is_pub,
                            span: s.span,
                        },
                    );
                }
                Decl::Enum(e) => {
                    e.module = module.clone();
                    let canonical = self.canonical(&module, &e.name);
                    if self.enums.contains_key(&canonical) {
                        return Err(semantic_error(
                            format!("duplicate enum '{}'", canonical),
                            e.span,
                        ));
                    }
                    let mut seen = HashSet::new();
                    let mut variants = Vec::new();
                    let mut next: i64 = 0;
                    for variant in &e.variants {
                        if !seen.insert(variant.name.clone()) {
                            return Err(semantic_error(
                                format!(
                                    "duplicate variant '{}' in enum '{}'",
                                    variant.name, e.name
                                ),
                                variant.span,
                            ));
                        }
                        let value = variant.value.unwrap_or(next);
                        next = value + 1;
                        variants.push((variant.name.clone(), value));
                    }
                    self.enums.insert(
                        canonical.clone(),
                        EnumInfo {
                            name: canonical,
                            variants,
                            module: module.clone(),
                            is_pub: e.is_pub,
                            span: e.span,
                        },
                    );
                }
                Decl::Trait(t) => {
                    t.module = module.clone();
                    let canonical = self.canonical(&module, &t.name);
                    if self.traits.contains_key(&canonical) {
                        return Err(semantic_error(
                            format!("duplicate trait '{}'", canonical),
                            t.span,
                        ));
                    }
                    self.traits.insert(
                        canonical.clone(),
                        TraitInfo {
                            name: canonical,
                            methods: t.methods.clone(),
                            module: module.clone(),
                            is_pub: t.is_pub,
                            span: t.span,
                        },
                    );
                }
                Decl::Reuse(r) => {
                    r.module = module.clone();
                    let canonical = self.canonical(&module, &r.name);
                    if self.reuses.contains_key(&canonical) {
                        return Err(semantic_error(
                            format!(
                                "duplicate reuse '{}': reusable logic has one definition per module",
                                canonical
                            ),
                            r.span,
                        ));
                    }
                    self.reuses.insert(canonical, r.clone());
                }
                Decl::Block(b) => {
                    b.module = module.clone();
                    b.imported = !module.is_empty();
                }
                Decl::Impl(_) => {}
            }
        }
        // Impls are registered after traits so the trait must exist.
        for decl in program.decls.iter() {
            if let Decl::Impl(imp) = decl {
                self.register_impl(imp, &module)?;
            }
        }
        Ok(())
    }

    fn register_impl(&mut self, imp: &ImplDecl, module: &str) -> CompileResult<()> {
        let trait_name = if imp.inherent {
            // Inherent methods hang off the type itself.
            String::new()
        } else {
            let name = self
                .resolve_type_name(&imp.trait_name, module)
                .ok_or_else(|| {
                    semantic_error(format!("unknown trait '{}'", imp.trait_name), imp.span)
                })?;
            let info = self.traits.get(&name).cloned().ok_or_else(|| {
                semantic_error(format!("'{}' is not a trait", imp.trait_name), imp.span)
            })?;
            self.require_visible("trait", &name, &info.module, info.is_pub, module, imp.span)?;
            name
        };
        let type_name = match &imp.type_name {
            TypeExpr::Named(n, _) => self
                .resolve_type_name(n, module)
                .ok_or_else(|| semantic_error(format!("unknown type '{}'", n), imp.span))?,
            other => {
                return Err(semantic_error(
                    format!("cannot implement a trait for '{}'", other.name()),
                    imp.span,
                ))
            }
        };
        if !self.structs.contains_key(&type_name) && !self.enums.contains_key(&type_name) {
            return Err(semantic_error(
                format!("cannot implement a trait for unknown type '{}'", type_name),
                imp.span,
            ));
        }
        // An implementation must match the trait's declared signature
        // (parameter count, parameter types and return type).
        if !trait_name.is_empty() {
            for method in &imp.methods {
                let m = method;
                if let Some(sig) = self
                    .traits
                    .get(&trait_name)
                    .and_then(|t| t.methods.iter().find(|s| s.name == m.name))
                    .cloned()
                {
                    if sig.params.len() != m.params.len() {
                        return Err(semantic_error(
                            format!(
                            "method '{}' of '{}' takes {} parameter(s) but the impl declares {}",
                            m.name,
                            trait_name,
                            sig.params.len(),
                            m.params.len()
                        ),
                            m.span,
                        ));
                    }
                    let expected_ret = match &sig.ret {
                        Some(texpr) => self.resolve_type(texpr, &HashMap::new(), module)?,
                        None => Type::Void,
                    };
                    let actual_ret = match &m.ret {
                        Some(texpr) => self.resolve_type(texpr, &HashMap::new(), module)?,
                        None => Type::Void,
                    };
                    if !expected_ret.compatible_with(&actual_ret)
                        && !actual_ret.compatible_with(&expected_ret)
                    {
                        return Err(type_error(
                            format!(
                                "method '{}' of '{}' returns {} but the impl returns {}",
                                m.name,
                                trait_name,
                                expected_ret.name(),
                                actual_ret.name()
                            ),
                            m.span,
                        ));
                    }
                    for (i, (s, a)) in sig.params.iter().zip(m.params.iter()).enumerate() {
                        let expected = match &s.ty {
                            Some(texpr) => self.resolve_type(texpr, &HashMap::new(), module)?,
                            None => continue,
                        };
                        let actual = match &a.ty {
                            Some(texpr) => self.resolve_type(texpr, &HashMap::new(), module)?,
                            None => Type::Unknown,
                        };
                        // `self` binds to the implementing type, so only the
                        // remaining parameters are compared literally.
                        if i == 0 && s.name == "self" {
                            continue;
                        }
                        if !expected.compatible_with(&actual) {
                            return Err(type_error(
                                format!(
                                "parameter '{}' of method '{}' is {} in '{}' but {} in the impl",
                                s.name,
                                m.name,
                                expected.name(),
                                trait_name,
                                actual.name()
                            ),
                                a.span,
                            ));
                        }
                    }
                }
            }
        }
        let key = (trait_name.clone(), type_name.clone());
        let entry = self.impls.entry(key.clone()).or_default();
        for method in &imp.methods {
            let mut m = method.clone();
            m.module = module.to_string();
            if !trait_name.is_empty() {
                m.trait_name = Some(trait_name.clone());
            }
            m.self_ty = Some(imp.type_name.clone());
            if entry.contains_key(&m.name) {
                return Err(semantic_error(
                    format!(
                        "duplicate method '{}' in impl of '{}' for '{}'",
                        m.name, trait_name, type_name
                    ),
                    m.span,
                ));
            }
            entry.insert(m.name.clone(), m);
        }
        // Every trait method must be implemented.
        let required: Vec<String> = self
            .traits
            .get(&trait_name)
            .map(|t| t.methods.iter().map(|m| m.name.clone()).collect())
            .unwrap_or_default();
        for name in required {
            if !entry.contains_key(&name) {
                return Err(semantic_error(
                    format!(
                        "impl of '{}' for '{}' is missing method '{}'",
                        trait_name, type_name, name
                    ),
                    imp.span,
                ));
            }
        }
        Ok(())
    }

    /// Reject a reference to a declaration that is private to another module.
    fn require_visible(
        &self,
        kind: &str,
        canonical: &str,
        module_of_decl: &str,
        is_pub: bool,
        caller: &str,
        span: Span,
    ) -> CompileResult<()> {
        if is_pub || module_of_decl == caller {
            return Ok(());
        }
        Err(semantic_error(
            format!(
                "{} '{}' is private to module '{}'; mark it 'pub' to use it elsewhere",
                kind, canonical, module_of_decl
            ),
            span,
        ))
    }

    /// Resolve a possibly module-qualified name to its canonical form.
    fn resolve_type_name(&self, name: &str, module: &str) -> Option<String> {
        if name.contains('.') {
            let full = name.to_string();
            if self.structs.contains_key(&full)
                || self.enums.contains_key(&full)
                || self.traits.contains_key(&full)
                || self.reuses.contains_key(&full)
            {
                return Some(full);
            }
            return None;
        }
        let local = self.canonical(module, name);
        if self.structs.contains_key(&local)
            || self.enums.contains_key(&local)
            || self.traits.contains_key(&local)
            || self.reuses.contains_key(&local)
        {
            return Some(local);
        }
        if self.structs.contains_key(name)
            || self.enums.contains_key(name)
            || self.traits.contains_key(name)
            || self.reuses.contains_key(name)
        {
            return Some(name.to_string());
        }
        None
    }

    // -----------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------

    fn resolve_type(
        &mut self,
        ty: &TypeExpr,
        generics: &HashMap<String, Type>,
        module: &str,
    ) -> CompileResult<Type> {
        match ty {
            TypeExpr::Ptr(inner) => Ok(Type::Ptr(Box::new(
                self.resolve_type(inner, generics, module)?,
            ))),
            TypeExpr::Array(inner) => Ok(Type::Array(Box::new(
                self.resolve_type(inner, generics, module)?,
            ))),
            TypeExpr::Named(name, args) => {
                if let Some(bound) = generics.get(name) {
                    if !args.is_empty() {
                        return Err(type_error(
                            format!("type parameter '{}' does not take arguments", name),
                            Span::dummy(),
                        ));
                    }
                    return Ok(bound.clone());
                }
                match name.as_str() {
                    "int" => return Ok(Type::Int),
                    "double" => return Ok(Type::Double),
                    "string" => return Ok(Type::String),
                    "bool" => return Ok(Type::Bool),
                    "void" => return Ok(Type::Void),
                    "mixed" => return Ok(Type::Mixed),
                    _ => {}
                }
                let mut resolved_args = Vec::new();
                for arg in args {
                    resolved_args.push(self.resolve_type(arg, generics, module)?);
                }
                if let Some(canonical) = self.resolve_type_name(name, module) {
                    if self.structs.contains_key(&canonical) {
                        let info = self.structs.get(&canonical).unwrap().clone();
                        if info.generics.len() != resolved_args.len() {
                            return Err(type_error(
                                format!(
                                    "struct '{}' expects {} type argument(s), found {}",
                                    info.name,
                                    info.generics.len(),
                                    resolved_args.len()
                                ),
                                Span::dummy(),
                            ));
                        }
                        let inst = Type::Struct {
                            name: canonical.clone(),
                            args: resolved_args,
                        };
                        self.ensure_struct_instance(&canonical, &inst)?;
                        return Ok(inst);
                    }
                    if self.enums.contains_key(&canonical) {
                        return Ok(Type::Enum(canonical));
                    }
                    return Err(type_error(
                        format!("'{}' is not a value type", name),
                        Span::dummy(),
                    ));
                }
                Err(semantic_error(
                    format!("unknown type '{}'", name),
                    Span::dummy(),
                ))
            }
        }
    }

    /// Resolve and cache the field list of a struct instantiation.
    fn ensure_struct_instance(&mut self, canonical: &str, ty: &Type) -> CompileResult<()> {
        let key = type_key(ty);
        if self.struct_fields.contains_key(&key) {
            return Ok(());
        }
        let info = self.structs.get(canonical).cloned().ok_or_else(|| {
            semantic_error(format!("unknown struct '{}'", canonical), Span::dummy())
        })?;
        let args = match ty {
            Type::Struct { args, .. } => args.clone(),
            _ => Vec::new(),
        };
        let mut generics = HashMap::new();
        for (name, arg) in info.generics.iter().zip(args.iter()) {
            generics.insert(name.clone(), arg.clone());
        }
        let mut fields = Vec::new();
        for (fname, fty) in &info.fields {
            let resolved = self.resolve_type(fty, &generics, &info.module)?;
            fields.push((fname.clone(), resolved));
        }
        self.struct_fields.insert(key, fields);
        Ok(())
    }

    fn struct_fields_of(&self, ty: &Type) -> Option<&Vec<(String, Type)>> {
        self.struct_fields.get(&type_key(ty))
    }

    // -----------------------------------------------------------------
    // Entry point
    // -----------------------------------------------------------------

    pub fn check(&mut self, mut programs: Vec<Program>) -> CompileResult<CheckedProgram> {
        if programs.is_empty() {
            return Err(semantic_error("no input program", Span::dummy()));
        }
        let mut root = programs.remove(0);
        let mut imported = programs;

        for program in imported.iter_mut() {
            self.register_program(program)?;
        }
        self.register_program(&mut root)?;
        self.validate_queues(&root)?;
        if !root.decls.iter().any(|d| matches!(d, Decl::Block(_))) {
            return Err(semantic_error(
                "-A program has no blocks: at least one `block name(queue) { ... }` is required",
                Span::dummy(),
            )
            .with_hint("add a block such as `block main(1) { aout(\"hello\") }`"));
        }

        // Globals from every module: module-level `set` / `setc`.
        let mut module_top_level = Vec::new();
        for program in imported.iter() {
            let module = program.module_name();
            if !program.top_level.is_empty() {
                let mut ctx = FuncCtx::new(FuncKind::TopLevel, &module);
                let stmts = self.check_stmts(program.top_level.clone(), &mut ctx)?;
                module_top_level.push((module.clone(), stmts));
            }
        }

        // Root top-level statements.
        let mut top_ctx = FuncCtx::new(FuncKind::TopLevel, &root.module_name());
        let top_level = self.check_stmts(root.top_level.clone(), &mut top_ctx)?;
        let top_level_owned = self.owned_locals(&top_level, &top_ctx);

        // Blocks.
        let mut blocks: Vec<BlockDecl> = Vec::new();
        for decl in &root.decls {
            if let Decl::Block(b) = decl {
                blocks.push(b.clone());
            }
        }

        let mut checked_blocks = Vec::new();
        for block in &blocks {
            checked_blocks.push(self.check_block(block)?);
        }

        // Schedule: ascending queue order, stable within a queue.
        let mut sorted = checked_blocks;
        sorted.sort_by_key(|b| b.decl.queue.queue);
        for (i, block) in sorted.iter_mut().enumerate() {
            block.func_name = format!("__a_block_{}", i);
        }

        let mut instance_index = HashMap::new();
        let mut instances = Vec::new();
        for key in &self.instance_order {
            if let Some(inst) = self.instances.get(key) {
                instance_index.insert(key.clone(), instances.len());
                instances.push(inst.clone());
            }
        }

        Ok(CheckedProgram {
            program: root,
            modules: imported,
            structs: self.structs.clone(),
            struct_fields: self.struct_fields.clone(),
            enums: self.enums.clone(),
            traits: self.traits.clone(),
            impls: self.impl_instances.clone(),
            instances,
            instance_index,
            blocks_sorted: sorted,
            globals: self.globals.clone(),
            module_top_level,
            top_level_owned,
            module_paths: self.module_paths.clone(),
            top_level,
        })
    }

    // -----------------------------------------------------------------
    // Queue validation
    // -----------------------------------------------------------------

    fn validate_queues(&self, program: &Program) -> CompileResult<()> {
        let blocks: Vec<&BlockDecl> = program
            .decls
            .iter()
            .filter_map(|d| match d {
                Decl::Block(b) => Some(b),
                _ => None,
            })
            .collect();

        let mut by_queue: BTreeMap<u32, Vec<&BlockDecl>> = BTreeMap::new();
        for block in &blocks {
            by_queue.entry(block.queue.queue).or_default().push(block);
        }

        for (queue, members) in &by_queue {
            let declared: Vec<&BlockDecl> = members
                .iter()
                .copied()
                .filter(|m| m.queue.group_size.is_some())
                .collect();

            // A queue without an explicit group may hold only one block: the
            // queue is the execution slot. Parallel work is declared with
            // `(N:N)` / `(N:NxK)`.
            if declared.is_empty() {
                if members.len() > 1 {
                    return Err(queue_error(
                        format!(
                            "duplicate assignment to queue {}: {} blocks declare queue {} but only one block may run alone in a queue (use {}:{} or {}:{}xK for a parallel group)",
                            queue,
                            members.len(),
                            queue,
                            queue,
                            queue,
                            queue,
                            queue
                        ),
                        members[1].queue.span,
                    ));
                }
                continue;
            }

            let size = declared[0].queue.group_size.unwrap();
            for member in members.iter() {
                if member.queue.group_size != declared[0].queue.group_size {
                    let what = match member.queue.group_size {
                        Some(k) => format!("a group of {}", k),
                        None => "no parallel group".to_string(),
                    };
                    return Err(queue_error(
                        format!(
                            "inconsistent parallel group in queue {}: block '{}' declares a group of {} but block '{}' declares {}",
                            queue, member.name, size, declared[0].name, what
                        ),
                        member.queue.span,
                    ));
                }
            }
            let actual = declared.len() as u32;
            if actual < size {
                return Err(queue_error(
                    format!(
                        "missing parallel group members in queue {}: the group declares {} blocks but only {} block(s) were found",
                        queue, size, actual
                    ),
                    declared[0].queue.span,
                ));
            }
            if actual > size {
                return Err(queue_error(
                    format!(
                        "excess parallel group members in queue {}: the group declares {} blocks but {} blocks were found",
                        queue, size, actual
                    ),
                    declared[0].queue.span,
                ));
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------
    // Blocks
    // -----------------------------------------------------------------

    fn check_block(&mut self, block: &BlockDecl) -> CompileResult<ScheduledBlock> {
        let mut ctx = FuncCtx::new(FuncKind::Block, &block.module);
        let mut attaches = Vec::new();
        for attach in &block.attaches {
            // Attaching something that is not a reuse is a distinct mistake
            // from calling an unknown function, so it gets its own message.
            let known = self
                .resolve_type_name(&attach.name, &block.module)
                .map(|c| self.reuses.contains_key(&c))
                .unwrap_or(false);
            if !known {
                return Err(semantic_error(
                    format!(
                        "undefined reuse '{}' attached to block '{}'",
                        attach.name, block.name
                    ),
                    attach.span,
                ));
            }
            let mut args = Vec::new();
            for arg in &attach.args {
                args.push(self.check_expr(arg.clone(), &mut ctx)?);
            }
            let arg_types: Vec<Type> = args.iter().map(|a| expr_type(a)).collect();
            let instance_key = self.resolve_call(
                &attach.name,
                &arg_types,
                &mut args,
                &block.module,
                None,
                attach.span,
            )?;
            let inst = self.instances.get(&instance_key).ok_or_else(|| {
                semantic_error(
                    format!("reuse '{}' could not be instantiated", attach.name),
                    attach.span,
                )
            })?;
            let ty = if inst.fallible {
                Type::Result(Box::new(inst.ret.clone()))
            } else {
                inst.ret.clone()
            };
            if matches!(ty, Type::Void) {
                return Err(semantic_error(
                    format!(
                        "reuse '{}' returns nothing, so it cannot be attached to a block",
                        attach.name
                    ),
                    attach.span,
                ));
            }
            let binding = attach.binding.clone();
            if binding != "self" {
                ctx.declare(
                    &binding,
                    VarInfo {
                        ty: ty.clone(),
                        mutable: false,
                        is_global: false,
                    },
                );
            }
            attaches.push(AttachInfo {
                binding,
                instance_key,
                args,
                ty,
            });
        }
        let body = self.check_stmts(block.body.clone(), &mut ctx)?;
        let owned = self.owned_locals(&body, &ctx);
        let mut decl = block.clone();
        decl.body = body;
        Ok(ScheduledBlock {
            func_name: String::new(),
            decl,
            attaches,
            owned,
        })
    }

    // -----------------------------------------------------------------
    // Reuse instantiation (monomorphization)
    // -----------------------------------------------------------------

    /// Resolve a call to a reuse and return its instance key, instantiating a
    /// monomorphized copy when needed.
    fn resolve_call(
        &mut self,
        name: &str,
        arg_types: &[Type],
        args: &mut [Expr],
        module: &str,
        self_ty: Option<&Type>,
        span: Span,
    ) -> CompileResult<String> {
        // Trait methods resolved through an impl.
        if let Some(self_ty) = self_ty {
            let type_name = named_type_name(self_ty).ok_or_else(|| {
                type_error(
                    format!("cannot call method '{}' on '{}'", name, self_ty.name()),
                    span,
                )
            })?;
            let mut found = None;
            for ((trait_name, ty), methods) in &self.impls {
                if ty == &type_name && methods.contains_key(name) {
                    found = Some((trait_name.clone(), ty.clone(), methods[name].clone()));
                    break;
                }
            }
            let (trait_name, ty_name, decl) = found.ok_or_else(|| {
                semantic_error(
                    format!(
                        "no method '{}' found for type '{}' (no impl provides it)",
                        name, type_name
                    ),
                    span,
                )
            })?;
            let mut all_args = Vec::new();
            all_args.push(Expr::typed(
                ExprKind::Var("self".to_string()),
                span,
                self_ty.clone(),
            ));
            for arg in args.iter_mut() {
                all_args.push(arg.clone());
            }
            let mut all_types = vec![self_ty.clone()];
            all_types.extend(arg_types.iter().cloned());
            let impl_key = format!("impl:{}:{}:{}", trait_name, ty_name, name);
            let key = self.instantiate(&decl, &impl_key, &all_types, span)?;
            self.impl_instances
                .entry((trait_name.clone(), ty_name.clone()))
                .or_default()
                .insert(name.to_string(), key.clone());
            // Trait method signature validation.
            if let Some(trait_info) = self.traits.get(&trait_name) {
                if let Some(sig) = trait_info.methods.iter().find(|m| m.name == name) {
                    let expected = sig.params.len();
                    if expected != all_args.len() {
                        return Err(semantic_error(
                            format!(
                                "method '{}' expects {} parameter(s), found {}",
                                name,
                                expected,
                                all_args.len()
                            ),
                            span,
                        ));
                    }
                }
            }
            return Ok(key);
        }

        let resolved = self.resolve_type_name(name, module);
        if let Some(canonical) = &resolved {
            if let Some(decl) = self.reuses.get(canonical) {
                if !decl.is_pub && decl.module != *module {
                    return Err(semantic_error(
                        format!(
                            "reuse '{}' is private to module '{}'; mark it 'pub' to use it elsewhere",
                            canonical, decl.module
                        ),
                        span,
                    ));
                }
            }
        }
        let canonical = resolved
            .filter(|c| self.reuses.contains_key(c))
            .ok_or_else(|| semantic_error(format!("unknown function '{}'", name), span))?;
        let decl = self.reuses.get(&canonical).cloned().unwrap();
        self.instantiate(&decl, &canonical, arg_types, span)
    }

    fn instantiate(
        &mut self,
        decl: &ReuseDecl,
        canonical: &str,
        arg_types: &[Type],
        span: Span,
    ) -> CompileResult<String> {
        let inst_name = format!("{}|{}", canonical, decl.name);
        // Bind parameter types: declared annotations win, otherwise use the
        // argument's type. `self` binds to the impl type.
        let mut param_types: Vec<Type> = Vec::new();
        for (i, param) in decl.params.iter().enumerate() {
            let declared = match &param.ty {
                Some(texpr) => Some(
                    self.resolve_type(texpr, &HashMap::new(), &decl.module)
                        .or_else(|_| Ok::<Type, Diagnostic>(Type::Unknown))?,
                ),
                None => None,
            };
            let from_arg = arg_types.get(i).cloned().unwrap_or(Type::Unknown);
            let chosen = match declared {
                Some(Type::Unknown) => from_arg,
                Some(t) if matches!(t, Type::Param(_)) => from_arg,
                Some(t) => t,
                None => from_arg,
            };
            param_types.push(chosen);
        }
        if arg_types.len() != decl.params.len() {
            return Err(semantic_error(
                format!(
                    "{} expects {} argument(s) but {} were given",
                    decl.name,
                    decl.params.len(),
                    arg_types.len()
                ),
                span,
            ));
        }
        // Type arguments: unify generic parameters from declared signatures.
        let mut generics: HashMap<String, Type> = HashMap::new();
        for (param, arg) in decl.params.iter().zip(arg_types.iter()) {
            if let Some(texpr) = &param.ty {
                self.unify_type(texpr, arg, &mut generics, &decl.module);
            }
        }
        // A generic that never appears in a parameter is inferred from the body
        // context only; leave it unbound and report if it is used.
        let key = format!("{}<{}>", inst_name, type_key_list(&param_types));
        if self.instances.contains_key(&key) {
            return Ok(key);
        }

        // Substitute generics into parameter types and return type.
        let mut substituted_params = Vec::new();
        for param in &decl.params {
            match &param.ty {
                Some(texpr) => {
                    let mut t = self.resolve_type(texpr, &generics, &decl.module)?;
                    if matches!(t, Type::Unknown) {
                        t = Type::Unknown;
                    }
                    substituted_params.push(t);
                }
                None => substituted_params.push(Type::Unknown),
            }
        }
        for (i, t) in param_types.iter().enumerate() {
            if let Some(slot) = substituted_params.get_mut(i) {
                if matches!(slot, Type::Unknown) {
                    *slot = t.clone();
                }
            }
        }
        let declared_ret = match &decl.ret {
            Some(texpr) => Some(self.resolve_type(texpr, &generics, &decl.module)?),
            None => None,
        };

        // Detect fallibility syntactically before checking.
        let fallible = stmts_fallible(&decl.body);

        let mut body_decl = decl.clone();
        body_decl.params = decl
            .params
            .iter()
            .zip(substituted_params.iter())
            .map(|(p, t)| Param {
                name: p.name.clone(),
                ty: Some(type_to_expr(t)),
                span: p.span,
            })
            .collect();
        body_decl.ret = declared_ret.as_ref().map(type_to_expr);

        // Publish a provisional instance before checking the body: a recursive
        // call then resolves against the signature instead of re-entering
        // instantiation forever. The checked body replaces it below.
        let symbol = format!("__a_reuse_{}", sanitize_symbol(canonical, &param_types));
        if !self.instances.contains_key(&key) {
            self.instance_order.push(key.clone());
            self.instances.insert(
                key.clone(),
                ReuseInstance {
                    key: key.clone(),
                    symbol: symbol.clone(),
                    display_name: canonical.to_string(),
                    params: substituted_params.clone(),
                    ret: declared_ret.clone().unwrap_or(Type::Unknown),
                    fallible,
                    decl: body_decl.clone(),
                    module: decl.module.clone(),
                },
            );
        }

        // Check the body with concrete types.
        let mut ctx = FuncCtx::new(FuncKind::Reuse, &decl.module);
        ctx.ret = declared_ret.clone().unwrap_or(Type::Unknown);
        ctx.fallible = fallible;
        ctx.params = substituted_params.clone();
        if let Some(self_expr) = &decl.self_ty {
            let self_ty = self.resolve_type(self_expr, &generics, &decl.module)?;
            ctx.self_ty = Some(self_ty);
        }
        for (param, ty) in decl.params.iter().zip(substituted_params.iter()) {
            ctx.declare(
                &param.name,
                VarInfo {
                    ty: ty.clone(),
                    mutable: false,
                    is_global: false,
                },
            );
        }
        let body = self.check_stmts(body_decl.body.clone(), &mut ctx)?;
        let ret = if matches!(ctx.ret, Type::Unknown) {
            Type::Void
        } else {
            ctx.ret.clone()
        };
        body_decl.body = body;

        let inst = ReuseInstance {
            key: key.clone(),
            symbol,
            display_name: canonical.to_string(),
            params: substituted_params,
            ret,
            fallible,
            decl: body_decl,
            module: decl.module.clone(),
        };
        self.instances.insert(key.clone(), inst);
        Ok(key)
    }

    fn unify_type(
        &self,
        texpr: &TypeExpr,
        actual: &Type,
        generics: &mut HashMap<String, Type>,
        module: &str,
    ) {
        match texpr {
            TypeExpr::Named(name, args) => {
                if let Some(info) = self.structs.get(&self.canonical(module, name)) {
                    if info.generics.iter().any(|g| g == name) || info.generics.len() == args.len()
                    {
                        if let Type::Struct {
                            args: actual_args, ..
                        } = actual
                        {
                            for (sub, actual_arg) in args.iter().zip(actual_args.iter()) {
                                self.unify_type(sub, actual_arg, generics, module);
                            }
                        }
                    }
                    return;
                }
                if args.is_empty() && !is_builtin_type_name(name) {
                    // A bare type parameter?
                    if !self.structs.contains_key(&self.canonical(module, name))
                        && !self.enums.contains_key(&self.canonical(module, name))
                    {
                        generics
                            .entry(name.clone())
                            .or_insert_with(|| actual.clone());
                    }
                }
                for (sub, actual_arg) in args
                    .iter()
                    .zip(match actual {
                        Type::Struct { args, .. } => args.as_slice(),
                        _ => &[],
                    })
                    .map(|(a, b)| (a, b))
                {
                    self.unify_type(sub, actual_arg, generics, module);
                }
            }
            TypeExpr::Ptr(inner) => {
                if let Type::Ptr(actual_inner) = actual {
                    self.unify_type(inner, actual_inner, generics, module);
                }
            }
            TypeExpr::Array(inner) => {
                if let Type::Array(actual_inner) = actual {
                    self.unify_type(inner, actual_inner, generics, module);
                }
            }
        }
    }

    // -----------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------

    fn check_stmts(&mut self, stmts: Vec<Stmt>, ctx: &mut FuncCtx) -> CompileResult<Vec<Stmt>> {
        let mut out = Vec::new();
        for stmt in stmts {
            match self.check_stmt(stmt, ctx)? {
                Some(s) => out.push(s),
                None => {}
            }
        }
        Ok(out)
    }

    fn check_stmt(&mut self, stmt: Stmt, ctx: &mut FuncCtx) -> CompileResult<Option<Stmt>> {
        let span = stmt.span;
        let kind = match stmt.kind {
            StmtKind::Set {
                name,
                mutable,
                expr,
            } => {
                let expr = self.check_expr(expr, ctx)?;
                let ty = expr_type(&expr);
                if ctx.lookup(&name).is_some() {
                    return Err(semantic_error(
                        format!("variable '{}' is already declared in this scope", name),
                        span,
                    ));
                }
                let info = VarInfo {
                    ty: ty.clone(),
                    mutable,
                    is_global: matches!(ctx.kind, FuncKind::TopLevel),
                };
                // A top-level `set`/`setc` is program-wide: it lives in the
                // global store so every block can read it after the startup
                // statements have run.
                if info.is_global {
                    self.globals.insert(name.clone(), info.clone());
                }
                ctx.declare(&name, info);
                StmtKind::Set {
                    name,
                    mutable,
                    expr,
                }
            }
            StmtKind::Assign { name, expr } => {
                let expr = self.check_expr(expr, ctx)?;
                let ty = expr_type(&expr);
                let info = ctx
                    .lookup(&name)
                    .cloned()
                    .or_else(|| self.globals.get(&name).cloned());
                let info = info.ok_or_else(|| {
                    semantic_error(format!("undefined variable '{}'", name), span)
                })?;
                if !info.mutable {
                    return Err(semantic_error(
                        format!(
                            "cannot assign to '{}': it was declared with 'set' (fixed); use 'setc' for a changeable variable",
                            name
                        ),
                        span,
                    )
                    .with_hint(format!(
                        "change the declaration to `setc {} = ...` to allow assignments",
                        name
                    )));
                }
                if !info.ty.compatible_with(&ty) {
                    return Err(type_error(
                        format!(
                            "cannot assign {} to '{}' of type {} (types are fixed at declaration)",
                            ty.name(),
                            name,
                            info.ty.name()
                        ),
                        span,
                    ));
                }
                StmtKind::Assign { name, expr }
            }
            StmtKind::AssignTo { target, expr } => {
                let target = self.check_expr(target, ctx)?;
                let expr = self.check_expr(expr, ctx)?;
                let target_ty = expr_type(&target);
                let value_ty = expr_type(&expr);
                if !target_ty.compatible_with(&value_ty) {
                    return Err(type_error(
                        format!("cannot assign {} to {}", value_ty.name(), target_ty.name()),
                        span,
                    ));
                }
                if !self.is_mutable_lvalue(&target, ctx) {
                    return Err(semantic_error(
                        "cannot assign through this expression: it is not a changeable location",
                        span,
                    ));
                }
                StmtKind::AssignTo { target, expr }
            }
            StmtKind::Insert { name, op, expr } => {
                let expr = self.check_expr(expr, ctx)?;
                let info = ctx.lookup(&name).cloned().ok_or_else(|| {
                    semantic_error(format!("undefined variable '{}'", name), span)
                })?;
                if !info.mutable {
                    return Err(semantic_error(
                        format!(
                            "cannot modify '{}': it was declared with 'set'; use 'setc'",
                            name
                        ),
                        span,
                    ));
                }
                let value_ty = expr_type(&expr);
                match &info.ty {
                    Type::Array(elem) => {
                        if op != InsertOp::Add {
                            return Err(semantic_error(
                                "'insert' on an array appends a value; only 'insert name = value' is valid",
                                span,
                            ));
                        }
                        if !elem.compatible_with(&value_ty) && !matches!(**elem, Type::Mixed) {
                            return Err(type_error(
                                format!(
                                    "cannot insert {} into array of {}",
                                    value_ty.name(),
                                    elem.name()
                                ),
                                span,
                            ));
                        }
                    }
                    t if t.is_numeric() => {
                        if !value_ty.is_numeric() && !value_ty.is_unknown() {
                            return Err(type_error(
                                format!(
                                    "'insert' arithmetic needs a numeric value, found {}",
                                    value_ty.name()
                                ),
                                span,
                            ));
                        }
                    }
                    other => {
                        return Err(type_error(
                            format!("cannot 'insert' into '{}' of type {}", name, other.name()),
                            span,
                        ))
                    }
                }
                StmtKind::Insert { name, op, expr }
            }
            StmtKind::Remove { name, index } => {
                let index = self.check_expr(index, ctx)?;
                let info = ctx.lookup(&name).cloned().ok_or_else(|| {
                    semantic_error(format!("undefined variable '{}'", name), span)
                })?;
                if !info.mutable {
                    return Err(semantic_error(
                        format!(
                            "cannot modify '{}': it was declared with 'set'; use 'setc'",
                            name
                        ),
                        span,
                    ));
                }
                if !matches!(info.ty, Type::Array(_)) {
                    return Err(type_error(
                        format!("'remove' needs an array, '{}' is {}", name, info.ty.name()),
                        span,
                    ));
                }
                StmtKind::Remove { name, index }
            }
            StmtKind::Aout { args } => {
                let mut checked = Vec::new();
                for arg in args {
                    match arg {
                        AoutArg::Literal(s, sp) => checked.push(AoutArg::Literal(s, sp)),
                        AoutArg::Evaluated(e) => {
                            checked.push(AoutArg::Evaluated(self.check_expr(e, ctx)?))
                        }
                    }
                }
                StmtKind::Aout { args: checked }
            }
            StmtKind::When {
                branches,
                else_body,
            } => {
                let mut checked = Vec::new();
                for (cond, body) in branches {
                    let cond = self.check_condition(cond, ctx)?;
                    ctx.push_scope();
                    let body = self.check_stmts(body, ctx)?;
                    ctx.pop_scope();
                    checked.push((cond, body));
                }
                let else_body = match else_body {
                    Some(body) => {
                        ctx.push_scope();
                        let b = self.check_stmts(body, ctx)?;
                        ctx.pop_scope();
                        Some(b)
                    }
                    None => None,
                };
                StmtKind::When {
                    branches: checked,
                    else_body,
                }
            }
            StmtKind::Loop { body, count } => {
                let count = match count {
                    Some(c) => {
                        let c = self.check_expr(c, ctx)?;
                        let ty = expr_type(&c);
                        if !matches!(ty, Type::Int | Type::Unknown) {
                            return Err(type_error(
                                format!(
                                    "a counted loop needs an integer count, found {}",
                                    ty.name()
                                ),
                                span,
                            ));
                        }
                        Some(c)
                    }
                    None => None,
                };
                ctx.push_scope();
                let body = self.check_stmts(body, ctx)?;
                ctx.pop_scope();
                StmtKind::Loop { body, count }
            }
            StmtKind::While {
                cond,
                body,
                cleanup,
            } => {
                let cond = self.check_condition(cond, ctx)?;
                ctx.push_scope();
                let body = self.check_stmts(body, ctx)?;
                let cleanup = self.check_stmts(cleanup, ctx)?;
                ctx.pop_scope();
                StmtKind::While {
                    cond,
                    body,
                    cleanup,
                }
            }
            StmtKind::Each {
                var,
                iterable,
                body,
            } => {
                let iterable = self.check_expr(iterable, ctx)?;
                let elem = match expr_type(&iterable) {
                    Type::Array(elem) => *elem,
                    Type::Unknown => Type::Unknown,
                    other => {
                        return Err(type_error(
                            format!("'each' needs an array, found {}", other.name()),
                            span,
                        ))
                    }
                };
                ctx.push_scope();
                ctx.declare(
                    &var,
                    VarInfo {
                        ty: elem,
                        mutable: false,
                        is_global: false,
                    },
                );
                let body = self.check_stmts(body, ctx)?;
                ctx.pop_scope();
                StmtKind::Each {
                    var,
                    iterable,
                    body,
                }
            }
            StmtKind::Return { expr } => {
                if !ctx.in_function() {
                    return Err(semantic_error(
                        "'return' is only valid inside a reuse",
                        span,
                    ));
                }
                let expr = match expr {
                    Some(e) => Some(self.check_expr(e, ctx)?),
                    None => None,
                };
                match &expr {
                    Some(e) => {
                        let ty = expr_type(e);
                        if matches!(ctx.ret, Type::Unknown) {
                            ctx.ret = ty;
                        } else if !ctx.ret.compatible_with(&ty)
                            && !ty.compatible_with(&ctx.ret)
                            && !matches!(ctx.ret, Type::Void)
                        {
                            return Err(type_error(
                                format!(
                                    "this reuse returns {} but 'return' produced {}",
                                    ctx.ret.name(),
                                    ty.name()
                                ),
                                span,
                            ));
                        }
                    }
                    None => {
                        if matches!(ctx.ret, Type::Unknown) {
                            ctx.ret = Type::Void;
                        }
                    }
                }
                StmtKind::Return { expr }
            }
            StmtKind::Fail { expr } => {
                let expr = self.check_expr(expr, ctx)?;
                let ty = expr_type(&expr);
                if !matches!(ty, Type::String | Type::Int | Type::Unknown | Type::Enum(_)) {
                    return Err(type_error(
                        format!(
                            "a failure value must be a string, integer or enum, found {}",
                            ty.name()
                        ),
                        span,
                    ));
                }
                ctx.fallible = true;
                StmtKind::Fail { expr }
            }
            StmtKind::ExprStmt { expr } => {
                let expr = self.check_expr(expr, ctx)?;
                StmtKind::ExprStmt { expr }
            }
        };
        Ok(Some(Stmt::new(kind, span)))
    }

    fn check_condition(&mut self, cond: Expr, ctx: &mut FuncCtx) -> CompileResult<Expr> {
        let checked = self.check_expr(cond, ctx)?;
        let ty = expr_type(&checked);
        if !matches!(ty, Type::Bool | Type::Unknown) {
            return Err(type_error(
                format!("a condition must be bool (on/off), found {}", ty.name()),
                checked.span,
            ));
        }
        Ok(checked)
    }

    fn is_mutable_lvalue(&self, expr: &Expr, ctx: &FuncCtx) -> bool {
        match &expr.kind {
            ExprKind::Var(name) => ctx
                .lookup(name)
                .map(|i| i.mutable)
                .or_else(|| self.globals.get(name).map(|i| i.mutable))
                .unwrap_or(false),
            ExprKind::Field(base, _) => self.is_mutable_lvalue(base, ctx),
            ExprKind::Index(base, _) => match &base.kind {
                ExprKind::Var(name) => ctx
                    .lookup(name)
                    .map(|i| i.mutable)
                    .or_else(|| self.globals.get(name).map(|i| i.mutable))
                    .unwrap_or(true),
                _ => true,
            },
            // Writing through a pointer is always allowed: the pointer's
            // pointee mutability is a runtime concern in Core.
            ExprKind::Deref(_) => true,
            _ => false,
        }
    }

    // -----------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------

    fn check_expr(&mut self, expr: Expr, ctx: &mut FuncCtx) -> CompileResult<Expr> {
        let span = expr.span;
        let kind = expr.kind;
        match kind {
            ExprKind::Int(_) => Ok(Expr::typed(kind, span, Type::Int)),
            ExprKind::Double(_) => Ok(Expr::typed(kind, span, Type::Double)),
            ExprKind::Str(_) => Ok(Expr::typed(kind, span, Type::String)),
            ExprKind::Bool(_) => Ok(Expr::typed(kind, span, Type::Bool)),
            ExprKind::Var(name) => {
                if let Some(info) = ctx.lookup(&name).cloned() {
                    return Ok(Expr::typed(ExprKind::Var(name), span, info.ty));
                }
                if let Some(info) = self.globals.get(&name).cloned() {
                    return Ok(Expr::typed(ExprKind::Var(name), span, info.ty));
                }
                if name == "make" {
                    return Err(semantic_error(
                        "'make' introduces the -A Foundation namespace, which Core Edition does not ship",
                        span,
                    ));
                }
                Err(semantic_error(
                    format!("undefined variable '{}'", name),
                    span,
                ))
            }
            ExprKind::ArrayLit(elems) => {
                let mut checked = Vec::new();
                let mut elem_ty: Option<Type> = None;
                let mut mixed = false;
                for elem in elems {
                    let e = self.check_expr(elem, ctx)?;
                    let t = expr_type(&e);
                    match &elem_ty {
                        None => elem_ty = Some(t),
                        Some(prev) => {
                            if !prev.compatible_with(&t) && !t.compatible_with(prev) {
                                mixed = true;
                            }
                        }
                    }
                    checked.push(e);
                }
                let ty = if mixed || checked.is_empty() {
                    Type::Array(Box::new(Type::Mixed))
                } else {
                    Type::Array(Box::new(elem_ty.unwrap()))
                };
                Ok(Expr::typed(ExprKind::ArrayLit(checked), span, ty))
            }
            ExprKind::Index(base, index) => {
                let base = self.check_expr(*base, ctx)?;
                let index = self.check_expr(*index, ctx)?;
                let index_ty = expr_type(&index);
                if !matches!(index_ty, Type::Int | Type::Unknown) {
                    return Err(type_error(
                        format!("an array index must be an int, found {}", index_ty.name()),
                        index.span,
                    ));
                }
                let elem = match expr_type(&base) {
                    Type::Array(elem) => *elem,
                    Type::Unknown => Type::Unknown,
                    other => {
                        return Err(type_error(
                            format!("cannot index into {}", other.name()),
                            span,
                        ))
                    }
                };
                Ok(Expr::typed(
                    ExprKind::Index(Box::new(base), Box::new(index)),
                    span,
                    elem,
                ))
            }
            ExprKind::Field(base, name) => self.check_field(*base, name, span, ctx),
            ExprKind::MethodCall(base, name, args) => {
                // `module.reuse(...)` is written with dots, so it arrives as a
                // method call. When the receiver is a module path and the full
                // name names a reuse, resolve it as an ordinary call.
                if let Some(prefix) = field_path(&base) {
                    let full = format!("{}.{}", prefix, name);
                    if self
                        .resolve_type_name(&full, &ctx.module)
                        .map(|c| self.reuses.contains_key(&c))
                        .unwrap_or(false)
                    {
                        let mut checked_args = Vec::new();
                        for arg in args {
                            checked_args.push(self.check_expr(arg, ctx)?);
                        }
                        let arg_types: Vec<Type> = checked_args.iter().map(expr_type).collect();
                        let module = ctx.module.clone();
                        let key = self.resolve_call(
                            &full,
                            &arg_types,
                            &mut checked_args,
                            &module,
                            None,
                            span,
                        )?;
                        let inst = self.instances.get(&key).cloned().ok_or_else(|| {
                            semantic_error(
                                format!("reuse '{}' could not be instantiated", full),
                                span,
                            )
                        })?;
                        let ty = if inst.fallible {
                            Type::Result(Box::new(inst.ret.clone()))
                        } else {
                            inst.ret.clone()
                        };
                        return Ok(Expr::typed(ExprKind::Call(full, checked_args), span, ty));
                    }
                }
                let base = self.check_expr(*base, ctx)?;
                let mut checked_args = Vec::new();
                for arg in args {
                    checked_args.push(self.check_expr(arg, ctx)?);
                }
                let base_ty = expr_type(&base);
                let arg_types: Vec<Type> = checked_args.iter().map(expr_type).collect();
                let key = self.resolve_call(
                    &name,
                    &arg_types,
                    &mut checked_args,
                    &ctx.module.clone(),
                    Some(&base_ty),
                    span,
                )?;
                let inst = self.instances.get(&key).cloned().ok_or_else(|| {
                    semantic_error(format!("method '{}' could not be instantiated", name), span)
                })?;
                let ty = if inst.fallible {
                    Type::Result(Box::new(inst.ret.clone()))
                } else {
                    inst.ret.clone()
                };
                Ok(Expr::typed(
                    ExprKind::MethodCall(Box::new(base), name, checked_args),
                    span,
                    ty,
                ))
            }
            ExprKind::Call(name, args) => {
                let mut checked_args = Vec::new();
                for arg in args {
                    checked_args.push(self.check_expr(arg, ctx)?);
                }
                let arg_types: Vec<Type> = checked_args.iter().map(expr_type).collect();
                if name == "random" {
                    if checked_args.len() != 2 {
                        return Err(semantic_error(
                            "random() takes two arguments: random(min, max)",
                            span,
                        ));
                    }
                    return Ok(Expr::typed(
                        ExprKind::Call(name, checked_args),
                        span,
                        Type::Int,
                    ));
                }
                let module = ctx.module.clone();
                let key =
                    self.resolve_call(&name, &arg_types, &mut checked_args, &module, None, span)?;
                let inst = self.instances.get(&key).cloned().ok_or_else(|| {
                    semantic_error(
                        format!("function '{}' could not be instantiated", name),
                        span,
                    )
                })?;
                let ty = if inst.fallible {
                    Type::Result(Box::new(inst.ret.clone()))
                } else {
                    inst.ret.clone()
                };
                Ok(Expr::typed(ExprKind::Call(name, checked_args), span, ty))
            }
            ExprKind::StructLit(name, fields) => self.check_struct_literal(name, fields, span, ctx),
            ExprKind::Intrinsic(name, args) => self.check_intrinsic(name, args, span, ctx),
            ExprKind::Binary(op, left, right) => {
                let left = self.check_expr(*left, ctx)?;
                let right = self.check_expr(*right, ctx)?;
                let lt = expr_type(&left);
                let rt = expr_type(&right);
                let result = match op {
                    BinOp::And => {
                        if !matches!(lt, Type::Bool | Type::Unknown)
                            || !matches!(rt, Type::Bool | Type::Unknown)
                        {
                            return Err(type_error("'and' requires bool operands", span));
                        }
                        Type::Bool
                    }
                    BinOp::Eq | BinOp::Ne => {
                        // Aggregates have no value equality: comparing them
                        // would silently compare pointers.
                        for side in [&lt, &rt] {
                            if matches!(
                                side,
                                Type::Struct { .. } | Type::Array(_) | Type::Result(_)
                            ) {
                                return Err(type_error(
                                    format!(
                                        "cannot compare {} values with '{}': only numbers, strings, booleans, enums and pointers support equality",
                                        side.name(),
                                        op.symbol()
                                    ),
                                    span,
                                ));
                            }
                        }
                        if !lt.compatible_with(&rt) && !rt.compatible_with(&lt) {
                            return Err(type_error(
                                format!("cannot compare {} with {}", lt.name(), rt.name()),
                                span,
                            ));
                        }
                        Type::Bool
                    }
                    BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                        let ok = (lt.is_numeric() && rt.is_numeric())
                            || (matches!(lt, Type::String) && matches!(rt, Type::String))
                            || matches!(lt, Type::Unknown)
                            || matches!(rt, Type::Unknown);
                        if !ok {
                            return Err(type_error(
                                format!(
                                    "cannot compare {} with {} using '{}'",
                                    lt.name(),
                                    rt.name(),
                                    op.symbol()
                                ),
                                span,
                            ));
                        }
                        Type::Bool
                    }
                    BinOp::Mod => {
                        if !(matches!(lt, Type::Int | Type::Unknown)
                            && matches!(rt, Type::Int | Type::Unknown))
                        {
                            return Err(type_error("'%' requires integer operands", span));
                        }
                        Type::Int
                    }
                    _ => {
                        if !(lt.is_numeric() || matches!(lt, Type::Unknown))
                            || !(rt.is_numeric() || matches!(rt, Type::Unknown))
                        {
                            return Err(type_error(
                                format!(
                                    "arithmetic '{}' requires numeric operands, found {} and {}",
                                    op.symbol(),
                                    lt.name(),
                                    rt.name()
                                ),
                                span,
                            ));
                        }
                        Type::promoted(&lt, &rt)
                    }
                };
                Ok(Expr::typed(
                    ExprKind::Binary(op, Box::new(left), Box::new(right)),
                    span,
                    result,
                ))
            }
            ExprKind::UnaryMinus(inner) => {
                let inner = self.check_expr(*inner, ctx)?;
                let ty = expr_type(&inner);
                if !ty.is_numeric() && !ty.is_unknown() {
                    return Err(type_error(format!("cannot negate {}", ty.name()), span));
                }
                Ok(Expr::typed(ExprKind::UnaryMinus(Box::new(inner)), span, ty))
            }
            ExprKind::AddressOf(inner) => {
                let inner = self.check_expr(*inner, ctx)?;
                let ty = expr_type(&inner);
                Ok(Expr::typed(
                    ExprKind::AddressOf(Box::new(inner)),
                    span,
                    Type::Ptr(Box::new(ty)),
                ))
            }
            ExprKind::Deref(inner) => {
                let inner = self.check_expr(*inner, ctx)?;
                match expr_type(&inner) {
                    Type::Ptr(target) => {
                        Ok(Expr::typed(ExprKind::Deref(Box::new(inner)), span, *target))
                    }
                    other => Err(type_error(
                        format!("cannot dereference {}: not a pointer", other.name()),
                        span,
                    )),
                }
            }
            ExprKind::Try(inner) => {
                let inner = self.check_expr(*inner, ctx)?;
                let inner_ty = expr_type(&inner);
                match inner_ty {
                    Type::Result(ok) => {
                        if !ctx.in_function() {
                            return Err(semantic_error(
                                "'?' propagates a failure to the enclosing reuse; handle the result explicitly here",
                                span,
                            ));
                        }
                        ctx.fallible = true;
                        Ok(Expr::typed(ExprKind::Try(Box::new(inner)), span, *ok))
                    }
                    other => Err(type_error(
                        format!("'?' needs a fallible call (result), found {}", other.name()),
                        span,
                    )),
                }
            }
        }
    }

    fn check_field(
        &mut self,
        base: Expr,
        name: String,
        span: Span,
        ctx: &mut FuncCtx,
    ) -> CompileResult<Expr> {
        // `make.shape.*` belongs to -A Foundation. Core Edition ships no
        // Foundation packages, so say so instead of reporting a stray variable.
        if let Some(path) = field_path(&base) {
            if path == "make" || path.starts_with("make.") || path.ends_with(".make") {
                return Err(semantic_error(
                    format!(
                        "'{}.{}' belongs to the -A Foundation namespace, which Core Edition does not ship",
                        path, name
                    ),
                    span,
                ));
            }
        }
        // Enum variant: `Color.red`, or module-qualified `math.Color.red`.
        let module = ctx.module.clone();
        if let Some((enum_name, variant)) = self.enum_variant_of(&base, &name, &module) {
            let info = self
                .enums
                .get(&enum_name)
                .cloned()
                .ok_or_else(|| semantic_error(format!("unknown enum '{}'", enum_name), span))?;
            self.require_visible("enum", &enum_name, &info.module, info.is_pub, &module, span)?;
            // The variant must exist; the numeric value only matters to code
            // generation, which re-reads it from the enum table.
            self.enums
                .get(&enum_name)
                .and_then(|e| e.variants.iter().find(|(v, _)| v == &variant))
                .map(|(_, value)| *value)
                .ok_or_else(|| {
                    semantic_error(
                        format!("enum '{}' has no variant '{}'", enum_name, variant),
                        span,
                    )
                })?;
            return Ok(Expr::typed(
                ExprKind::Field(Box::new(base), name),
                span,
                Type::Enum(enum_name),
            ));
        }

        let base = self.check_expr(base, ctx)?;
        let base_ty = expr_type(&base);
        match &base_ty {
            Type::Struct { .. } => {
                let fields = self.struct_fields_of(&base_ty).cloned().ok_or_else(|| {
                    semantic_error(format!("unknown struct type {}", base_ty.name()), span)
                })?;
                let field_ty = fields
                    .iter()
                    .find(|(f, _)| f == &name)
                    .map(|(_, t)| t.clone())
                    .ok_or_else(|| {
                        semantic_error(
                            format!("struct {} has no field '{}'", base_ty.name(), name),
                            span,
                        )
                    })?;
                Ok(Expr::typed(
                    ExprKind::Field(Box::new(base), name),
                    span,
                    field_ty,
                ))
            }
            Type::Ptr(inner) => match &**inner {
                Type::Struct { .. } => {
                    let fields = self.struct_fields_of(inner).cloned().ok_or_else(|| {
                        semantic_error(format!("unknown struct type {}", inner.name()), span)
                    })?;
                    let field_ty = fields
                        .iter()
                        .find(|(f, _)| f == &name)
                        .map(|(_, t)| t.clone())
                        .ok_or_else(|| {
                            semantic_error(
                                format!("struct {} has no field '{}'", inner.name(), name),
                                span,
                            )
                        })?;
                    Ok(Expr::typed(
                        ExprKind::Field(Box::new(base), name),
                        span,
                        field_ty,
                    ))
                }
                other => Err(type_error(
                    format!("cannot access field '{}' on ptr<{}>", name, other.name()),
                    span,
                )),
            },
            Type::Array(_) if name == "length" => Ok(Expr::typed(
                ExprKind::Field(Box::new(base), name),
                span,
                Type::Int,
            )),
            Type::Result(ok) => match name.as_str() {
                "ok" => Ok(Expr::typed(
                    ExprKind::Field(Box::new(base), name),
                    span,
                    Type::Bool,
                )),
                "value" => Ok(Expr::typed(
                    ExprKind::Field(Box::new(base), name),
                    span,
                    (**ok).clone(),
                )),
                "error" => Ok(Expr::typed(
                    ExprKind::Field(Box::new(base), name),
                    span,
                    Type::String,
                )),
                other => Err(semantic_error(
                    format!(
                        "result has fields 'ok', 'value' and 'error', not '{}'",
                        other
                    ),
                    span,
                )),
            },
            Type::Enum(enum_name) if name == "value" => {
                let _ = enum_name;
                Ok(Expr::typed(
                    ExprKind::Field(Box::new(base), name),
                    span,
                    Type::Int,
                ))
            }
            Type::String if name == "length" => Ok(Expr::typed(
                ExprKind::Field(Box::new(base), name),
                span,
                Type::Int,
            )),
            Type::Unknown => Ok(Expr::typed(
                ExprKind::Field(Box::new(base), name),
                span,
                Type::Unknown,
            )),
            other => Err(semantic_error(
                format!("type {} has no field '{}'", other.name(), name),
                span,
            )),
        }
    }

    /// Recognize `EnumName.variant` and `module.EnumName.variant` patterns.
    fn enum_variant_of(&self, base: &Expr, field: &str, module: &str) -> Option<(String, String)> {
        let mut path: Vec<String> = Vec::new();
        let mut current = base;
        loop {
            match &current.kind {
                ExprKind::Var(name) => {
                    path.insert(0, name.clone());
                    break;
                }
                ExprKind::Field(inner, name) => {
                    path.insert(0, name.clone());
                    current = inner;
                }
                _ => return None,
            }
        }
        // `Color.red`
        let joined = path.join(".");
        if self.enums.contains_key(&joined) {
            return Some((joined, field.to_string()));
        }
        // `EnumName.variant` where EnumName is in the root or current module.
        if let Some(canonical) = self.resolve_type_name(&joined, module) {
            if self.enums.contains_key(&canonical) {
                return Some((canonical, field.to_string()));
            }
        }
        None
    }

    fn check_struct_literal(
        &mut self,
        name: String,
        fields: Vec<(String, Expr)>,
        span: Span,
        ctx: &mut FuncCtx,
    ) -> CompileResult<Expr> {
        let canonical = self
            .resolve_type_name(&name, &ctx.module.clone())
            .filter(|c| self.structs.contains_key(c))
            .ok_or_else(|| semantic_error(format!("unknown struct '{}'", name), span))?;
        let info = self.structs.get(&canonical).cloned().unwrap();
        let caller = ctx.module.clone();
        self.require_visible(
            "struct",
            &canonical,
            &info.module,
            info.is_pub,
            &caller,
            span,
        )?;

        let mut checked = Vec::new();
        let mut generics: HashMap<String, Type> = HashMap::new();
        for (fname, fexpr) in fields {
            let value = self.check_expr(fexpr, ctx)?;
            let value_ty = expr_type(&value);
            let declared = info
                .fields
                .iter()
                .find(|(f, _)| f == &fname)
                .cloned()
                .ok_or_else(|| {
                    semantic_error(
                        format!("struct '{}' has no field '{}'", canonical, fname),
                        span,
                    )
                })?;
            self.unify_type(&declared.1, &value_ty, &mut generics, &info.module);
            checked.push((fname, value));
        }
        // Every field must be initialised exactly once.
        let mut names: Vec<String> = checked.iter().map(|(n, _)| n.clone()).collect();
        names.sort();
        names.dedup();
        if names.len() != checked.len() {
            return Err(semantic_error(
                format!("a field of struct '{}' was initialised twice", canonical),
                span,
            ));
        }
        for (field, _) in &info.fields {
            if !checked.iter().any(|(n, _)| n == field) {
                return Err(semantic_error(
                    format!(
                        "missing field '{}' in literal for struct '{}'",
                        field, canonical
                    ),
                    span,
                ));
            }
        }
        // Infer generic arguments from the supplied field values.
        let mut args = Vec::new();
        for generic in &info.generics {
            match generics.get(generic) {
                Some(t) => args.push(t.clone()),
                None => {
                    return Err(type_error(
                        format!(
                            "cannot infer type argument '{}' for struct '{}'",
                            generic, canonical
                        ),
                        span,
                    ))
                }
            }
        }
        let inst_ty = Type::Struct {
            name: canonical.clone(),
            args,
        };
        self.ensure_struct_instance(&canonical, &inst_ty)?;
        // Re-check field types against the instantiated struct.
        let resolved = self.struct_fields_of(&inst_ty).cloned().unwrap_or_default();
        for (fname, value) in &checked {
            let declared = resolved
                .iter()
                .find(|(f, _)| f == fname)
                .map(|(_, t)| t.clone());
            let value_ty = expr_type(value);
            if let Some(declared) = declared {
                if !declared.compatible_with(&value_ty) {
                    return Err(type_error(
                        format!(
                            "field '{}' of '{}' expects {}, found {}",
                            fname,
                            canonical,
                            declared.name(),
                            value_ty.name()
                        ),
                        value.span,
                    ));
                }
            }
        }
        Ok(Expr::typed(
            ExprKind::StructLit(canonical, checked),
            span,
            inst_ty,
        ))
    }

    fn check_intrinsic(
        &mut self,
        name: String,
        args: Vec<Expr>,
        span: Span,
        ctx: &mut FuncCtx,
    ) -> CompileResult<Expr> {
        let mut checked = Vec::new();
        for arg in args {
            checked.push(self.check_expr(arg, ctx)?);
        }
        let types: Vec<Type> = checked.iter().map(expr_type).collect();
        let (expected, result) = intrinsic_signature(&name, &types, span)?;
        if expected != types.len() {
            return Err(semantic_error(
                format!(
                    "@{} expects {} argument(s), found {}",
                    name,
                    expected,
                    types.len()
                ),
                span,
            ));
        }
        Ok(Expr::typed(
            ExprKind::Intrinsic(name, checked),
            span,
            result,
        ))
    }

    // -----------------------------------------------------------------
    // Escape analysis (automatic release)
    // -----------------------------------------------------------------

    /// Locals of this function that own heap memory and never escape.
    fn owned_locals(&self, stmts: &[Stmt], ctx: &FuncCtx) -> Vec<(String, Type)> {
        let mut candidates: Vec<(String, Type)> = Vec::new();
        collect_owned(stmts, &mut candidates);
        if candidates.is_empty() {
            return candidates;
        }
        let mut escaped: HashSet<String> = HashSet::new();
        scan_escapes(stmts, &mut escaped);
        let _ = ctx;
        candidates
            .into_iter()
            .filter(|(name, _)| !escaped.contains(name))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

pub fn expr_type(expr: &Expr) -> Type {
    expr.ty.clone().unwrap_or(Type::Unknown)
}

fn is_builtin_type_name(name: &str) -> bool {
    matches!(
        name,
        "int" | "double" | "string" | "bool" | "void" | "mixed"
    )
}

fn named_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Struct { name, .. } => Some(name.clone()),
        Type::Enum(name) => Some(name.clone()),
        _ => None,
    }
}

pub fn type_key(ty: &Type) -> String {
    ty.name()
}

fn type_key_list(types: &[Type]) -> String {
    types.iter().map(|t| t.name()).collect::<Vec<_>>().join(",")
}

/// Dotted name of a field chain (`make.shape` for `make.shape.circle`), or
/// `None` when the base is not a plain name chain.
fn field_path(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Var(name) => Some(name.clone()),
        ExprKind::Field(base, name) => field_path(base).map(|p| format!("{}.{}", p, name)),
        _ => None,
    }
}

fn sanitize_symbol(canonical: &str, params: &[Type]) -> String {
    let mut out = String::new();
    for ch in canonical.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            // Anything else ('::', '.', brackets, generics...) would break the
            // assembly label or the linker; flatten it to a separator.
            out.push('_');
        }
    }
    if !params.is_empty() {
        out.push_str("__");
        for ty in params {
            out.push('_');
            for ch in ty.name().chars() {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    out.push(ch);
                } else {
                    out.push('_');
                }
            }
        }
    }
    out
}

#[allow(dead_code)]
fn decl_trait_suffix(decl: &ReuseDecl) -> String {
    decl.trait_name.clone().unwrap_or_default()
}

fn type_to_expr(ty: &Type) -> TypeExpr {
    match ty {
        Type::Int => TypeExpr::Named("int".into(), vec![]),
        Type::Double => TypeExpr::Named("double".into(), vec![]),
        Type::String => TypeExpr::Named("string".into(), vec![]),
        Type::Bool => TypeExpr::Named("bool".into(), vec![]),
        Type::Void => TypeExpr::Named("void".into(), vec![]),
        Type::Mixed => TypeExpr::Named("mixed".into(), vec![]),
        Type::Unknown => TypeExpr::Named("mixed".into(), vec![]),
        Type::Param(name) => TypeExpr::Named(name.clone(), vec![]),
        Type::Array(inner) => TypeExpr::Array(Box::new(type_to_expr(inner))),
        Type::Ptr(inner) => TypeExpr::Ptr(Box::new(type_to_expr(inner))),
        Type::Result(inner) => TypeExpr::Named("result".into(), vec![type_to_expr(inner)]),
        Type::Struct { name, args } => {
            TypeExpr::Named(name.clone(), args.iter().map(type_to_expr).collect())
        }
        Type::Enum(name) => TypeExpr::Named(name.clone(), vec![]),
    }
}

/// Does this statement list contain `fail` or `?` anywhere?
fn stmts_fallible(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_fallible)
}

fn stmt_fallible(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Fail { .. } => true,
        StmtKind::Set { expr, .. } | StmtKind::Assign { expr, .. } => expr_has_try(expr),
        StmtKind::AssignTo { target, expr } => expr_has_try(target) || expr_has_try(expr),
        StmtKind::Insert { expr, .. } => expr_has_try(expr),
        StmtKind::Remove { index, .. } => expr_has_try(index),
        StmtKind::Aout { args } => args.iter().any(|a| match a {
            AoutArg::Evaluated(e) => expr_has_try(e),
            _ => false,
        }),
        StmtKind::When {
            branches,
            else_body,
        } => {
            branches
                .iter()
                .any(|(c, b)| expr_has_try(c) || stmts_fallible(b))
                || else_body
                    .as_ref()
                    .map(|b| stmts_fallible(b))
                    .unwrap_or(false)
        }
        StmtKind::Loop { body, count } => {
            stmts_fallible(body) || count.as_ref().map(expr_has_try).unwrap_or(false)
        }
        StmtKind::While {
            cond,
            body,
            cleanup,
        } => expr_has_try(cond) || stmts_fallible(body) || stmts_fallible(cleanup),
        StmtKind::Each { iterable, body, .. } => expr_has_try(iterable) || stmts_fallible(body),
        StmtKind::Return { expr } => expr.as_ref().map(expr_has_try).unwrap_or(false),
        StmtKind::ExprStmt { expr } => expr_has_try(expr),
    }
}

fn expr_has_try(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Try(_) => true,
        ExprKind::ArrayLit(items) => items.iter().any(expr_has_try),
        ExprKind::Index(a, b) => expr_has_try(a) || expr_has_try(b),
        ExprKind::Field(a, _) => expr_has_try(a),
        ExprKind::MethodCall(a, _, args) => expr_has_try(a) || args.iter().any(expr_has_try),
        ExprKind::Call(_, args) => args.iter().any(expr_has_try),
        ExprKind::StructLit(_, fields) => fields.iter().any(|(_, e)| expr_has_try(e)),
        ExprKind::Intrinsic(_, args) => args.iter().any(expr_has_try),
        ExprKind::Binary(_, a, b) => expr_has_try(a) || expr_has_try(b),
        ExprKind::UnaryMinus(a) | ExprKind::AddressOf(a) | ExprKind::Deref(a) => expr_has_try(a),
        _ => false,
    }
}

fn collect_owned(stmts: &[Stmt], out: &mut Vec<(String, Type)>) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Set { name, expr, .. } => {
                let ty = expr_type(expr);
                if matches!(ty, Type::Array(_) | Type::Struct { .. } | Type::Result(_)) {
                    out.push((name.clone(), ty));
                }
            }
            StmtKind::When {
                branches,
                else_body,
            } => {
                for (_, body) in branches {
                    collect_owned(body, out);
                }
                if let Some(body) = else_body {
                    collect_owned(body, out);
                }
            }
            StmtKind::Loop { body, .. } => collect_owned(body, out),
            StmtKind::While { body, cleanup, .. } => {
                collect_owned(body, out);
                collect_owned(cleanup, out);
            }
            StmtKind::Each { body, .. } => collect_owned(body, out),
            _ => {}
        }
    }
}

/// Conservative escape scan: a variable escapes if it is returned, passed to a
/// call, stored into a container, assigned to another variable, or has its
/// address taken.
fn scan_escapes(stmts: &[Stmt], escaped: &mut HashSet<String>) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Set { expr, .. } => scan_expr(expr, escaped, false),
            StmtKind::Assign { expr, .. } => scan_expr(expr, escaped, false),
            StmtKind::AssignTo { target, expr } => {
                scan_expr(target, escaped, false);
                scan_expr(expr, escaped, false);
            }
            StmtKind::Insert { expr, .. } => scan_expr(expr, escaped, true),
            StmtKind::Remove { index, .. } => scan_expr(index, escaped, false),
            StmtKind::Aout { args } => {
                for arg in args {
                    if let AoutArg::Evaluated(e) = arg {
                        scan_expr(e, escaped, false);
                    }
                }
            }
            StmtKind::When {
                branches,
                else_body,
            } => {
                for (cond, body) in branches {
                    scan_expr(cond, escaped, false);
                    scan_escapes(body, escaped);
                }
                if let Some(body) = else_body {
                    scan_escapes(body, escaped);
                }
            }
            StmtKind::Loop { body, count } => {
                scan_escapes(body, escaped);
                if let Some(c) = count {
                    scan_expr(c, escaped, false);
                }
            }
            StmtKind::While {
                cond,
                body,
                cleanup,
            } => {
                scan_expr(cond, escaped, false);
                scan_escapes(body, escaped);
                scan_escapes(cleanup, escaped);
            }
            StmtKind::Each { iterable, body, .. } => {
                scan_expr(iterable, escaped, true);
                scan_escapes(body, escaped);
            }
            StmtKind::Return { expr } => {
                if let Some(e) = expr {
                    scan_expr(e, escaped, true);
                }
            }
            StmtKind::Fail { expr } => scan_expr(expr, escaped, true),
            StmtKind::ExprStmt { expr } => scan_expr(expr, escaped, false),
        }
    }
}

fn scan_expr(expr: &Expr, escaped: &mut HashSet<String>, value_escapes: bool) {
    match &expr.kind {
        ExprKind::Var(name) => {
            if value_escapes {
                escaped.insert(name.clone());
            }
        }
        ExprKind::ArrayLit(items) => {
            for item in items {
                scan_expr(item, escaped, true);
            }
        }
        ExprKind::Index(a, b) => {
            scan_expr(a, escaped, false);
            scan_expr(b, escaped, false);
        }
        ExprKind::Field(a, _) => scan_expr(a, escaped, false),
        ExprKind::MethodCall(a, _, args) => {
            scan_expr(a, escaped, true);
            for arg in args {
                scan_expr(arg, escaped, true);
            }
        }
        ExprKind::Call(_, args) => {
            for arg in args {
                scan_expr(arg, escaped, true);
            }
        }
        ExprKind::StructLit(_, fields) => {
            for (_, e) in fields {
                scan_expr(e, escaped, true);
            }
        }
        ExprKind::Intrinsic(_, args) => {
            for arg in args {
                scan_expr(arg, escaped, true);
            }
        }
        ExprKind::Binary(_, a, b) => {
            scan_expr(a, escaped, value_escapes);
            scan_expr(b, escaped, value_escapes);
        }
        ExprKind::UnaryMinus(a) => scan_expr(a, escaped, value_escapes),
        ExprKind::AddressOf(a) => scan_expr(a, escaped, true),
        ExprKind::Deref(a) => scan_expr(a, escaped, value_escapes),
        ExprKind::Try(a) => scan_expr(a, escaped, value_escapes),
        _ => {}
    }
}

/// Intrinsic table: name -> (arity, result type).
fn intrinsic_signature(name: &str, args: &[Type], span: Span) -> CompileResult<(usize, Type)> {
    let ptr_or_mixed =
        |t: &Type| -> bool { matches!(t, Type::Ptr(_) | Type::Mixed | Type::Unknown) };
    let int_like =
        |t: &Type| -> bool { matches!(t, Type::Int | Type::Unknown | Type::Bool | Type::Mixed) };
    let expect = |cond: bool, what: &str| -> CompileResult<()> {
        if cond {
            Ok(())
        } else {
            Err(type_error(format!("@{}: {}", name, what), span))
        }
    };
    // Positional checks: the n-th argument must be an integer / a pointer.
    let arg_int = |n: usize, what: &str| -> CompileResult<()> {
        expect(args.get(n).map(int_like).unwrap_or(true), what)
    };
    let arg_ptr = |n: usize, what: &str| -> CompileResult<()> {
        expect(args.get(n).map(ptr_or_mixed).unwrap_or(true), what)
    };
    match name {
        "alloc" => {
            arg_int(0, "expects a size in bytes (int)")?;
            Ok((1, Type::Ptr(Box::new(Type::Mixed))))
        }
        "free" => {
            arg_ptr(0, "expects a pointer")?;
            Ok((1, Type::Void))
        }
        "cell" => Ok((
            1,
            Type::Ptr(Box::new(args.first().cloned().unwrap_or(Type::Unknown))),
        )),
        "unbox" => {
            arg_ptr(0, "expects a cell")?;
            let target = match args.first() {
                Some(Type::Ptr(inner)) => (**inner).clone(),
                _ => Type::Unknown,
            };
            Ok((1, target))
        }
        "mem_copy" => {
            arg_ptr(0, "expects a destination pointer")?;
            arg_ptr(1, "expects a source pointer")?;
            arg_int(2, "expects a length in bytes (int)")?;
            Ok((3, Type::Void))
        }
        "mem_set" => {
            arg_ptr(0, "expects a destination pointer")?;
            arg_int(1, "expects a byte value (int)")?;
            arg_int(2, "expects a length in bytes (int)")?;
            Ok((3, Type::Void))
        }
        "mem_read8" | "mem_read64" => {
            arg_ptr(0, "expects a pointer to read")?;
            Ok((1, Type::Int))
        }
        "mem_write8" | "mem_write64" => {
            arg_ptr(0, "expects a pointer to write")?;
            arg_int(1, "expects the value to write (int)")?;
            Ok((2, Type::Void))
        }
        "live_allocs" => Ok((0, Type::Int)),
        "popcount" | "clz" | "ctz" | "bswap" => {
            arg_int(0, "expects an integer argument")?;
            Ok((1, Type::Int))
        }
        "rotl" | "rotr" => {
            arg_int(0, "expects an integer argument")?;
            arg_int(1, "expects a shift count (int)")?;
            Ok((2, Type::Int))
        }
        "atomic_add" | "atomic_sub" | "atomic_swap" => {
            arg_ptr(0, "expects a pointer to the shared word")?;
            arg_int(1, "expects an integer value")?;
            Ok((2, Type::Int))
        }
        "atomic_cas" => {
            arg_ptr(0, "expects a pointer to the shared word")?;
            arg_int(1, "expects the expected value (int)")?;
            arg_int(2, "expects the new value (int)")?;
            Ok((3, Type::Bool))
        }
        "fence" => Ok((0, Type::Void)),
        "arch" | "os" => Ok((0, Type::String)),
        "pointer_bits" => Ok((0, Type::Int)),
        "has_atomics" => Ok((0, Type::Bool)),
        other => Err(semantic_error(
            format!("unknown intrinsic '@{}'", other),
            span,
        )),
    }
}

/// Build the execution schedule from the block declarations of a program.
pub fn schedule_from(program: &Program) -> Vec<ScheduledBlock> {
    let mut blocks: Vec<BlockDecl> = program
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Block(b) => Some(b.clone()),
            _ => None,
        })
        .collect();
    blocks.sort_by_key(|b| b.queue.queue);
    blocks
        .into_iter()
        .enumerate()
        .map(|(i, decl)| ScheduledBlock {
            func_name: format!("__a_block_{}", i),
            decl,
            attaches: Vec::new(),
            owned: Vec::new(),
        })
        .collect()
}

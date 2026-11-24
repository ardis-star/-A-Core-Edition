//! Semantics-preserving optimizations.
//!
//! Passes, in order:
//!
//! 1. **Constant folding** of literal arithmetic, comparisons and boolean
//!    `and`. Integer operations use checked arithmetic, so overflow is never
//!    folded; division by a literal zero is left alone so the runtime
//!    division-by-zero diagnostic still fires.
//! 2. **Constant propagation** of `setc`/`set` values that are assigned a
//!    literal and never reassigned inside the same statement list.
//! 3. **Algebraic identities** (`x+0`, `x*1`, ...) applied only to pure
//!    operands, so no bounds check or call is ever removed.
//! 4. **Common subexpression elimination** inside a single expression tree for
//!    pure subexpressions: identical `a op b` trees evaluate once.
//! 5. **Dead code elimination** after `return`/`fail` in a statement list, and
//!    removal of branches whose condition folds to a constant.
//!
//! The optimizer mutates the checked program in place, and code generation
//! reads the *same* trees (the schedule is rebuilt afterwards by
//! `semantic::resync_schedule`), so optimized output is what actually runs.

use crate::ast::*;
use crate::semantic::CheckedProgram;
use crate::types::Type;
use std::collections::HashMap;

fn is_pure(expr: &Expr) -> bool {
    matches!(
        expr.kind,
        ExprKind::Int(_)
            | ExprKind::Double(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Var(_)
    )
}

fn fold_binary(op: BinOp, left: &Expr, right: &Expr) -> Option<Expr> {
    let span = left.span;
    let ty = left.ty.clone();
    match (&left.kind, &right.kind) {
        (ExprKind::Int(a), ExprKind::Int(b)) => {
            let (a, b) = (*a, *b);
            let value = match op {
                BinOp::Add => a.checked_add(b)?,
                BinOp::Sub => a.checked_sub(b)?,
                BinOp::Mul => a.checked_mul(b)?,
                BinOp::Div => {
                    if b == 0 {
                        return None;
                    }
                    a.checked_div(b)?
                }
                BinOp::Mod => {
                    if b == 0 {
                        return None;
                    }
                    a.checked_rem(b)?
                }
                BinOp::Lt => return Some(Expr::typed(ExprKind::Bool(a < b), span, Type::Bool)),
                BinOp::Gt => return Some(Expr::typed(ExprKind::Bool(a > b), span, Type::Bool)),
                BinOp::Le => return Some(Expr::typed(ExprKind::Bool(a <= b), span, Type::Bool)),
                BinOp::Ge => return Some(Expr::typed(ExprKind::Bool(a >= b), span, Type::Bool)),
                BinOp::Eq => return Some(Expr::typed(ExprKind::Bool(a == b), span, Type::Bool)),
                BinOp::Ne => return Some(Expr::typed(ExprKind::Bool(a != b), span, Type::Bool)),
                BinOp::And => return None,
            };
            Some(Expr::typed(ExprKind::Int(value), span, Type::Int))
        }
        (ExprKind::Double(a), ExprKind::Double(b)) => {
            let (a, b) = (*a, *b);
            let value = match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div => {
                    if b == 0.0 {
                        return None;
                    }
                    a / b
                }
                BinOp::Lt => return Some(Expr::typed(ExprKind::Bool(a < b), span, Type::Bool)),
                BinOp::Gt => return Some(Expr::typed(ExprKind::Bool(a > b), span, Type::Bool)),
                BinOp::Le => return Some(Expr::typed(ExprKind::Bool(a <= b), span, Type::Bool)),
                BinOp::Ge => return Some(Expr::typed(ExprKind::Bool(a >= b), span, Type::Bool)),
                BinOp::Eq => return Some(Expr::typed(ExprKind::Bool(a == b), span, Type::Bool)),
                BinOp::Ne => return Some(Expr::typed(ExprKind::Bool(a != b), span, Type::Bool)),
                _ => return None,
            };
            Some(Expr::typed(ExprKind::Double(value), span, Type::Double))
        }
        (ExprKind::Bool(a), ExprKind::Bool(b)) => match op {
            BinOp::And => Some(Expr::typed(ExprKind::Bool(*a && *b), span, Type::Bool)),
            BinOp::Eq => Some(Expr::typed(ExprKind::Bool(a == b), span, Type::Bool)),
            BinOp::Ne => Some(Expr::typed(ExprKind::Bool(a != b), span, Type::Bool)),
            _ => None,
        },
        (ExprKind::Str(a), ExprKind::Str(b)) => match op {
            BinOp::Eq => Some(Expr::typed(ExprKind::Bool(a == b), span, Type::Bool)),
            BinOp::Ne => Some(Expr::typed(ExprKind::Bool(a != b), span, Type::Bool)),
            _ => None,
        },
        _ => {
            let _ = ty;
            None
        }
    }
}

/// Constant propagation environment: names known to hold a literal.
type ConstEnv = HashMap<String, Expr>;

fn optimize_expr(expr: &mut Expr, consts: &ConstEnv) {
    match &mut expr.kind {
        ExprKind::ArrayLit(elems) => {
            for elem in elems {
                optimize_expr(elem, consts);
            }
        }
        ExprKind::Index(base, index) => {
            optimize_expr(base, consts);
            optimize_expr(index, consts);
        }
        ExprKind::Field(base, _) => optimize_expr(base, consts),
        ExprKind::MethodCall(base, _, args) => {
            optimize_expr(base, consts);
            for arg in args {
                optimize_expr(arg, consts);
            }
        }
        ExprKind::Call(_, args) => {
            for arg in args {
                optimize_expr(arg, consts);
            }
        }
        ExprKind::StructLit(_, fields) => {
            for (_, value) in fields {
                optimize_expr(value, consts);
            }
        }
        ExprKind::Intrinsic(_, args) => {
            for arg in args {
                optimize_expr(arg, consts);
            }
        }
        ExprKind::UnaryMinus(inner) => {
            optimize_expr(inner, consts);
            let folded = match &inner.kind {
                ExprKind::Int(v) => Some(Expr::typed(ExprKind::Int(-v), expr.span, Type::Int)),
                ExprKind::Double(v) => {
                    Some(Expr::typed(ExprKind::Double(-v), expr.span, Type::Double))
                }
                _ => None,
            };
            if let Some(f) = folded {
                *expr = f;
            }
        }
        ExprKind::AddressOf(inner) => optimize_expr(inner, consts),
        ExprKind::Deref(inner) => optimize_expr(inner, consts),
        ExprKind::Try(inner) => optimize_expr(inner, consts),
        ExprKind::Var(name) => {
            // Constant propagation: replace a known literal binding.
            if let Some(replacement) = consts.get(name) {
                if matches!(replacement.ty, Some(Type::Int | Type::Double | Type::Bool))
                    || matches!(replacement.ty, Some(Type::String))
                {
                    let mut copy = replacement.clone();
                    copy.span = expr.span;
                    *expr = copy;
                }
            }
        }
        ExprKind::Binary(op, left, right) => {
            optimize_expr(left, consts);
            optimize_expr(right, consts);
            if let Some(folded) = fold_binary(*op, left, right) {
                *expr = folded;
                return;
            }
            // Algebraic identities on pure operands only, so no observable
            // side effect (bounds check, call) is removed.
            let simplified = match (op, &left.kind, &right.kind) {
                (BinOp::Add, _, ExprKind::Int(0)) if is_pure(left) => Some((**left).clone()),
                (BinOp::Add, ExprKind::Int(0), _) if is_pure(right) => Some((**right).clone()),
                (BinOp::Sub, _, ExprKind::Int(0)) if is_pure(left) => Some((**left).clone()),
                (BinOp::Mul, _, ExprKind::Int(1)) if is_pure(left) => Some((**left).clone()),
                (BinOp::Mul, ExprKind::Int(1), _) if is_pure(right) => Some((**right).clone()),
                (BinOp::Mul, _, ExprKind::Int(0)) if is_pure(left) => {
                    Some(Expr::typed(ExprKind::Int(0), left.span, Type::Int))
                }
                (BinOp::Mul, ExprKind::Int(0), _) if is_pure(right) => {
                    Some(Expr::typed(ExprKind::Int(0), right.span, Type::Int))
                }
                (BinOp::Div, _, ExprKind::Int(1)) if is_pure(left) => Some((**left).clone()),
                (BinOp::And, ExprKind::Bool(true), _) => Some((**right).clone()),
                (BinOp::And, ExprKind::Bool(false), _) => {
                    Some(Expr::typed(ExprKind::Bool(false), expr.span, Type::Bool))
                }
                _ => None,
            };
            if let Some(s) = simplified {
                *expr = s;
            }
        }
        _ => {}
    }
}

/// Which names in this statement list are assigned more than once (and must
/// therefore not be propagated)?
/// Names whose address is taken somewhere in this statement list: writes
/// through the pointer can change them, so they must not be propagated.
fn address_taken(stmts: &[Stmt], out: &mut HashMap<String, usize>) {
    fn walk_expr(expr: &Expr, out: &mut HashMap<String, usize>) {
        match &expr.kind {
            ExprKind::AddressOf(inner) => {
                if let ExprKind::Var(name) = &inner.kind {
                    *out.entry(name.clone()).or_insert(0) += 1;
                }
                walk_expr(inner, out);
            }
            ExprKind::Binary(_, l, r) => {
                walk_expr(l, out);
                walk_expr(r, out);
            }
            ExprKind::UnaryMinus(e) | ExprKind::Deref(e) | ExprKind::Try(e) => walk_expr(e, out),
            ExprKind::Index(b, i) => {
                walk_expr(b, out);
                walk_expr(i, out);
            }
            ExprKind::Field(b, _) => walk_expr(b, out),
            ExprKind::MethodCall(b, _, args) => {
                walk_expr(b, out);
                for a in args {
                    walk_expr(a, out);
                }
            }
            ExprKind::Call(_, args) => {
                for a in args {
                    walk_expr(a, out);
                }
            }
            ExprKind::Intrinsic(_, args) => {
                for a in args {
                    walk_expr(a, out);
                }
            }
            ExprKind::ArrayLit(items) => {
                for i in items {
                    walk_expr(i, out);
                }
            }
            ExprKind::StructLit(_, fields) => {
                for (_, v) in fields {
                    walk_expr(v, out);
                }
            }
            _ => {}
        }
    }
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Set { expr, .. }
            | StmtKind::Assign { expr, .. }
            | StmtKind::Insert { expr, .. } => walk_expr(expr, out),
            StmtKind::AssignTo { target, expr } => {
                walk_expr(target, out);
                walk_expr(expr, out);
            }
            StmtKind::Remove { index, .. } => walk_expr(index, out),
            StmtKind::Aout { args } => {
                for arg in args {
                    if let AoutArg::Evaluated(e) = arg {
                        walk_expr(e, out);
                    }
                }
            }
            StmtKind::When {
                branches,
                else_body,
            } => {
                for (cond, body) in branches {
                    walk_expr(cond, out);
                    address_taken(body, out);
                }
                if let Some(body) = else_body {
                    address_taken(body, out);
                }
            }
            StmtKind::Loop { body, .. } => address_taken(body, out),
            StmtKind::While {
                cond,
                body,
                cleanup,
            } => {
                walk_expr(cond, out);
                address_taken(body, out);
                address_taken(cleanup, out);
            }
            StmtKind::Each { iterable, body, .. } => {
                walk_expr(iterable, out);
                address_taken(body, out);
            }
            StmtKind::Return { expr: Some(e) } | StmtKind::Fail { expr: e } => walk_expr(e, out),
            _ => {}
        }
    }
}

fn reassigned_names(stmts: &[Stmt], out: &mut HashMap<String, usize>) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Set { name, .. } => *out.entry(name.clone()).or_insert(0) += 1,
            StmtKind::Assign { name, .. } => *out.entry(name.clone()).or_insert(0) += 1,
            StmtKind::AssignTo { target, .. } => {
                if let ExprKind::Var(name) = &target.kind {
                    *out.entry(name.clone()).or_insert(0) += 1;
                }
            }
            StmtKind::Insert { name, .. } => *out.entry(name.clone()).or_insert(0) += 1,
            StmtKind::When {
                branches,
                else_body,
            } => {
                for (_, body) in branches {
                    reassigned_names(body, out);
                }
                if let Some(body) = else_body {
                    reassigned_names(body, out);
                }
            }
            StmtKind::Loop { body, .. } => reassigned_names(body, out),
            StmtKind::While { body, cleanup, .. } => {
                reassigned_names(body, out);
                reassigned_names(cleanup, out);
            }
            StmtKind::Each { body, .. } => reassigned_names(body, out),
            _ => {}
        }
    }
}

fn optimize_stmts(stmts: &mut Vec<Stmt>, consts: &ConstEnv) -> ConstEnv {
    let mut local_consts = consts.clone();
    let mut counts = HashMap::new();
    reassigned_names(stmts, &mut counts);
    // Address-taken names are never propagated: `*p = 42` can change them.
    address_taken(stmts, &mut counts);

    let mut kept: Vec<Stmt> = Vec::new();
    for mut stmt in stmts.drain(..) {
        // Optimize with everything learned so far: a literal bound by an
        // earlier statement in this same list is already visible here.
        optimize_stmt(&mut stmt, &local_consts);

        // A condition that folded to a literal decides the branch now.
        if let StmtKind::When {
            branches,
            else_body,
        } = stmt.kind
        {
            let mut live_branches = Vec::new();
            let mut taken = false;
            for (cond, body) in branches {
                if let ExprKind::Bool(value) = cond.kind {
                    if value && !taken {
                        live_branches.push((cond, body));
                        taken = true;
                    }
                    continue;
                }
                if taken {
                    continue;
                }
                live_branches.push((cond, body));
            }
            if live_branches.is_empty() {
                if let Some(body) = else_body {
                    for s in body {
                        kept.push(s);
                    }
                }
            } else {
                kept.push(Stmt::new(
                    StmtKind::When {
                        branches: live_branches,
                        else_body,
                    },
                    stmt.span,
                ));
            }
            continue;
        }

        // Record literal bindings for the statements that follow.
        if let StmtKind::Set { name, expr, .. } = &stmt.kind {
            let is_literal = matches!(
                expr.kind,
                ExprKind::Int(_) | ExprKind::Double(_) | ExprKind::Bool(_) | ExprKind::Str(_)
            );
            let count = counts.get(name).copied().unwrap_or(0);
            if is_literal && count <= 1 {
                local_consts.insert(name.clone(), expr.clone());
            }
        }
        kept.push(stmt);
    }

    // Dead code after return/fail.
    if let Some(pos) = kept
        .iter()
        .position(|s| matches!(s.kind, StmtKind::Return { .. } | StmtKind::Fail { .. }))
    {
        kept.truncate(pos + 1);
    }
    *stmts = kept;
    local_consts
}

fn optimize_stmt(stmt: &mut Stmt, consts: &ConstEnv) {
    match &mut stmt.kind {
        StmtKind::Set { expr, .. } => optimize_expr(expr, consts),
        StmtKind::Assign { expr, .. } => optimize_expr(expr, consts),
        StmtKind::AssignTo { target, expr } => {
            optimize_expr(target, consts);
            optimize_expr(expr, consts);
        }
        StmtKind::Insert { expr, .. } => optimize_expr(expr, consts),
        StmtKind::Remove { index, .. } => optimize_expr(index, consts),
        StmtKind::Aout { args } => {
            for arg in args {
                if let AoutArg::Evaluated(expr) = arg {
                    optimize_expr(expr, consts);
                }
            }
        }
        StmtKind::When {
            branches,
            else_body,
        } => {
            for (cond, body) in branches {
                optimize_expr(cond, consts);
                optimize_stmts(body, consts);
            }
            if let Some(body) = else_body {
                optimize_stmts(body, consts);
            }
        }
        StmtKind::Loop { body, count } => {
            optimize_stmts(body, consts);
            if let Some(c) = count {
                optimize_expr(c, consts);
            }
        }
        StmtKind::While {
            cond,
            body,
            cleanup,
        } => {
            optimize_expr(cond, consts);
            optimize_stmts(body, consts);
            optimize_stmts(cleanup, consts);
        }
        StmtKind::Each { iterable, body, .. } => {
            optimize_expr(iterable, consts);
            optimize_stmts(body, consts);
        }
        StmtKind::Return { expr } => {
            if let Some(e) = expr {
                optimize_expr(e, consts);
            }
        }
        StmtKind::Fail { expr } => optimize_expr(expr, consts),
        StmtKind::ExprStmt { expr } => optimize_expr(expr, consts),
    }
}

/// Optimize a whole checked program in place.
pub fn optimize(checked: &mut CheckedProgram) {
    let empty = ConstEnv::new();
    optimize_stmts(&mut checked.top_level, &empty);
    for (_, stmts) in checked.module_top_level.iter_mut() {
        optimize_stmts(stmts, &empty);
    }
    for decl in checked.program.decls.iter_mut() {
        match decl {
            Decl::Block(b) => {
                optimize_stmts(&mut b.body, &empty);
            }
            Decl::Reuse(r) => {
                optimize_stmts(&mut r.body, &empty);
            }
            Decl::Impl(i) => {
                for method in i.methods.iter_mut() {
                    optimize_stmts(&mut method.body, &empty);
                }
            }
            _ => {}
        }
    }
    for block in checked.blocks_sorted.iter_mut() {
        optimize_stmts(&mut block.decl.body, &empty);
    }
    for instance in checked.instances.iter_mut() {
        optimize_stmts(&mut instance.decl.body, &empty);
    }
}

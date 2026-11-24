//! Abstract syntax tree for -A Core.
//!
//! The tree is deliberately simple: everything that survives parsing is a
//! declaration ([`Decl`]) or a statement ([`Stmt`]). Types written by the
//! programmer (struct fields, reuse signatures, trait methods) are kept as
//! [`TypeExpr`] and resolved during semantic analysis.

use crate::diagnostics::Span;
use crate::types::Type;

/// Binary operators. There is deliberately no `Or` operator in Core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    And,
}

impl BinOp {
    pub fn symbol(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::And => "and",
        }
    }

    pub fn is_comparison(&self) -> bool {
        matches!(
            self,
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne
        )
    }
}

/// A type as written in source: `int`, `Point`, `Box<T>`, `ptr<int>`.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String, Vec<TypeExpr>),
    Ptr(Box<TypeExpr>),
    Array(Box<TypeExpr>),
}

impl TypeExpr {
    pub fn name(&self) -> String {
        match self {
            TypeExpr::Named(n, args) => {
                if args.is_empty() {
                    n.clone()
                } else {
                    let inner: Vec<String> = args.iter().map(|a| a.name()).collect();
                    format!("{}<{}>", n, inner.join(", "))
                }
            }
            TypeExpr::Ptr(inner) => format!("ptr<{}>", inner.name()),
            TypeExpr::Array(inner) => format!("[{}]", inner.name()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Double(f64),
    Str(String),
    Bool(bool),
    Var(String),
    ArrayLit(Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    /// `base.field` / `base.length` / `result.ok`
    Field(Box<Expr>, String),
    /// `base.method(args)` — resolved statically against a trait impl.
    MethodCall(Box<Expr>, String, Vec<Expr>),
    Call(String, Vec<Expr>),
    /// `Point { x = 1, y = 2 }`
    StructLit(String, Vec<(String, Expr)>),
    /// `@mem_copy(dst, src, n)`
    Intrinsic(String, Vec<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    UnaryMinus(Box<Expr>),
    /// `&value` — address of an lvalue, or of a temporary cell.
    AddressOf(Box<Expr>),
    /// `*pointer`
    Deref(Box<Expr>),
    /// `call()?` — propagate a failure to the enclosing reuse.
    Try(Box<Expr>),
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    /// Resolved by semantic analysis. Code generation reads this instead of
    /// re-inferring types, so the tree the backend sees is exactly the tree
    /// that was checked.
    pub ty: Option<Type>,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Expr {
            kind,
            span,
            ty: None,
        }
    }

    pub fn typed(kind: ExprKind, span: Span, ty: Type) -> Self {
        Expr {
            kind,
            span,
            ty: Some(ty),
        }
    }
}

/// The operation of an `insert` statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// One argument of an `aout()` call: either a literal string printed
/// directly, or an expression marked by the `A` execution marker.
#[derive(Debug, Clone)]
pub enum AoutArg {
    Literal(String, Span),
    Evaluated(Expr),
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Set {
        name: String,
        mutable: bool,
        expr: Expr,
    },
    Assign {
        name: String,
        expr: Expr,
    },
    /// Assignment to a field, index or dereferenced pointer: `p.x = 1`.
    AssignTo {
        target: Expr,
        expr: Expr,
    },
    Insert {
        name: String,
        op: InsertOp,
        expr: Expr,
    },
    Remove {
        name: String,
        index: Expr,
    },
    Aout {
        args: Vec<AoutArg>,
    },
    When {
        branches: Vec<(Expr, Vec<Stmt>)>,
        else_body: Option<Vec<Stmt>>,
    },
    /// `count: Some(n)` is a counted loop; `count: None` wraps `while`.
    Loop {
        body: Vec<Stmt>,
        count: Option<Expr>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
        cleanup: Vec<Stmt>,
    },
    Each {
        var: String,
        iterable: Expr,
        body: Vec<Stmt>,
    },
    Return {
        expr: Option<Expr>,
    },
    /// `fail expr` — return a failure from a reuse, or terminate a block
    /// with an error.
    Fail {
        expr: Expr,
    },
    ExprStmt {
        expr: Expr,
    },
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

impl Stmt {
    pub fn new(kind: StmtKind, span: Span) -> Self {
        Stmt { kind, span }
    }
}

/// Execution-queue specification from a block header:
/// `(N)` single block, `(N:N)` two parallel blocks, `(N:NxK)` K parallel blocks.
#[derive(Debug, Clone, Copy)]
pub struct QueueSpec {
    pub queue: u32,
    /// `Some(k)` when the header declares a parallel group of `k` blocks
    /// (`(N:N)` is `Some(2)`, `(N:Nx4)` is `Some(4)`); `None` for a plain
    /// `(N)` queue in which the block runs on its own.
    pub group_size: Option<u32>,
    pub span: Span,
}

/// One reuse attached to a block: `random` or `random_between(1, 100)`.
#[derive(Debug, Clone)]
pub struct Attach {
    pub name: String,
    /// Local name the cached result is bound to (defaults to `name`).
    pub binding: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct BlockDecl {
    pub name: String,
    pub attaches: Vec<Attach>,
    pub queue: QueueSpec,
    pub body: Vec<Stmt>,
    pub span: Span,
    /// Module this block was declared in (empty for the root module).
    pub module: String,
    /// True when the block comes from an imported module and is therefore
    /// not part of this program's execution schedule.
    pub imported: bool,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Option<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ReuseDecl {
    pub name: String,
    pub generics: Vec<String>,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub body: Vec<Stmt>,
    pub span: Span,
    pub is_pub: bool,
    pub module: String,
    /// Trait this method implements, when declared inside `impl Trait for Type`.
    pub trait_name: Option<String>,
    /// Self type bound by `self` in a trait method.
    pub self_ty: Option<TypeExpr>,
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: String,
    pub generics: Vec<String>,
    pub fields: Vec<(String, TypeExpr)>,
    pub span: Span,
    pub is_pub: bool,
    pub module: String,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
    pub value: Option<i64>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<Variant>,
    pub span: Span,
    pub is_pub: bool,
    pub module: String,
}

#[derive(Debug, Clone)]
pub struct MethodSig {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TraitDecl {
    pub name: String,
    pub methods: Vec<MethodSig>,
    pub span: Span,
    pub is_pub: bool,
    pub module: String,
}

#[derive(Debug, Clone)]
pub struct ImplDecl {
    /// Trait being implemented. Empty for an inherent `impl Type { ... }`,
    /// which attaches methods to a type without naming a trait.
    pub trait_name: String,
    /// True when no trait is named (`impl Type { ... }`).
    pub inherent: bool,
    pub type_name: TypeExpr,
    pub methods: Vec<ReuseDecl>,
    pub span: Span,
    pub module: String,
}

/// `pub? decl` visibility marker is carried on each declaration.
#[derive(Debug, Clone)]
pub enum Decl {
    Block(BlockDecl),
    Reuse(ReuseDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Trait(TraitDecl),
    Impl(ImplDecl),
}

impl Decl {
    pub fn name(&self) -> String {
        match self {
            Decl::Block(b) => b.name.clone(),
            Decl::Reuse(r) => r.name.clone(),
            Decl::Struct(s) => s.name.clone(),
            Decl::Enum(e) => e.name.clone(),
            Decl::Trait(t) => t.name.clone(),
            Decl::Impl(i) => i.type_name.name(),
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Decl::Block(b) => b.span,
            Decl::Reuse(r) => r.span,
            Decl::Struct(s) => s.span,
            Decl::Enum(e) => e.span,
            Decl::Trait(t) => t.span,
            Decl::Impl(i) => i.span,
        }
    }

    pub fn is_pub(&self) -> bool {
        match self {
            Decl::Block(_) | Decl::Impl(_) => true,
            Decl::Reuse(r) => r.is_pub,
            Decl::Struct(s) => s.is_pub,
            Decl::Enum(e) => e.is_pub,
            Decl::Trait(t) => t.is_pub,
        }
    }
}

/// A single source file after parsing.
#[derive(Debug, Clone)]
pub struct Program {
    /// Module path segments, e.g. `["utils", "math"]`. Empty for the root file.
    pub module_path: Vec<String>,
    pub imports: Vec<ImportDecl>,
    pub decls: Vec<Decl>,
    pub top_level: Vec<Stmt>,
}

impl Program {
    pub fn new() -> Program {
        Program {
            module_path: Vec::new(),
            imports: Vec::new(),
            decls: Vec::new(),
            top_level: Vec::new(),
        }
    }

    /// `"utils.math"`, or `""` for the root module.
    pub fn module_name(&self) -> String {
        self.module_path.join(".")
    }
}

#[derive(Debug, Clone)]
pub struct ImportDecl {
    /// Path segments as written, e.g. `["utils", "math"]`.
    pub path: Vec<String>,
    pub span: Span,
}

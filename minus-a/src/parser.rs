//! Recursive-descent parser for -A Core.
//!
//! Grammar (informal):
//!
//! ```text
//! program   := ("module" path)? import* (decl | stmt)* EOF
//! import    := "import" path
//! path      := IDENT ("." IDENT)*
//! decl      := "pub"? (block | reuse | struct | enum | trait | impl)
//! block     := "block" IDENT "(" (attach ",")* queue ")" "{" stmt* "}"
//! attach    := IDENT ("(" args ")")?
//! queue     := INT (":" INT ("x" INT)?)?
//! reuse     := "reuse" IDENT generics? "(" params? ")" ("->" type)? "{" stmt* "}"
//! struct    := "struct" IDENT generics? "{" (IDENT ":" type)* "}"
//! enum      := "enum" IDENT "{" (IDENT ("=" INT)?)* "}"
//! trait     := "trait" IDENT "{" (reuse_sig)* "}"
//! impl      := "impl" IDENT "for" type "{" decl_reuse* "}"
//! stmt      := set | setc | insert | remove | aout | when | loop | each
//!            | return | fail | assignment | expr
//! expr      := and_expr ("?" )*
//! ```

use crate::ast::*;
use crate::diagnostics::{parse_error, CompileResult, Span};
use crate::lexer::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Nesting depth of `(`/`[` delimiters currently open. A newline only ends
    /// a statement at depth 0, so expressions wrapped in parentheses or
    /// brackets may span lines while `set p = &x` / `*p = 42` stay separate
    /// statements.
    depth: usize,
}

fn is_snake_case(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !name.chars().next().unwrap().is_ascii_digit()
}

fn is_type_case(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.chars().next().unwrap().is_ascii_uppercase()
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            pos: 0,
            depth: 0,
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn next(&mut self) -> Token {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::LParen | TokenKind::LBracket => self.depth += 1,
            TokenKind::RParen | TokenKind::RBracket => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    /// True when the next token starts a new source line and is not nested in
    /// parentheses or brackets, i.e. where a statement ends.
    fn at_line_break(&self) -> bool {
        self.depth == 0 && self.peek().nl_before
    }

    fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.next();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind, what: &str) -> CompileResult<Token> {
        if self.at(kind) {
            Ok(self.next())
        } else {
            Err(parse_error(
                format!("expected {}, found '{}'", what, self.describe_peek()),
                self.peek().span,
            ))
        }
    }

    fn describe_peek(&self) -> String {
        let tok = self.peek();
        if matches!(tok.kind, TokenKind::Eof) {
            "end of file".to_string()
        } else if tok.text.is_empty() {
            format!("{:?}", tok.kind)
        } else {
            tok.text.clone()
        }
    }

    fn expect_ident(&mut self, what: &str) -> CompileResult<(String, Span)> {
        match self.peek().kind.clone() {
            TokenKind::Ident(name) => {
                let span = self.peek().span;
                self.next();
                Ok((name, span))
            }
            _ => Err(parse_error(
                format!("expected {}, found '{}'", what, self.describe_peek()),
                self.peek().span,
            )),
        }
    }

    // -----------------------------------------------------------------
    // Top level
    // -----------------------------------------------------------------

    pub fn parse_program(&mut self) -> CompileResult<Program> {
        let mut program = Program::new();

        // Optional module header: `module utils.math`
        if self.at(&TokenKind::Module) {
            let span = self.next().span;
            let path = self.parse_path("module name")?;
            if path.is_empty() {
                return Err(parse_error("expected a module name", span));
            }
            program.module_path = path;
        }

        while self.at(&TokenKind::Import) {
            let span = self.next().span;
            let path = self.parse_path("module path after 'import'")?;
            if path.is_empty() {
                return Err(parse_error("expected a module path after 'import'", span));
            }
            program.imports.push(ImportDecl { path, span });
        }

        while !matches!(self.peek().kind, TokenKind::Eof) {
            if self.at(&TokenKind::Import) {
                let span = self.next().span;
                let path = self.parse_path("module path after 'import'")?;
                program.imports.push(ImportDecl { path, span });
                continue;
            }
            if self.starts_decl() {
                let decl = self.parse_decl()?;
                program.decls.push(decl);
            } else {
                let stmt = self.parse_stmt()?;
                program.top_level.push(stmt);
            }
        }
        Ok(program)
    }

    fn parse_path(&mut self, what: &str) -> CompileResult<Vec<String>> {
        let mut segments = Vec::new();
        let (first, _) = self.expect_ident(what)?;
        segments.push(first);
        while self.at(&TokenKind::Dot) {
            self.next();
            let (seg, _) = self.expect_ident("path segment")?;
            segments.push(seg);
        }
        Ok(segments)
    }

    fn starts_decl(&self) -> bool {
        matches!(
            self.peek().kind,
            TokenKind::Block
                | TokenKind::Reuse
                | TokenKind::Struct
                | TokenKind::Enum
                | TokenKind::Trait
                | TokenKind::Impl
                | TokenKind::Pub
        )
    }

    fn parse_decl(&mut self) -> CompileResult<Decl> {
        let is_pub = self.eat(&TokenKind::Pub);
        match self.peek().kind {
            TokenKind::Block => {
                if is_pub {
                    return Err(parse_error(
                        "blocks are scheduled units and are always public; remove 'pub'",
                        self.peek().span,
                    ));
                }
                Ok(Decl::Block(self.parse_block_decl()?))
            }
            TokenKind::Reuse => Ok(Decl::Reuse(self.parse_reuse_decl(is_pub, false)?)),
            TokenKind::Struct => Ok(Decl::Struct(self.parse_struct_decl(is_pub)?)),
            TokenKind::Enum => Ok(Decl::Enum(self.parse_enum_decl(is_pub)?)),
            TokenKind::Trait => Ok(Decl::Trait(self.parse_trait_decl(is_pub)?)),
            TokenKind::Impl => {
                if is_pub {
                    return Err(parse_error(
                        "'impl' blocks are always visible; remove 'pub'",
                        self.peek().span,
                    ));
                }
                Ok(Decl::Impl(self.parse_impl_decl()?))
            }
            _ => Err(parse_error(
                format!("expected a declaration, found '{}'", self.describe_peek()),
                self.peek().span,
            )),
        }
    }

    // -----------------------------------------------------------------
    // Declarations
    // -----------------------------------------------------------------

    fn parse_generics(&mut self) -> CompileResult<Vec<String>> {
        let mut generics = Vec::new();
        if !self.at(&TokenKind::Lt) {
            return Ok(generics);
        }
        self.next();
        loop {
            let (name, _) = self.expect_ident("type parameter name")?;
            generics.push(name);
            if self.eat(&TokenKind::Comma) {
                continue;
            }
            self.expect(&TokenKind::Gt, "'>' to close the type parameter list")?;
            break;
        }
        Ok(generics)
    }

    /// Parse `(params)` used by reuse declarations, trait methods and impl
    /// methods. Parameters may carry an explicit `: type`, and the first one
    /// may be `self`.
    fn parse_params(&mut self) -> CompileResult<Vec<Param>> {
        self.expect(&TokenKind::LParen, "'(' to open the parameter list")?;
        let mut params = Vec::new();
        if self.eat(&TokenKind::RParen) {
            return Ok(params);
        }
        loop {
            let span = self.peek().span;
            if self.at(&TokenKind::SelfKw) {
                self.next();
                params.push(Param {
                    name: "self".to_string(),
                    ty: None,
                    span,
                });
            } else {
                let (name, nspan) = self.expect_ident("parameter name")?;
                let ty = if self.eat(&TokenKind::Colon) {
                    Some(self.parse_type_expr()?)
                } else {
                    None
                };
                params.push(Param {
                    name,
                    ty,
                    span: nspan,
                });
            }
            if self.eat(&TokenKind::Comma) {
                continue;
            }
            self.expect(&TokenKind::RParen, "')' to close the parameter list")?;
            break;
        }
        Ok(params)
    }

    fn parse_return_type(&mut self) -> CompileResult<Option<TypeExpr>> {
        if self.eat(&TokenKind::Arrow) {
            Ok(Some(self.parse_type_expr()?))
        } else {
            Ok(None)
        }
    }

    fn parse_type_expr(&mut self) -> CompileResult<TypeExpr> {
        if self.eat(&TokenKind::Amp) {
            let inner = self.parse_type_expr()?;
            return Ok(TypeExpr::Ptr(Box::new(inner)));
        }
        if self.eat(&TokenKind::LBracket) {
            let inner = self.parse_type_expr()?;
            self.expect(&TokenKind::RBracket, "']' to close the array type")?;
            return Ok(TypeExpr::Array(Box::new(inner)));
        }
        let (name, span) = self.expect_ident("a type name")?;
        if name == "ptr" && self.at(&TokenKind::Lt) {
            self.next();
            let inner = self.parse_type_expr()?;
            self.expect(&TokenKind::Gt, "'>' to close the pointer type")?;
            return Ok(TypeExpr::Ptr(Box::new(inner)));
        }
        let mut args = Vec::new();
        if self.at(&TokenKind::Lt) {
            self.next();
            loop {
                args.push(self.parse_type_expr()?);
                if self.eat(&TokenKind::Comma) {
                    continue;
                }
                self.expect(&TokenKind::Gt, "'>' to close the type arguments")?;
                break;
            }
        }
        let _ = span;
        Ok(TypeExpr::Named(name, args))
    }

    fn parse_block_decl(&mut self) -> CompileResult<BlockDecl> {
        let start = self.expect(&TokenKind::Block, "'block'")?.span;
        let (name, name_span) = self.expect_ident("block name")?;
        if !is_snake_case(&name) {
            return Err(parse_error(
                format!(
                    "block name '{}' must use snake_case with underscores, never spaces",
                    name
                ),
                name_span,
            ));
        }
        self.expect(&TokenKind::LParen, "'(' after the block name")?;

        let mut attaches = Vec::new();
        let queue;
        // The queue specification is the final item in the header.
        if matches!(self.peek().kind, TokenKind::Int(_)) {
            queue = self.parse_queue_spec()?;
        } else {
            loop {
                let (attach_name, attach_span) = self.expect_ident("attached reuse name")?;
                if !is_snake_case(&attach_name) {
                    return Err(parse_error(
                        format!("reuse name '{}' must be snake_case", attach_name),
                        attach_span,
                    ));
                }
                let mut args = Vec::new();
                if self.eat(&TokenKind::LParen) {
                    if !self.eat(&TokenKind::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.eat(&TokenKind::Comma) {
                                continue;
                            }
                            self.expect(&TokenKind::RParen, "')' to close the reuse arguments")?;
                            break;
                        }
                    }
                }
                // Optional local binding: `random as roll`
                let binding = if self.eat(&TokenKind::As) {
                    let (b, _) = self.expect_ident("binding name")?;
                    b
                } else {
                    attach_name.clone()
                };
                attaches.push(Attach {
                    name: attach_name,
                    binding,
                    args,
                    span: attach_span,
                });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
                if matches!(self.peek().kind, TokenKind::Int(_)) {
                    break;
                }
            }
            queue = self.parse_queue_spec()?;
        }
        self.expect(&TokenKind::RParen, "')' to close the block header")?;
        let body = self.parse_block_body()?;
        Ok(BlockDecl {
            name,
            attaches,
            queue,
            body,
            span: start,
            module: String::new(),
            imported: false,
        })
    }

    fn parse_queue_spec(&mut self) -> CompileResult<QueueSpec> {
        let tok = self.next();
        let token_span = tok.span;
        let n = match tok.kind {
            TokenKind::Int(v) if v > 0 => v as u32,
            TokenKind::Int(_) => {
                return Err(parse_error(
                    "invalid queue number 0; queues start at 1",
                    token_span,
                ))
            }
            _ => {
                return Err(parse_error(
                    format!("expected a queue number, found '{}'", tok.text),
                    token_span,
                ))
            }
        };
        let mut group_size: Option<u32> = None;
        if self.eat(&TokenKind::Colon) {
            let second = self.next();
            let m = match second.kind {
                TokenKind::Int(v) if v > 0 => v as u32,
                _ => {
                    return Err(parse_error(
                        "expected a queue number after ':' (as in '2:2')",
                        second.span,
                    ))
                }
            };
            if m != n {
                return Err(parse_error(
                    format!(
                        "invalid parallel queue '{}:{}': both numbers must match (use {}:{} or {}:{}xK)",
                        n, m, n, n, n, n
                    ),
                    token_span,
                ));
            }
            group_size = Some(2);
            if let TokenKind::Ident(k) = self.peek().kind.clone() {
                if let Some(count) = k.strip_prefix('x') {
                    if let Ok(parsed) = count.parse::<u32>() {
                        if parsed < 2 {
                            return Err(parse_error(
                                "the group size after 'x' must be at least 2; use (N:N) for two blocks",
                                self.peek().span,
                            ));
                        }
                        self.next();
                        group_size = Some(parsed);
                    }
                }
            }
        }
        Ok(QueueSpec {
            queue: n,
            group_size,
            span: token_span,
        })
    }

    fn parse_block_body(&mut self) -> CompileResult<Vec<Stmt>> {
        self.expect(&TokenKind::LBrace, "'{' to open the body")?;
        let mut stmts = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            if matches!(self.peek().kind, TokenKind::Eof) {
                return Err(parse_error(
                    "unexpected end of file: missing '}'",
                    self.peek().span,
                ));
            }
            stmts.push(self.parse_stmt()?);
        }
        self.expect(&TokenKind::RBrace, "'}' to close the body")?;
        Ok(stmts)
    }

    fn parse_reuse_decl(&mut self, is_pub: bool, allow_self: bool) -> CompileResult<ReuseDecl> {
        let start = self.expect(&TokenKind::Reuse, "'reuse'")?.span;
        let (name, name_span) = self.expect_ident("reuse name")?;
        if !is_snake_case(&name) {
            return Err(parse_error(
                format!("reuse name '{}' must be snake_case", name),
                name_span,
            ));
        }
        let generics = self.parse_generics()?;
        let params = self.parse_params()?;
        let ret = self.parse_return_type()?;
        let body = self.parse_block_body()?;
        let _ = allow_self;
        Ok(ReuseDecl {
            name,
            generics,
            params,
            ret,
            body,
            span: start,
            is_pub,
            module: String::new(),
            trait_name: None,
            self_ty: None,
        })
    }

    fn parse_struct_decl(&mut self, is_pub: bool) -> CompileResult<StructDecl> {
        let start = self.expect(&TokenKind::Struct, "'struct'")?.span;
        let (name, name_span) = self.expect_ident("struct name")?;
        if !is_type_case(&name) {
            return Err(parse_error(
                format!("struct name '{}' must be PascalCase", name),
                name_span,
            ));
        }
        let generics = self.parse_generics()?;
        self.expect(&TokenKind::LBrace, "'{' to open the struct body")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let (field, fspan) = self.expect_ident("field name")?;
            if !is_snake_case(&field) {
                return Err(parse_error(
                    format!("field name '{}' must be snake_case", field),
                    fspan,
                ));
            }
            self.expect(&TokenKind::Colon, "':' after the field name")?;
            let ty = self.parse_type_expr()?;
            fields.push((field, ty));
            self.eat(&TokenKind::Comma);
        }
        self.expect(&TokenKind::RBrace, "'}' to close the struct body")?;
        Ok(StructDecl {
            name,
            generics,
            fields,
            span: start,
            is_pub,
            module: String::new(),
        })
    }

    fn parse_enum_decl(&mut self, is_pub: bool) -> CompileResult<EnumDecl> {
        let start = self.expect(&TokenKind::Enum, "'enum'")?.span;
        let (name, name_span) = self.expect_ident("enum name")?;
        if !is_type_case(&name) {
            return Err(parse_error(
                format!("enum name '{}' must be PascalCase", name),
                name_span,
            ));
        }
        self.expect(&TokenKind::LBrace, "'{' to open the enum body")?;
        let mut variants = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let (variant, vspan) = self.expect_ident("variant name")?;
            if !is_snake_case(&variant) {
                return Err(parse_error(
                    format!("enum variant '{}' must be snake_case", variant),
                    vspan,
                ));
            }
            let value = if self.eat(&TokenKind::Eq) {
                match self.next().kind {
                    TokenKind::Int(v) => Some(v),
                    other => {
                        return Err(parse_error(
                            format!("enum variant values must be integers, found {:?}", other),
                            vspan,
                        ))
                    }
                }
            } else {
                None
            };
            variants.push(Variant {
                name: variant,
                value,
                span: vspan,
            });
            self.eat(&TokenKind::Comma);
        }
        self.expect(&TokenKind::RBrace, "'}' to close the enum body")?;
        Ok(EnumDecl {
            name,
            variants,
            span: start,
            is_pub,
            module: String::new(),
        })
    }

    fn parse_trait_decl(&mut self, is_pub: bool) -> CompileResult<TraitDecl> {
        let start = self.expect(&TokenKind::Trait, "'trait'")?.span;
        let (name, name_span) = self.expect_ident("trait name")?;
        if !is_type_case(&name) {
            return Err(parse_error(
                format!("trait name '{}' must be PascalCase", name),
                name_span,
            ));
        }
        self.expect(&TokenKind::LBrace, "'{' to open the trait body")?;
        let mut methods = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let mspan = self
                .expect(&TokenKind::Reuse, "'reuse' inside a trait")?
                .span;
            let (mname, _) = self.expect_ident("method name")?;
            if !is_snake_case(&mname) {
                return Err(parse_error(
                    format!("method name '{}' must be snake_case", mname),
                    mspan,
                ));
            }
            let params = self.parse_params()?;
            let ret = self.parse_return_type()?;
            // A trait method is a signature; a body here is a mistake.
            if self.at(&TokenKind::LBrace) {
                return Err(parse_error(
                    "trait methods declare behaviour only; put the body in an 'impl' block",
                    self.peek().span,
                ));
            }
            methods.push(MethodSig {
                name: mname,
                params,
                ret,
                span: mspan,
            });
        }
        self.expect(&TokenKind::RBrace, "'}' to close the trait body")?;
        Ok(TraitDecl {
            name,
            methods,
            span: start,
            is_pub,
            module: String::new(),
        })
    }

    fn parse_impl_decl(&mut self) -> CompileResult<ImplDecl> {
        let start = self.expect(&TokenKind::Impl, "'impl'")?.span;
        let (first, _) = self.expect_ident("trait name or type name after 'impl'")?;
        // `impl Trait for Type { ... }` implements a trait, `impl Type { ... }`
        // attaches methods to the type directly.
        let (trait_name, inherent, type_name) = if self.eat(&TokenKind::For) {
            (first, false, self.parse_type_expr()?)
        } else {
            (String::new(), true, TypeExpr::Named(first, Vec::new()))
        };
        self.expect(&TokenKind::LBrace, "'{' to open the impl body")?;
        let mut methods = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            if !self.at(&TokenKind::Reuse) {
                return Err(parse_error(
                    "an impl block contains 'reuse' method definitions",
                    self.peek().span,
                ));
            }
            let mut method = self.parse_reuse_decl(false, true)?;
            if !inherent {
                method.trait_name = Some(trait_name.clone());
            }
            method.self_ty = Some(type_name.clone());
            methods.push(method);
        }
        self.expect(&TokenKind::RBrace, "'}' to close the impl body")?;
        Ok(ImplDecl {
            trait_name,
            inherent,
            type_name,
            methods,
            span: start,
            module: String::new(),
        })
    }

    // -----------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------

    fn parse_stmt(&mut self) -> CompileResult<Stmt> {
        let span = self.peek().span;
        match self.peek().kind.clone() {
            TokenKind::Set | TokenKind::Setc => {
                let mutable = self.eat(&TokenKind::Setc);
                if !mutable {
                    self.next();
                }
                let (name, name_span) = self.expect_ident("variable name")?;
                if !is_snake_case(&name) {
                    return Err(parse_error(
                        format!("variable name '{}' must be snake_case", name),
                        name_span,
                    ));
                }
                self.expect(&TokenKind::Eq, "'=' after the variable name")?;
                let expr = self.parse_expr()?;
                Ok(Stmt::new(
                    StmtKind::Set {
                        name,
                        mutable,
                        expr,
                    },
                    span,
                ))
            }
            TokenKind::Insert => {
                self.next();
                let (name, _) = self.expect_ident("variable name after 'insert'")?;
                self.expect(&TokenKind::Eq, "'=' after 'insert <name>'")?;
                // The operand prefix selects the operation: `5` / `-5` add and
                // subtract, `x5` multiplies, `/5` divides. A bare variable name
                // is an addition, and on an array `insert a = 5` appends.
                let (op, expr) = if self.eat(&TokenKind::Minus) {
                    (InsertOp::Sub, self.parse_expr()?)
                } else if self.eat(&TokenKind::Slash) {
                    (InsertOp::Div, self.parse_expr()?)
                } else if let TokenKind::Ident(text) = self.peek().kind.clone() {
                    match text.strip_prefix('x') {
                        Some(rest) => {
                            let tok = self.next();
                            let expr = match rest.parse::<i64>() {
                                Ok(v) => Expr::new(ExprKind::Int(v), tok.span),
                                Err(_) => Expr::new(ExprKind::Var(rest.to_string()), tok.span),
                            };
                            (InsertOp::Mul, expr)
                        }
                        None => (InsertOp::Add, self.parse_expr()?),
                    }
                } else {
                    (InsertOp::Add, self.parse_expr()?)
                };
                Ok(Stmt::new(StmtKind::Insert { name, op, expr }, span))
            }
            TokenKind::Remove => {
                self.next();
                let (name, _) = self.expect_ident("array name after 'remove'")?;
                self.expect(&TokenKind::LBracket, "'[' with the index to remove")?;
                let index = self.parse_expr()?;
                self.expect(&TokenKind::RBracket, "']' after the index")?;
                Ok(Stmt::new(StmtKind::Remove { name, index }, span))
            }
            TokenKind::Aout => {
                self.next();
                self.expect(&TokenKind::LParen, "'(' after 'aout'")?;
                let mut args = Vec::new();
                if !self.eat(&TokenKind::RParen) {
                    loop {
                        let arg_span = self.peek().span;
                        if self.at(&TokenKind::A) {
                            self.next();
                            self.expect(&TokenKind::Comma, "',' after 'A' marker in aout()")?;
                            let expr = self.parse_expr()?;
                            args.push(AoutArg::Evaluated(expr));
                        } else if let TokenKind::Str(s) = self.peek().kind.clone() {
                            self.next();
                            args.push(AoutArg::Literal(s, arg_span));
                        } else {
                            return Err(parse_error(
                                "aout() arguments must be string literals or 'A' followed by an expression, e.g. aout(\"value: \", A, x)",
                                arg_span,
                            ));
                        }
                        if self.eat(&TokenKind::Comma) {
                            continue;
                        }
                        self.expect(&TokenKind::RParen, "')' to close aout()")?;
                        break;
                    }
                }
                Ok(Stmt::new(StmtKind::Aout { args }, span))
            }
            TokenKind::When => self.parse_when(),
            TokenKind::Loop => self.parse_loop(),
            TokenKind::Each => self.parse_each(),
            TokenKind::Return => {
                self.next();
                // `return` alone ends with the line (or the enclosing brace).
                if self.at(&TokenKind::RBrace) || self.at_line_break() {
                    Ok(Stmt::new(StmtKind::Return { expr: None }, span))
                } else {
                    let expr = self.parse_expr()?;
                    Ok(Stmt::new(StmtKind::Return { expr: Some(expr) }, span))
                }
            }
            TokenKind::Fail => {
                self.next();
                let expr = self.parse_expr()?;
                Ok(Stmt::new(StmtKind::Fail { expr }, span))
            }
            _ => {
                let expr = self.parse_expr()?;
                if self.eat(&TokenKind::Eq) {
                    let value = self.parse_expr()?;
                    if let ExprKind::Var(name) = &expr.kind {
                        return Ok(Stmt::new(
                            StmtKind::Assign {
                                name: name.clone(),
                                expr: value,
                            },
                            span,
                        ));
                    }
                    return Ok(Stmt::new(
                        StmtKind::AssignTo {
                            target: expr,
                            expr: value,
                        },
                        span,
                    ));
                }
                Ok(Stmt::new(StmtKind::ExprStmt { expr }, span))
            }
        }
    }

    fn parse_when(&mut self) -> CompileResult<Stmt> {
        let span = self.expect(&TokenKind::When, "'when'")?.span;
        let mut branches = Vec::new();
        let cond = self.parse_expr_until_block()?;
        let body = self.parse_block_body()?;
        branches.push((cond, body));
        while self.at(&TokenKind::Else) {
            self.next();
            let cond = self.parse_expr_until_block()?;
            let body = self.parse_block_body()?;
            branches.push((cond, body));
        }
        let else_body = if self.eat(&TokenKind::Anyelse) {
            Some(self.parse_block_body()?)
        } else {
            None
        };
        Ok(Stmt::new(
            StmtKind::When {
                branches,
                else_body,
            },
            span,
        ))
    }

    fn parse_loop(&mut self) -> CompileResult<Stmt> {
        let span = self.expect(&TokenKind::Loop, "'loop'")?.span;
        self.expect(&TokenKind::LBrace, "'{' to open the loop body")?;
        if self.at(&TokenKind::While) {
            let while_span = self.next().span;
            let cond = self.parse_expr_until_block()?;
            self.expect(&TokenKind::LBrace, "'{' to open the while body")?;
            let mut body = Vec::new();
            let mut cleanup = Vec::new();
            while !self.at(&TokenKind::Then) && !self.at(&TokenKind::RBrace) {
                if matches!(self.peek().kind, TokenKind::Eof) {
                    return Err(parse_error(
                        "unexpected end of file in loop",
                        self.peek().span,
                    ));
                }
                body.push(self.parse_stmt()?);
            }
            if self.eat(&TokenKind::Then) {
                while !self.at(&TokenKind::RBrace) {
                    if matches!(self.peek().kind, TokenKind::Eof) {
                        return Err(parse_error(
                            "unexpected end of file in loop",
                            self.peek().span,
                        ));
                    }
                    cleanup.push(self.parse_stmt()?);
                }
            }
            self.expect(&TokenKind::RBrace, "'}' to close the while body")?;
            self.expect(&TokenKind::RBrace, "'}' to close the loop")?;
            let _ = while_span;
            Ok(Stmt::new(
                StmtKind::Loop {
                    body: vec![Stmt::new(
                        StmtKind::While {
                            cond,
                            body,
                            cleanup,
                        },
                        span,
                    )],
                    count: None,
                },
                span,
            ))
        } else {
            let mut body = Vec::new();
            while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Comma) {
                if matches!(self.peek().kind, TokenKind::Eof) {
                    return Err(parse_error(
                        "unexpected end of file in loop",
                        self.peek().span,
                    ));
                }
                body.push(self.parse_stmt()?);
            }
            let count = if self.eat(&TokenKind::Comma) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect(&TokenKind::RBrace, "'}' to close the loop")?;
            Ok(Stmt::new(StmtKind::Loop { body, count }, span))
        }
    }

    fn parse_each(&mut self) -> CompileResult<Stmt> {
        let span = self.expect(&TokenKind::Each, "'each'")?.span;
        let (var, _) = self.expect_ident("iteration variable after 'each'")?;
        if !is_snake_case(&var) {
            return Err(parse_error(
                format!("iteration variable '{}' must be snake_case", var),
                span,
            ));
        }
        self.expect(&TokenKind::In, "'in' after the iteration variable")?;
        let iterable = self.parse_expr_until_block()?;
        let body = self.parse_block_body()?;
        Ok(Stmt::new(
            StmtKind::Each {
                var,
                iterable,
                body,
            },
            span,
        ))
    }

    // -----------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------

    /// Parse a condition that is immediately followed by a body: a struct
    /// literal cannot start here (mirrors Rust's restriction).
    fn parse_expr_until_block(&mut self) -> CompileResult<Expr> {
        self.parse_expr_inner(false)
    }

    pub fn parse_expr(&mut self) -> CompileResult<Expr> {
        self.parse_expr_inner(true)
    }

    fn parse_expr_inner(&mut self, allow_struct_lit: bool) -> CompileResult<Expr> {
        let mut expr = self.parse_and(allow_struct_lit)?;
        while self.at(&TokenKind::Question) && !self.at_line_break() {
            let span = self.next().span;
            expr = Expr::new(ExprKind::Try(Box::new(expr)), span);
        }
        Ok(expr)
    }

    fn parse_and(&mut self, allow: bool) -> CompileResult<Expr> {
        let mut left = self.parse_cmp(allow)?;
        while self.at(&TokenKind::And) && !self.at_line_break() {
            let span = self.next().span;
            let right = self.parse_cmp(allow)?;
            left = Expr::new(
                ExprKind::Binary(BinOp::And, Box::new(left), Box::new(right)),
                span,
            );
        }
        if self.at(&TokenKind::Or) {
            return Err(parse_error(
                "'or' is not part of -A Core; use nested when/else conditions instead",
                self.peek().span,
            )
            .with_hint("combine conditions with 'and', or split them into a when / else chain"));
        }
        Ok(left)
    }

    fn parse_cmp(&mut self, allow: bool) -> CompileResult<Expr> {
        let mut left = self.parse_add(allow)?;
        loop {
            if self.at_line_break() {
                break;
            }
            let op = match self.peek().kind {
                TokenKind::Lt => BinOp::Lt,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::Le => BinOp::Le,
                TokenKind::Ge => BinOp::Ge,
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::NotEq => BinOp::Ne,
                _ => break,
            };
            let span = self.next().span;
            let right = self.parse_add(allow)?;
            left = Expr::new(ExprKind::Binary(op, Box::new(left), Box::new(right)), span);
        }
        Ok(left)
    }

    fn parse_add(&mut self, allow: bool) -> CompileResult<Expr> {
        let mut left = self.parse_mul(allow)?;
        loop {
            if self.at_line_break() {
                break;
            }
            let op = match self.peek().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            let span = self.next().span;
            let right = self.parse_mul(allow)?;
            left = Expr::new(ExprKind::Binary(op, Box::new(left), Box::new(right)), span);
        }
        Ok(left)
    }

    fn parse_mul(&mut self, allow: bool) -> CompileResult<Expr> {
        let mut left = self.parse_unary(allow)?;
        loop {
            if self.at_line_break() {
                break;
            }
            let op = match self.peek().kind {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                _ => break,
            };
            let span = self.next().span;
            let right = self.parse_unary(allow)?;
            left = Expr::new(ExprKind::Binary(op, Box::new(left), Box::new(right)), span);
        }
        Ok(left)
    }

    fn parse_unary(&mut self, allow: bool) -> CompileResult<Expr> {
        let span = self.peek().span;
        if self.eat(&TokenKind::Minus) {
            let inner = self.parse_unary(allow)?;
            return Ok(Expr::new(ExprKind::UnaryMinus(Box::new(inner)), span));
        }
        if self.eat(&TokenKind::Amp) {
            let inner = self.parse_unary(allow)?;
            return Ok(Expr::new(ExprKind::AddressOf(Box::new(inner)), span));
        }
        if self.eat(&TokenKind::Star) {
            let inner = self.parse_unary(allow)?;
            return Ok(Expr::new(ExprKind::Deref(Box::new(inner)), span));
        }
        self.parse_postfix(allow)
    }

    fn parse_postfix(&mut self, allow: bool) -> CompileResult<Expr> {
        let mut expr = self.parse_primary(allow)?;
        loop {
            if self.at_line_break() {
                break;
            }
            match self.peek().kind {
                TokenKind::LBracket => {
                    let span = self.next().span;
                    let index = self.parse_expr()?;
                    self.expect(&TokenKind::RBracket, "']' after the index")?;
                    expr = Expr::new(ExprKind::Index(Box::new(expr), Box::new(index)), span);
                }
                TokenKind::Dot => {
                    let span = self.next().span;
                    let (name, _) = self.expect_ident("field or method name after '.'")?;
                    if self.at(&TokenKind::LParen) {
                        self.next();
                        let mut args = Vec::new();
                        if !self.eat(&TokenKind::RParen) {
                            loop {
                                args.push(self.parse_expr()?);
                                if self.eat(&TokenKind::Comma) {
                                    continue;
                                }
                                self.expect(&TokenKind::RParen, "')' to close the arguments")?;
                                break;
                            }
                        }
                        expr = Expr::new(ExprKind::MethodCall(Box::new(expr), name, args), span);
                    } else {
                        expr = Expr::new(ExprKind::Field(Box::new(expr), name), span);
                    }
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    /// Recognize `Name { field = value, ... }` (optionally qualified) without
    /// committing the parser: scan ahead for IDENT (`.` IDENT)* `{`.
    fn struct_lit_ahead(&self) -> Option<usize> {
        let mut i = self.pos;
        if !matches!(self.tokens[i].kind, TokenKind::Ident(_)) {
            return None;
        }
        i += 1;
        while matches!(self.tokens[i].kind, TokenKind::Dot)
            && matches!(self.tokens[i + 1].kind, TokenKind::Ident(_))
        {
            i += 2;
        }
        if matches!(self.tokens[i].kind, TokenKind::LBrace) {
            Some(i)
        } else {
            None
        }
    }

    fn parse_struct_lit(&mut self) -> CompileResult<Expr> {
        let span = self.peek().span;
        let mut segments = Vec::new();
        let (first, _) = self.expect_ident("struct name")?;
        segments.push(first);
        while self.at(&TokenKind::Dot) {
            self.next();
            let (seg, _) = self.expect_ident("path segment")?;
            segments.push(seg);
        }
        self.expect(&TokenKind::LBrace, "'{' to open the struct literal")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let (name, _) = self.expect_ident("field name")?;
            self.expect(&TokenKind::Eq, "'=' after the field name")?;
            let value = self.parse_expr()?;
            fields.push((name, value));
            // Fields may be separated by a comma or by a line break, so a
            // multi-line literal reads like the struct declaration.
            if !self.eat(&TokenKind::Comma) && !self.at_line_break() {
                break;
            }
        }
        self.expect(&TokenKind::RBrace, "'}' to close the struct literal")?;
        Ok(Expr::new(
            ExprKind::StructLit(segments.join("."), fields),
            span,
        ))
    }

    fn parse_primary(&mut self, allow: bool) -> CompileResult<Expr> {
        let span = self.peek().span;
        match self.peek().kind.clone() {
            TokenKind::Int(v) => {
                self.next();
                Ok(Expr::new(ExprKind::Int(v), span))
            }
            TokenKind::Double(v) => {
                self.next();
                Ok(Expr::new(ExprKind::Double(v), span))
            }
            TokenKind::Str(s) => {
                self.next();
                Ok(Expr::new(ExprKind::Str(s), span))
            }
            TokenKind::On => {
                self.next();
                Ok(Expr::new(ExprKind::Bool(true), span))
            }
            TokenKind::Off => {
                self.next();
                Ok(Expr::new(ExprKind::Bool(false), span))
            }
            TokenKind::SelfKw => {
                self.next();
                Ok(Expr::new(ExprKind::Var("self".to_string()), span))
            }
            TokenKind::A => Err(parse_error(
                "the 'A' execution marker is only valid inside aout()",
                span,
            )),
            TokenKind::At => {
                self.next();
                let (name, _) = self.expect_ident("intrinsic name after '@'")?;
                self.expect(&TokenKind::LParen, "'(' after the intrinsic name")?;
                let mut args = Vec::new();
                if !self.eat(&TokenKind::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if self.eat(&TokenKind::Comma) {
                            continue;
                        }
                        self.expect(&TokenKind::RParen, "')' to close the intrinsic call")?;
                        break;
                    }
                }
                Ok(Expr::new(ExprKind::Intrinsic(name, args), span))
            }
            TokenKind::LParen => {
                self.next();
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "')' to close the expression")?;
                Ok(expr)
            }
            TokenKind::LBracket => {
                self.next();
                let mut elems = Vec::new();
                if !self.eat(&TokenKind::RBracket) {
                    loop {
                        elems.push(self.parse_expr()?);
                        if self.eat(&TokenKind::Comma) {
                            if self.at(&TokenKind::RBracket) {
                                break;
                            }
                            continue;
                        }
                        break;
                    }
                    self.expect(&TokenKind::RBracket, "']' to close the array literal")?;
                }
                Ok(Expr::new(ExprKind::ArrayLit(elems), span))
            }
            TokenKind::Ident(name) => {
                if allow {
                    if let Some(_brace_pos) = self.struct_lit_ahead() {
                        return self.parse_struct_lit();
                    }
                }
                self.next();
                if self.at(&TokenKind::LParen) {
                    self.next();
                    let mut args = Vec::new();
                    if !self.eat(&TokenKind::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.eat(&TokenKind::Comma) {
                                continue;
                            }
                            self.expect(&TokenKind::RParen, "')' to close the arguments")?;
                            break;
                        }
                    }
                    return Ok(Expr::new(ExprKind::Call(name, args), span));
                }
                Ok(Expr::new(ExprKind::Var(name), span))
            }
            _ => Err(parse_error(
                format!("expected an expression, found '{}'", self.describe_peek()),
                span,
            )),
        }
    }
}

/// Parse a token stream into a [`Program`].
pub fn parse(tokens: Vec<Token>) -> CompileResult<Program> {
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

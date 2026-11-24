//! -A Core Edition compiler library.
//!
//! Pipeline: module loading -> lex -> parse -> semantic analysis (names, types,
//! modules, queues, reuse instantiation) -> optimization -> assembly code generation.
//! See `docs/ARCHITECTURE.md`.

pub mod ast;
pub mod cli;
pub mod codegen;
pub mod diagnostics;
pub mod lexer;
pub mod modules;
pub mod optimizer;
pub mod parser;
pub mod runtime;
pub mod semantic;
pub mod types;

use diagnostics::{CompileResult, Diagnostic};
use std::path::Path;

/// Compilation options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Run the optimizer before code generation. Turning it off is what the
    /// differential tests use to prove optimization preserves behaviour.
    pub optimize: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { optimize: true }
    }
}

/// Compile -A source text into x86-64 assembly text.
///
/// Imports in the source are resolved relative to the current directory.
pub fn compile_source(source: &str) -> CompileResult<String> {
    compile_source_opts(source, Options::default())
}

/// Compile -A source text with explicit options.
pub fn compile_source_opts(source: &str, opts: Options) -> CompileResult<String> {
    let loaded = modules::load_source(source)?;
    compile_modules(loaded, opts)
}

/// Compile a -A file, resolving its imports relative to the file's directory.
pub fn compile_file(path: &Path) -> CompileResult<String> {
    compile_file_opts(path, Options::default())
}

/// Compile a -A file with explicit options.
pub fn compile_file_opts(path: &Path, opts: Options) -> CompileResult<String> {
    let loaded = modules::load(path)?;
    compile_modules(loaded, opts)
}

/// Type-check -A source text without generating code.
pub fn check_source(source: &str) -> CompileResult<()> {
    let loaded = modules::load_source(source)?;
    check_modules(loaded)
}

/// Type-check a -A file without generating code.
pub fn check_file(path: &Path) -> CompileResult<()> {
    let loaded = modules::load(path)?;
    check_modules(loaded)
}

fn to_programs(loaded: Vec<modules::LoadedModule>) -> Vec<ast::Program> {
    loaded.into_iter().map(|m| m.program).collect()
}

fn check_modules(loaded: Vec<modules::LoadedModule>) -> CompileResult<()> {
    let programs = to_programs(loaded);
    let mut checker = semantic::Checker::new();
    checker.check(programs)?;
    Ok(())
}

fn compile_modules(loaded: Vec<modules::LoadedModule>, opts: Options) -> CompileResult<String> {
    let programs = to_programs(loaded);
    let mut checker = semantic::Checker::new();
    let mut checked = checker.check(programs)?;
    if opts.optimize {
        // The optimizer rewrites the very trees code generation reads (the
        // schedule and the monomorphized instances), so what runs is what was
        // optimized.
        optimizer::optimize(&mut checked);
    }
    Ok(codegen::generate(&checked))
}

/// Attach a file name to a diagnostic so the CLI can point at the source.
pub fn with_file(diag: Diagnostic, file: &str) -> Diagnostic {
    diag.with_file(file)
}

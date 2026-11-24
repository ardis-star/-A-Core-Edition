//! Module loading and resolution.
//!
//! A module is a source file. `import utils.math` loads `utils/math.ma` relative
//! to the importing file and exposes its declarations under the `utils.math`
//! prefix. A file may declare its own module path explicitly with
//! `module utils.math`; otherwise the path is derived from the import that
//! reached it.
//!
//! Resolution rules, documented in `docs/MODULES.md`:
//!
//! * imports are resolved relative to the directory of the importing file;
//! * each module is loaded once, no matter how many files import it;
//! * import cycles are rejected with the cycle spelled out;
//! * only `pub` declarations of an imported module are visible;
//! * names are used qualified (`utils.math.area`), so imports can never clash.

use crate::ast::Program;
use crate::diagnostics::{semantic_error, CompileResult, Span};
use crate::lexer::lex;
use crate::parser::parse;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A loaded module and where it came from.
#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub path: PathBuf,
    pub program: Program,
}

/// Load a root file and everything it imports.
///
/// The returned vector starts with the root module and continues with its
/// dependencies in depth-first import order.
pub fn load(root: &Path) -> CompileResult<Vec<LoadedModule>> {
    let mut loader = Loader {
        modules: Vec::new(),
        seen: HashSet::new(),
        stack: Vec::new(),
    };
    loader.load_file(root, &[])?;
    Ok(loader.finish())
}

/// Load -A source text that is not backed by a file, for tests and for
/// `--source` style entry points.
pub fn load_source(source: &str) -> CompileResult<Vec<LoadedModule>> {
    let mut loader = Loader {
        modules: Vec::new(),
        seen: HashSet::new(),
        stack: Vec::new(),
    };
    loader.parse_program(source, Path::new("main.ma"), &[], true)?;
    Ok(loader.finish())
}

/// Loaded modules, with the root program first and its imports after it.
///
/// Imports are discovered depth first, so the root is recorded last; the
/// semantic analyser treats the first program as the entry point, so the order
/// is normalised here instead of leaking load order into the checker.
fn finish_modules(modules: &mut Vec<LoadedModule>) {
    if let Some(root) = modules.pop() {
        modules.insert(0, root);
    }
}

struct Loader {
    modules: Vec<LoadedModule>,
    /// Canonical module paths already loaded.
    seen: HashSet<String>,
    /// Import stack, for cycle reporting.
    stack: Vec<String>,
}

impl Loader {
    fn finish(&mut self) -> Vec<LoadedModule> {
        finish_modules(&mut self.modules);
        std::mem::take(&mut self.modules)
    }

    fn load_file(&mut self, path: &Path, module_path: &[String]) -> CompileResult<()> {
        let key = if module_path.is_empty() {
            path.to_string_lossy().to_string()
        } else {
            module_path.join(".")
        };
        if self.stack.contains(&key) {
            let mut cycle = self.stack.clone();
            cycle.push(key.clone());
            return Err(semantic_error(
                format!(
                    "cyclic import detected: {}. -A Core does not support import cycles",
                    cycle.join(" -> ")
                ),
                Span::dummy(),
            ));
        }
        if self.seen.contains(&key) {
            return Ok(());
        }
        self.seen.insert(key.clone());

        let source = std::fs::read_to_string(path).map_err(|e| {
            semantic_error(
                format!("cannot read module '{}': {}", path.display(), e),
                Span::dummy(),
            )
        })?;
        self.stack.push(key);
        self.parse_program(&source, path, module_path, false)?;
        self.stack.pop();
        Ok(())
    }

    fn parse_program(
        &mut self,
        source: &str,
        path: &Path,
        module_path: &[String],
        is_root: bool,
    ) -> CompileResult<()> {
        let tokens = lex(source)?;
        let mut program = parse(tokens)?;
        // An explicit `module` header must agree with the path it is imported
        // through, otherwise the two names would silently disagree.
        if program.module_path.is_empty() {
            program.module_path = module_path.to_vec();
        } else if !module_path.is_empty() && program.module_path != module_path {
            return Err(semantic_error(
                format!(
                    "module '{}' is imported as '{}'; a module header must match the import path",
                    program.module_path.join("."),
                    module_path.join(".")
                ),
                Span::dummy(),
            ));
        }
        let effective: Vec<String> = program.module_path.clone();
        let _ = is_root;

        // Load imports first (depth-first), then record this module.
        let base_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let imports = program.imports.clone();
        for import in imports {
            let mut candidate = base_dir.clone();
            for segment in &import.path[..import.path.len() - 1] {
                candidate.push(segment);
            }
            let file = format!("{}.ma", import.path.last().unwrap());
            candidate.push(file);
            // The module name of an imported file is the path written in the
            // import statement, resolved relative to this file's directory; it
            // is not nested under the importer's own name.
            let target_path = import.path.clone();
            let _ = &effective;
            self.load_file(&candidate, &target_path)?;
        }

        // The root program keeps its (possibly empty) path; imports carry the
        // prefix they were reached through.
        self.modules.push(LoadedModule {
            path: path.to_path_buf(),
            program,
        });
        Ok(())
    }
}

//! `-a create <name>`: write a starter -A program.
//!
//! The template is a complete, valid -A Core program: `-a run` works on it
//! immediately, and the comments point at the two commands a newcomer needs.

use super::{PROG, SOURCE_EXT};
use crate::diagnostics::{Diagnostic, Span};
use std::path::{Path, PathBuf};

/// Turn a user-supplied name into a `.ma` path.
///
/// `hello.ma` stays as it is, `game` becomes `game.ma`, and anything with a
/// different extension is rejected instead of silently rewritten.
pub fn source_path_for(name: &str) -> Result<PathBuf, Diagnostic> {
    if name.is_empty() {
        return Err(Diagnostic::new(
            "E_USAGE",
            "create needs a file name",
            Span::dummy(),
        ));
    }
    let path = Path::new(name);
    // Only a trailing component extension counts: `dir.with.dot/game` is fine.
    let stem = path.file_name().map(|n| n.to_string_lossy().to_string());
    let extension = path.extension().and_then(|e| e.to_str());
    match (stem, extension) {
        (None, _) => Err(Diagnostic::new(
            "E_USAGE",
            format!("'{}' is not a usable file name", name),
            Span::dummy(),
        )),
        (Some(_), Some(SOURCE_EXT)) => Ok(path.to_path_buf()),
        (Some(_), Some(other)) => {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| name.to_string());
            Err(Diagnostic::new(
                "E_USAGE",
                format!(
                    "'.{}' is not a -A source extension; -A sources use .{}",
                    other, SOURCE_EXT
                ),
                Span::dummy(),
            )
            .with_hint(format!(
                "write `-a create {}.{}` to create that file with the official extension",
                stem, SOURCE_EXT
            )))
        }
        (Some(_), None) => {
            // No extension at all: add the official one (`game` -> `game.ma`).
            let mut with_ext = path.to_path_buf().into_os_string();
            with_ext.push(".");
            with_ext.push(SOURCE_EXT);
            Ok(PathBuf::from(with_ext))
        }
    }
}

/// The starter program. Keep it minimal, valid and obviously runnable.
pub fn template(file: &Path) -> String {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("program.{}", SOURCE_EXT));
    format!(
        "// {name}\n\
         //\n\
         // Run it with:  -a run {name}\n\
         // Build it to:  -a build {name}\n\
         \n\
         block main(1) {{\n\
         \x20   aout(\"Hello, world!\")\n\
         }}\n",
        name = name
    )
}

/// `-a create`
pub fn create(name: &str, force: bool) -> Result<u8, Diagnostic> {
    let path = source_path_for(name)?;
    if path.exists() && !force {
        return Err(Diagnostic::new(
            "E_IO",
            format!("'{}' already exists; nothing was written", path.display()),
            Span::dummy(),
        )
        .with_file(path.display().to_string())
        .with_hint("pass --force to overwrite it"));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.is_dir() {
            return Err(Diagnostic::new(
                "E_IO",
                format!(
                    "cannot create '{}': directory '{}' does not exist",
                    path.display(),
                    parent.display()
                ),
                Span::dummy(),
            )
            .with_hint("create the directory first, or write the file in the current directory"));
        }
    }
    std::fs::write(&path, template(&path)).map_err(|e| {
        Diagnostic::new(
            "E_IO",
            format!("cannot write '{}': {}", path.display(), e),
            Span::dummy(),
        )
        .with_file(path.display().to_string())
    })?;
    println!("{}: created {}", PROG, path.display());
    println!("  next: -a run {}", path.display());
    Ok(0)
}

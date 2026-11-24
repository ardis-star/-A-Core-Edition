//! The -A Core Edition assembly backend: assemble and link the x86-64 assembly
//! that the compiler generates.
//!
//! The compiler targets x86-64 Intel-syntax assembly (see `docs/BACKEND.md`).
//! This crate is the backend's driver: it validates its inputs, invokes the
//! system assembler/linker, and propagates failures instead of hiding them. It
//! contains no language-specific knowledge — -A ends at the assembly text.

use std::env;
use std::path::Path;
use std::process::Command;

/// The C compiler/linker program used to assemble assembly files.
pub fn cc_program() -> String {
    env::var("CC").unwrap_or_else(|_| "cc".to_string())
}

/// Assemble and link a x86-64 assembly file into an executable.
pub fn assemble(input: &Path, output: &Path) -> Result<(), String> {
    if !input.exists() {
        return Err(format!("input file '{}' does not exist", input.display()));
    }
    let cc = cc_program();
    let status = Command::new(&cc)
        .args(["-x", "assembler", "-pthread"])
        .arg(input)
        .args(["-o"])
        .arg(output)
        .status()
        .map_err(|e| {
            format!(
                "failed to run assembler/linker '{}': {}. Is a C compiler installed?",
                cc, e
            )
        })?;
    if !status.success() {
        return Err(format!(
            "assembler/linker failed for '{}' (see compiler output above)",
            input.display()
        ));
    }
    Ok(())
}

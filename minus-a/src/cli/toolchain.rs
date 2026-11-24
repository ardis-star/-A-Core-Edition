//! Assembly, linking and execution.
//!
//! `build` and `run` share this code: the compiler produces assembly text, the
//! backend assembles and links it, and `run` executes the result in a scratch
//! directory that is removed afterwards. Nothing here writes into the source
//! directory unless the user asked for it.

use super::{attach_file, validate_source, PROG};
use crate::diagnostics::{Diagnostic, Span};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Assemble and link `asm_path` into `exe`, reporting failures as diagnostics.
pub fn assemble_and_link(asm_path: &Path, exe: &Path) -> Result<(), Diagnostic> {
    minus_a_backend::assemble(asm_path, exe).map_err(|e| {
        Diagnostic::new(
            "E_TOOLCHAIN",
            format!("could not assemble/link '{}': {}", exe.display(), e),
            Span::dummy(),
        )
        .with_file(asm_path.display().to_string())
        .with_hint("the backend needs a C toolchain (`cc`) on PATH; run `-a doctor` to check")
    })
}

/// A scratch directory that removes itself, even when a later step fails.
pub struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    pub fn new(tag: &str) -> Result<Scratch, Diagnostic> {
        let base = std::env::temp_dir();
        let pid = std::process::id();
        for attempt in 0..1000u32 {
            let dir = base.join(format!("a-core-{}-{}-{}", tag, pid, attempt));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(Scratch { dir }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(Diagnostic::new(
                        "E_IO",
                        format!(
                            "cannot create a scratch directory in '{}': {}",
                            base.display(),
                            e
                        ),
                        Span::dummy(),
                    )
                    .with_hint("set TMPDIR to a writable directory"))
                }
            }
        }
        Err(Diagnostic::new(
            "E_IO",
            format!(
                "cannot create a scratch directory in '{}': too many stale ones",
                base.display()
            ),
            Span::dummy(),
        ))
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `-a run`: compile, assemble, link, execute, propagate the exit status.
///
/// The assembly and the executable live in a scratch directory and are deleted
/// before the command returns, so running a program never litters its source
/// directory.
pub fn run_program(input: &Path, program_args: &[String]) -> Result<u8, Diagnostic> {
    validate_source(input)?;
    std::fs::read_to_string(input).map_err(|e| {
        Diagnostic::new(
            "E_IO",
            format!("cannot read '{}': {}", input.display(), e),
            Span::dummy(),
        )
        .with_file(input.display().to_string())
    })?;

    // Compile first: a rejected program must not create anything, not even a
    // scratch directory.
    let assembly = crate::compile_file(input).map_err(|d| attach_file(d, input))?;

    let scratch = Scratch::new("run")?;
    let asm_path = scratch.path("program.s");
    let exe_path = scratch.path("program");
    std::fs::write(&asm_path, &assembly).map_err(|e| {
        Diagnostic::new(
            "E_IO",
            format!("cannot write '{}': {}", asm_path.display(), e),
            Span::dummy(),
        )
    })?;
    assemble_and_link(&asm_path, &exe_path)?;

    let status = Command::new(&exe_path)
        .args(program_args)
        .status()
        .map_err(|e| {
            Diagnostic::new(
                "E_TOOLCHAIN",
                format!("cannot execute '{}': {}", exe_path.display(), e),
                Span::dummy(),
            )
            .with_hint("the executable was built but could not be started")
        })?;

    // The child inherits this process's stdout/stderr/stdin, so output ordering
    // and interactivity are exactly those of a directly started program.
    if let Some(code) = status.code() {
        Ok(code.clamp(0, 255) as u8)
    } else {
        // Killed by a signal: report the conventional 128 + signo.
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(signal) = status.signal() {
                let code = 128 + signal;
                eprintln!(
                    "{}: program terminated by signal {} ({})",
                    PROG, signal, code
                );
                return Ok(code.clamp(0, 255) as u8);
            }
        }
        Ok(1)
    }
}

//! Shared helpers for the -A Core integration tests.
//!
//! Every test compiles -A source with the real compiler library and, when a
//! system assembler/linker is available, assembles the generated x86-64 assembly
//! and executes the resulting program. Nothing here stubs out a stage: if the
//! backend is unavailable the test reports that it was skipped instead of
//! pretending the program ran.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A scratch directory that is removed when the guard is dropped.
pub struct Scratch {
    pub dir: PathBuf,
}

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("minus_a_test_{}_{}_{}", std::process::id(), n, tag));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        Scratch { dir }
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

/// Compile -A source to x86-64 assembly text, returning the assembly on success.
pub fn compile(source: &str) -> Result<String, String> {
    minus_a::compile_source(source).map_err(|d| d.with_file("main.ma").to_string())
}

/// Compile and require success, returning the x86-64 assembly.
pub fn compile_ok(source: &str) -> String {
    match compile(source) {
        Ok(asm) => asm,
        Err(rendered) => panic!("expected successful compilation, got:\n{}", rendered),
    }
}

/// Compile and require failure, returning the rendered diagnostics.
pub fn compile_err(source: &str) -> String {
    match compile(source) {
        Ok(_) => panic!("expected compilation to fail, but it succeeded"),
        Err(rendered) => rendered,
    }
}

/// True when a C toolchain capable of assembling -A assembly is present.
pub fn have_toolchain() -> bool {
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    Command::new(cc)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Result of running a compiled -A program.
pub struct Run {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Compile -A source, assemble it with the assembly backend and execute it.
///
/// Returns `None` when no assembler/linker is installed on the host, so callers
/// can skip the run rather than assert on output that was never produced.
pub fn run_program(tag: &str, source: &str) -> Option<Run> {
    if !have_toolchain() {
        return None;
    }
    let asm = compile_ok(source);
    let scratch = Scratch::new(tag);
    let asm_path = scratch.path("program.s");
    let exe_path = scratch.path("program");
    std::fs::write(&asm_path, &asm).expect("write assembly");

    minus_a_backend::assemble(&asm_path, &exe_path).unwrap_or_else(|e| {
        panic!(
            "the generated x86-64 assembly failed to assemble:\n{}\n--- assembly ---\n{}",
            e, asm
        )
    });

    let output = Command::new(&exe_path)
        .output()
        .expect("execute compiled program");
    Some(Run {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

/// Compile, assemble and run, reporting how long the *program* took.
pub fn run_program_timed(tag: &str, source: &str) -> Option<(Run, std::time::Duration)> {
    if !have_toolchain() {
        return None;
    }
    let asm = compile_ok(source);
    let scratch = Scratch::new(tag);
    let asm_path = scratch.path("program.s");
    let exe_path = scratch.path("program");
    std::fs::write(&asm_path, &asm).expect("write assembly");
    minus_a_backend::assemble(&asm_path, &exe_path)
        .unwrap_or_else(|e| panic!("the generated x86-64 assembly failed to assemble:\n{}", e));
    let started = std::time::Instant::now();
    let output = Command::new(&exe_path)
        .output()
        .expect("execute compiled program");
    let elapsed = started.elapsed();
    Some((
        Run {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        },
        elapsed,
    ))
}

/// Compile -A source with explicit options, assemble and run it.
pub fn run_program_opts(tag: &str, source: &str, opts: minus_a::Options) -> Option<Run> {
    if !have_toolchain() {
        return None;
    }
    let asm = minus_a::compile_source_opts(source, opts).unwrap_or_else(|d| {
        panic!(
            "expected successful compilation, got:\n{}",
            d.with_file("main.ma")
        )
    });
    let scratch = Scratch::new(tag);
    let asm_path = scratch.path("program.s");
    let exe_path = scratch.path("program");
    std::fs::write(&asm_path, &asm).expect("write assembly");
    minus_a_backend::assemble(&asm_path, &exe_path).unwrap_or_else(|e| {
        panic!(
            "the generated x86-64 assembly failed to assemble:\n{}\n--- assembly ---\n{}",
            e, asm
        )
    });
    let output = Command::new(&exe_path)
        .output()
        .expect("execute compiled program");
    Some(Run {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

/// Compile a multi-file -A project held in memory: `files` maps a file name to
/// its source, and `main_name` is the entry file. Imports resolve inside the
/// scratch directory, exactly as they do on disk.
pub fn run_files(tag: &str, files: &[(&str, &str)], main_name: &str) -> Option<Run> {
    if !have_toolchain() {
        return None;
    }
    let scratch = Scratch::new(tag);
    for (name, source) in files {
        std::fs::write(scratch.path(name), source).expect("write source file");
    }
    let asm = minus_a::compile_file(&scratch.path(main_name)).unwrap_or_else(|d| {
        panic!(
            "expected successful compilation of '{}', got:\n{}",
            main_name,
            d.with_file(main_name)
        )
    });
    let asm_path = scratch.path("program.s");
    let exe_path = scratch.path("program");
    std::fs::write(&asm_path, &asm).expect("write assembly");
    minus_a_backend::assemble(&asm_path, &exe_path)
        .unwrap_or_else(|e| panic!("the generated x86-64 assembly failed to assemble:\n{}", e));
    let output = Command::new(&exe_path)
        .output()
        .expect("execute compiled program");
    Some(Run {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

/// Check a multi-file project and return the rendered diagnostics.
pub fn check_files(tag: &str, files: &[(&str, &str)], main_name: &str) -> String {
    let scratch = Scratch::new(tag);
    for (name, source) in files {
        std::fs::write(scratch.path(name), source).expect("write source file");
    }
    match minus_a::check_file(&scratch.path(main_name)) {
        Ok(()) => String::new(),
        Err(d) => d.with_file(main_name).to_string(),
    }
}

/// Assert the program prints exactly `expected`.
pub fn assert_stdout(tag: &str, source: &str, expected: &str) {
    match run_program(tag, source) {
        Some(run) => {
            assert_eq!(
                run.stdout, expected,
                "unexpected program output (exit code {}, stderr: {})",
                run.status, run.stderr
            );
            assert_eq!(run.status, 0, "program exited with {}", run.status);
        }
        None => eprintln!("skipping '{}': no assembler/linker available", tag),
    }
}

/// Read a file from the workspace `examples/` directory.
pub fn example_source(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("examples")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read example '{}': {}", path.display(), e))
}

/// Serializes the timing-sensitive parallel tests.
///
/// Cargo runs test functions on several threads; two tests that both measure
/// wall-clock behaviour would otherwise starve each other's worker threads and
/// produce meaningless numbers. Each such test holds this lock for its duration.
pub static PARALLEL_TIMING_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the timing lock, recovering from a poisoned mutex.
pub fn timing_lock() -> std::sync::MutexGuard<'static, ()> {
    PARALLEL_TIMING_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

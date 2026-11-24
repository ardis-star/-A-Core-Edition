//! Tests for the `minus-a` command-line interface.
//!
//! These drive the real binary, so they cover argument handling, exit codes and
//! the diagnostics users actually see.

mod support;

use std::process::Command;

fn minus_a() -> &'static str {
    env!("CARGO_BIN_EXE_minus-a")
}

fn write(dir: &support::Scratch, name: &str, contents: &str) -> std::path::PathBuf {
    let path = dir.path(name);
    std::fs::write(&path, contents).expect("write source file");
    path
}

#[test]
fn cli_help_and_version_succeed() {
    let help = Command::new(minus_a()).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("compile"));

    let version = Command::new(minus_a()).arg("--version").output().unwrap();
    assert!(version.status.success());
}

#[test]
fn cli_compile_writes_assembly() {
    let dir = support::Scratch::new("cli_compile");
    let src = write(&dir, "hello.ma", "block hello(1) { aout(\"hi\") }\n");
    let out = dir.path("hello.s");

    let result = Command::new(minus_a())
        .args(["compile"])
        .arg(&src)
        .args(["-o"])
        .arg(&out)
        .output()
        .unwrap();

    assert!(result.status.success(), "compile failed");
    let asm = std::fs::read_to_string(&out).expect("output assembly written");
    assert!(asm.contains(".intel_syntax noprefix"));
    assert!(asm.contains(".globl main"));
}

#[test]
fn cli_accepts_ma_and_rejects_other_source_extensions() {
    if !support::have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let dir = support::Scratch::new("cli_extension");
    let good = write(
        &dir,
        "hello_world.ma",
        "block main(1) { aout(\"hello\") }\n",
    );
    let ok = Command::new(minus_a())
        .arg("check")
        .arg(&good)
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "a .ma file must be accepted: {}",
        String::from_utf8_lossy(&ok.stderr)
    );

    // `.a`, `.mp` and extensionless inputs are not -A sources.
    for (name, found) in [
        ("hello_world.a", ".a"),
        ("hello_world.mp", ".mp"),
        ("hello_world", "no extension"),
    ] {
        let path = write(&dir, name, "block main(1) { aout(\"hello\") }\n");
        let out = Command::new(minus_a())
            .arg("check")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            !out.status.success(),
            "'{}' must be rejected as a -A source file",
            name
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("is not a -A source file") && stderr.contains(found),
            "expected an extension diagnostic for '{}', got: {}",
            name,
            stderr
        );
        assert!(
            stderr.contains(".ma extension"),
            "the diagnostic must name the official extension, got: {}",
            stderr
        );
    }
}

#[test]
fn cli_identifies_itself_as_core_edition() {
    let help = Command::new(minus_a()).arg("--help").output().unwrap();
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(
        help_text.contains("-A Core Edition"),
        "help must name the edition: {}",
        help_text
    );
    assert!(
        help_text.contains("<input.ma>"),
        "usage must show the .ma extension: {}",
        help_text
    );
    assert!(
        !help_text.contains("+A"),
        "help must not mention a separate +A language: {}",
        help_text
    );

    let version = Command::new(minus_a()).arg("--version").output().unwrap();
    let version_text = String::from_utf8_lossy(&version.stdout);
    assert!(
        version_text.contains("-A Core Edition"),
        "version must name the edition: {}",
        version_text
    );

    // The check/summary line carries the product name too.
    let dir = support::Scratch::new("cli_brand");
    let src = write(&dir, "brand.ma", "block main(1) { aout(\"x\") }\n");
    let out = Command::new(minus_a())
        .arg("check")
        .arg(&src)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("-A Core Edition:"),
        "messages must be prefixed with the product name, got: {}",
        stderr
    );
}

#[test]
fn cli_compile_reports_errors_and_exits_non_zero() {
    let dir = support::Scratch::new("cli_error");
    let src = write(&dir, "bad.ma", "block main(1) { aout(A, nope) }\n");

    let result = Command::new(minus_a())
        .arg("compile")
        .arg(&src)
        .output()
        .unwrap();

    assert!(!result.status.success(), "invalid program must fail");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("E_SEMANTIC"), "stderr: {}", stderr);
    assert!(
        stderr.contains("bad.ma"),
        "diagnostic must name the file: {}",
        stderr
    );
    assert!(!dir.path("bad.s").exists(), "no output on failure");
}

#[test]
fn cli_check_stops_before_codegen() {
    let dir = support::Scratch::new("cli_check");
    let src = write(&dir, "ok.ma", "block main(1) { aout(\"ok\") }\n");

    let result = Command::new(minus_a())
        .arg("check")
        .arg(&src)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(!dir.path("ok.s").exists(), "check must not write assembly");
}

#[test]
fn cli_missing_argument_is_usage_error() {
    let result = Command::new(minus_a()).arg("compile").output().unwrap();
    assert!(!result.status.success());
}

#[test]
fn cli_unknown_command_is_usage_error() {
    let result = Command::new(minus_a()).arg("explode").output().unwrap();
    assert!(!result.status.success());
}

#[test]
fn cli_build_writes_the_sidecar_beside_the_executable() {
    if !support::have_toolchain() {
        eprintln!("skipping build test: no assembler/linker available");
        return;
    }
    let dir = support::Scratch::new("cli_sidecar");
    let src = write(&dir, "program.ma", "block main(1) { aout(\"sidecar\") }\n");
    let exe = dir.path("program");
    let result = Command::new(minus_a())
        .arg("build")
        .arg(&src)
        .args(["-o"])
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    // The intermediate assembly belongs next to the executable, not in the
    // process working directory.
    assert!(
        dir.path("program.s").exists(),
        "expected program.s beside the executable"
    );
    assert!(
        !std::path::Path::new("program.s").exists(),
        "the sidecar must not be written into the working directory"
    );
}

#[test]
fn cli_build_produces_a_running_executable() {
    if !support::have_toolchain() {
        eprintln!("skipping build test: no assembler/linker available");
        return;
    }
    let dir = support::Scratch::new("cli_build");
    let src = write(
        &dir,
        "program.ma",
        "block greet(1) { aout(\"hello from the cli\") }\n",
    );
    let exe = dir.path("program");

    let result = Command::new(minus_a())
        .arg("build")
        .arg(&src)
        .args(["-o"])
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let run = Command::new(&exe).output().expect("run built program");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "hello from the cli\n");
}

// ---------------------------------------------------------------------------
// `-a create`
// ---------------------------------------------------------------------------

/// Run a CLI command with `dir` as the working directory.
fn cli_in(dir: &support::Scratch, args: &[&str]) -> std::process::Output {
    Command::new(minus_a())
        .args(args)
        .current_dir(&dir.dir)
        .output()
        .expect("run the compiler")
}

#[test]
fn cli_create_writes_a_compilable_template() {
    if !support::have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let dir = support::Scratch::new("cli_create");
    let out = cli_in(&dir, &["create", "hello.ma"]);
    assert!(out.status.success(), "create failed: {:?}", out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hello.ma"),
        "create must print the created path, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(dir.path("hello.ma").is_file());

    // The template must be valid -A: check it, then run it.
    let check = cli_in(&dir, &["check", "hello.ma"]);
    assert!(
        check.status.success(),
        "the created template must compile: {}",
        stderr
    );
    let run = cli_in(&dir, &["run", "hello.ma"]);
    assert!(run.status.success(), "the created template must run");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "Hello, world!\n");
}

#[test]
fn cli_create_appends_the_ma_extension() {
    let dir = support::Scratch::new("cli_create_ext");
    let out = cli_in(&dir, &["create", "game"]);
    assert!(out.status.success(), "create failed: {:?}", out);
    assert!(dir.path("game.ma").is_file(), "expected game.ma");
    assert!(!dir.path("game").exists(), "no extensionless file");
}

#[test]
fn cli_create_rejects_other_extensions() {
    let dir = support::Scratch::new("cli_create_bad_ext");
    for name in ["bad.a", "bad.mp", "bad.txt"] {
        let out = cli_in(&dir, &["create", name]);
        assert_eq!(
            out.status.code(),
            Some(2),
            "'{}' must be a usage error, got: {:?}",
            name,
            out
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(".ma"),
            "the error must name the .ma extension: {}",
            stderr
        );
        assert!(!dir.path(name).exists(), "'{}' must not be created", name);
    }
}

#[test]
fn cli_create_never_overwrites_without_force() {
    let dir = support::Scratch::new("cli_create_exists");
    let path = dir.path("keep.ma");
    std::fs::write(&path, "// hand-written, do not lose me\n").unwrap();

    let out = cli_in(&dir, &["create", "keep.ma"]);
    assert_eq!(out.status.code(), Some(1), "expected refusal: {:?}", out);
    assert!(String::from_utf8_lossy(&out.stderr).contains("--force"));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "// hand-written, do not lose me\n",
        "the existing file must be untouched"
    );

    let forced = cli_in(&dir, &["create", "keep.ma", "--force"]);
    assert!(
        forced.status.success(),
        "--force must overwrite: {:?}",
        forced
    );
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("Hello, world!"));
}

// ---------------------------------------------------------------------------
// `-a run`
// ---------------------------------------------------------------------------

#[test]
fn cli_run_executes_and_leaves_no_artifacts() {
    if !support::have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let dir = support::Scratch::new("cli_run");
    write(
        &dir,
        "hello.ma",
        "block main(1) { aout(\"hello from run\") }\n",
    );
    let out = cli_in(&dir, &["run", "hello.ma"]);
    assert!(out.status.success(), "run failed: {:?}", out);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello from run\n");

    let mut entries: Vec<String> = std::fs::read_dir(&dir.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    entries.sort();
    assert_eq!(
        entries,
        vec!["hello.ma".to_string()],
        "run must not leave assembly or executables behind"
    );
}

#[test]
fn cli_run_propagates_the_program_exit_status() {
    if !support::have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let dir = support::Scratch::new("cli_run_fail");
    write(&dir, "boom.ma", "block main(1) { fail \"boom\" }\n");
    let out = cli_in(&dir, &["run", "boom.ma"]);
    assert_eq!(out.status.code(), Some(1), "a failing program exits 1");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("error: boom"),
        "the program's stderr must be forwarded: {:?}",
        out
    );
}

#[test]
fn cli_run_forwards_stdout_and_reports_runtime_errors() {
    if !support::have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let dir = support::Scratch::new("cli_run_bounds");
    write(
        &dir,
        "bounds.ma",
        "block main(1) {\n    aout(\"before\")\n    set a = [1, 2]\n    aout(A, a[5])\n}\n",
    );
    let out = cli_in(&dir, &["run", "bounds.ma"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "before\n");
    assert!(String::from_utf8_lossy(&out.stderr).contains("out of bounds"));
}

#[test]
fn cli_run_rejects_a_broken_program_without_executing_it() {
    if !support::have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let dir = support::Scratch::new("cli_run_reject");
    write(
        &dir,
        "broken.ma",
        "block main(1) {\n    set x = 1\n    x = 2\n}\n",
    );
    let out = cli_in(&dir, &["run", "broken.ma"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.stdout.len(), 0, "nothing may run: {:?}", out);
    assert!(stderr.contains("E_SEMANTIC"), "got: {}", stderr);
    assert!(stderr.contains("broken.ma"), "got: {}", stderr);
}

#[test]
fn cli_run_resolves_imports_relative_to_the_source_file() {
    if !support::have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let dir = support::Scratch::new("cli_run_modules");
    std::fs::create_dir_all(dir.path("game")).unwrap();
    std::fs::write(
        dir.path("game/math.ma"),
        "module math\n\npub reuse add(a, b) { return a + b }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path("game/main.ma"),
        "import math\n\nblock main(1) { aout(\"sum: \", A, math.add(20, 22)) }\n",
    )
    .unwrap();

    // Run it from a directory that is neither the source directory nor the
    // working directory of the import.
    let out = cli_in(&dir, &["run", "./game/main.ma"]);
    assert!(out.status.success(), "run failed: {:?}", out);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "sum: 42\n");
}

// ---------------------------------------------------------------------------
// `-a doctor`
// ---------------------------------------------------------------------------

#[test]
fn cli_doctor_reports_the_environment() {
    let out = Command::new(minus_a()).arg("doctor").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    for expected in [
        "-A Core Edition: doctor",
        "version",
        "executable",
        "PATH",
        "assembler/linker",
        "backend",
    ] {
        assert!(
            stdout.contains(expected),
            "doctor output must mention '{}':\n{}",
            expected,
            stdout
        );
    }
    // 0 when everything is in order, 1 when something needs attention; never a
    // usage error and never a crash.
    assert!(
        matches!(out.status.code(), Some(0) | Some(1)),
        "doctor exit status: {:?}",
        out.status
    );
}

#[test]
fn cli_usage_errors_exit_two() {
    for args in [
        vec!["frobnicate"],
        vec!["run"],
        vec!["compile"],
        vec!["check"],
        vec!["create"],
        vec!["build", "a.ma", "b.ma"],
        vec!["doctor", "extra"],
    ] {
        let out = Command::new(minus_a()).args(&args).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "{:?} must be a usage error, got: {:?}",
            args,
            out
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E_USAGE"), "got: {}", stderr);
    }
}

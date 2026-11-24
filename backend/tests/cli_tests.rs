//! Tests for the `minus-a-backend` toolchain binary.

use std::path::PathBuf;
use std::process::Command;

fn backend() -> &'static str {
    env!("CARGO_BIN_EXE_minus-a-backend")
}

fn have_toolchain() -> bool {
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    Command::new(cc)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "minus_a_backend_test_{}_{}",
            std::process::id(),
            tag
        ));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        Scratch(dir)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn assembles_explicit_assembly() {
    if !have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let scratch = Scratch::new("assemble");
    let asm = scratch.path("manual.s");
    std::fs::write(
        &asm,
        ".intel_syntax noprefix
.text
.globl main
.type main, @function
main:
    push rbp
    mov rbp, rsp
    lea rdi, [rip + msg]
    mov al, 0
    call printf
    xor eax, eax
    leave
    ret
.section .rodata
msg: .asciz \"minus-a-backend works\\n\"
.section .note.GNU-stack,\"\",@progbits
",
    )
    .unwrap();
    let exe = scratch.path("manual");

    let result = Command::new(backend())
        .arg(&asm)
        .args(["-o"])
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "minus-a-backend failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let run = Command::new(&exe).output().expect("run program");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "minus-a-backend works\n"
    );
}

#[test]
fn rejects_a_missing_input_file() {
    let result = Command::new(backend())
        .arg("/nonexistent/file.s")
        .args(["-o", "/tmp/minus_a_backend_should_not_exist"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("does not exist"));
}

#[test]
fn rejects_invalid_assembly() {
    if !have_toolchain() {
        eprintln!("skipping: no assembler/linker available");
        return;
    }
    let scratch = Scratch::new("invalid");
    let asm = scratch.path("bad.s");
    std::fs::write(&asm, "this is not assembly\n").unwrap();
    let exe = scratch.path("bad");

    let result = Command::new(backend())
        .arg(&asm)
        .args(["-o"])
        .arg(&exe)
        .output()
        .unwrap();
    assert!(!result.status.success(), "invalid assembly must fail");
}

#[test]
fn help_and_version_are_available() {
    assert!(Command::new(backend())
        .arg("--help")
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new(backend())
        .arg("--version")
        .output()
        .unwrap()
        .status
        .success());
}

#[test]
fn library_driver_reports_missing_input() {
    let err = minus_a_backend::assemble(
        std::path::Path::new("/nonexistent/lib_input.s"),
        std::path::Path::new("/tmp/minus_a_backend_lib_out"),
    )
    .unwrap_err();
    assert!(err.contains("does not exist"), "error: {}", err);
}

//! `-a doctor`: report the state of this installation and its toolchain.
//!
//! `doctor` only reads: it runs no installer, changes no `PATH` entry, and
//! writes nothing outside a temporary directory that it removes again. Every
//! check prints `ok`, `warn` or `fail`; the command exits 1 when any check
//! failed, so scripts can gate on it.

use super::{toolchain, PROG, VERSION};
use crate::diagnostics::Diagnostic;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Outcome of a single check.
enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    fn tag(&self) -> &'static str {
        match self {
            Status::Ok => " ok ",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

struct Report {
    lines: Vec<(Status, String, String)>,
}

impl Report {
    fn new() -> Report {
        Report { lines: Vec::new() }
    }

    fn push(&mut self, status: Status, label: &str, detail: impl Into<String>) {
        self.lines.push((status, label.to_string(), detail.into()));
    }

    fn problems(&self) -> usize {
        self.lines
            .iter()
            .filter(|(s, _, _)| matches!(s, Status::Fail))
            .count()
    }

    fn warnings(&self) -> usize {
        self.lines
            .iter()
            .filter(|(s, _, _)| matches!(s, Status::Warn))
            .count()
    }

    fn print(&self) {
        println!("{}: doctor", PROG);
        println!();
        let width = self
            .lines
            .iter()
            .map(|(_, label, _)| label.chars().count())
            .max()
            .unwrap_or(0);
        for (status, label, detail) in &self.lines {
            println!(
                "  [{}] {:<width$}  {}",
                status.tag(),
                label,
                detail,
                width = width
            );
        }
        println!();
        if self.problems() == 0 {
            println!(
                "{}: doctor found no problems ({} warning(s))",
                PROG,
                self.warnings()
            );
        } else {
            println!(
                "{}: doctor found {} problem(s) and {} warning(s)",
                PROG,
                self.problems(),
                self.warnings()
            );
        }
    }
}

/// `-a doctor`
pub fn doctor() -> Result<u8, Diagnostic> {
    let mut report = Report::new();

    // ---------------------------------------------------------- version
    report.push(Status::Ok, "version", VERSION);

    // ------------------------------------------------------ executable
    let exe = std::env::current_exe().ok();
    let exe_display = match &exe {
        Some(path) => match std::fs::canonicalize(path) {
            Ok(canonical) => canonical.display().to_string(),
            Err(_) => path.display().to_string(),
        },
        None => "<unknown>".to_string(),
    };
    match &exe {
        Some(_) => report.push(Status::Ok, "executable", exe_display.clone()),
        None => report.push(Status::Warn, "executable", "could not determine own path"),
    }
    let invoked_as = std::env::args()
        .next()
        .map(|a| {
            Path::new(&a)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or(a.clone())
        })
        .unwrap_or_else(|| "-a".to_string());
    if invoked_as == "-a" {
        report.push(Status::Ok, "installed name", "-a");
    } else {
        report.push(
            Status::Warn,
            "installed name",
            format!(
                "running as '{}', not '-a' (install with scripts/install.sh --user)",
                invoked_as
            ),
        );
    }

    // ------------------------------------------------------------ PATH
    match find_on_path("-a") {
        Some(found) => {
            let matches_exe = match (&exe, std::fs::canonicalize(&found)) {
                (Some(exe), Ok(found_canonical)) => match std::fs::canonicalize(exe) {
                    Ok(exe_canonical) => exe_canonical == found_canonical,
                    Err(_) => false,
                },
                _ => false,
            };
            if matches_exe {
                report.push(
                    Status::Ok,
                    "PATH",
                    format!("'-a' resolves to {}", found.display()),
                );
            } else {
                report.push(
                    Status::Warn,
                    "PATH",
                    format!(
                        "'-a' resolves to {} which is not the running compiler",
                        found.display()
                    ),
                );
            }
        }
        None => {
            let hint = match &exe {
                Some(path) => format!(
                    "'-a' is not on PATH; add {} to PATH",
                    path.parent()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "<install dir>".to_string())
                ),
                None => "'-a' is not on PATH".to_string(),
            };
            report.push(Status::Fail, "PATH", hint);
        }
    }

    // ------------------------------------------------- installed copies
    let user_bin = home_dir().map(|home| home.join(".local").join("bin"));
    let system_bin = PathBuf::from("/usr/local/bin");
    for (label, dir) in [
        ("user install", &user_bin),
        ("system install", &Some(system_bin.clone())),
    ] {
        let Some(dir) = dir else {
            report.push(Status::Warn, label, "install directory unknown");
            continue;
        };
        let target = dir.join("-a");
        if target.exists() {
            report.push(Status::Ok, label, target.display().to_string());
        } else if dir.is_dir() {
            report.push(
                Status::Warn,
                label,
                format!("not installed in {}", dir.display()),
            );
        } else {
            report.push(
                Status::Warn,
                label,
                format!("{} does not exist", dir.display()),
            );
        }
    }

    // ---------------------------------------------------- assembler/cc
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    match Command::new(&cc).arg("--version").output() {
        Ok(out) if out.status.success() => {
            let first = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            let source = if std::env::var_os("CC").is_some() {
                format!("{} (from CC)", cc)
            } else {
                cc.clone()
            };
            report.push(
                Status::Ok,
                "assembler/linker",
                if first.is_empty() {
                    source
                } else {
                    format!("{} — {}", source, first)
                },
            );
        }
        Ok(out) => report.push(
            Status::Fail,
            "assembler/linker",
            format!("'{}' exited with {}", cc, out.status),
        ),
        Err(e) => report.push(
            Status::Fail,
            "assembler/linker",
            format!(
                "'{}' could not be run ({}); install a C toolchain (gcc or clang)",
                cc, e
            ),
        ),
    }

    // ---------------------------------------------------- end-to-end
    match smoke_test() {
        Ok(Ok(detail)) => report.push(Status::Ok, "backend", detail),
        Ok(Err(detail)) => report.push(Status::Fail, "backend", detail),
        Err(detail) => report.push(Status::Warn, "backend", detail),
    }

    // ------------------------------------------------------ permissions
    match temp_writable() {
        Ok(path) => report.push(
            Status::Ok,
            "temp directory",
            format!("{} is writable", path.display()),
        ),
        Err(e) => report.push(Status::Fail, "temp directory", e),
    }
    report.push(
        Status::Ok,
        "privileges",
        if is_root() {
            "running as root; system-wide installs need no elevation".to_string()
        } else if user_bin.as_ref().map(|d| d.is_dir()).unwrap_or(false) {
            format!(
                "user installs need no elevation ({} exists)",
                user_bin.unwrap().display()
            )
        } else {
            "user installs need no elevation; system installs (--system) need sudo".to_string()
        },
    );

    report.print();
    if report.problems() > 0 {
        Ok(1)
    } else {
        Ok(0)
    }
}

/// Compile, assemble, link and run a one-line program in a scratch directory.
fn smoke_test() -> Result<Result<String, String>, String> {
    let scratch = match toolchain::Scratch::new("doctor") {
        Ok(s) => s,
        Err(e) => return Err(format!("no scratch directory: {}", e.message)),
    };
    let source = "block main(1) { aout(\"doctor ok\") }\n";
    let source_path = scratch.path("doctor.ma");
    if let Err(e) = std::fs::write(&source_path, source) {
        return Err(format!("cannot write a scratch source: {}", e));
    }
    let assembly = match crate::compile_file(&source_path) {
        Ok(a) => a,
        Err(d) => {
            return Ok(Err(format!(
                "the compiler rejected its own program: {}",
                d.message
            )))
        }
    };
    let asm_path = scratch.path("doctor.s");
    let exe_path = scratch.path("doctor");
    if let Err(e) = std::fs::write(&asm_path, assembly) {
        return Err(format!("cannot write scratch assembly: {}", e));
    }
    if let Err(d) = toolchain::assemble_and_link(&asm_path, &exe_path) {
        return Ok(Err(d.message));
    }
    match Command::new(&exe_path).output() {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if stdout == "doctor ok" {
                Ok(Ok("compile, assemble, link and run all work".to_string()))
            } else {
                Ok(Err(format!(
                    "a test program ran but printed '{}' instead of 'doctor ok'",
                    stdout
                )))
            }
        }
        Ok(out) => Ok(Err(format!(
            "a test program exited with {}",
            out.status.code().unwrap_or(-1)
        ))),
        Err(e) => Ok(Err(format!("a test program could not be started: {}", e))),
    }
}

fn temp_writable() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir();
    let probe = dir.join(format!("a-core-doctor-{}", std::process::id()));
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Ok(dir)
        }
        Err(e) => Err(format!("{} is not writable: {}", dir.display(), e)),
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn is_root() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|uid| uid.trim() == "0")
        .unwrap_or(false)
}

/// Look up `name` in `PATH` the way a shell would.
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

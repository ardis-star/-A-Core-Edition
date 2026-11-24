//! Tests for `scripts/install.sh` and `scripts/uninstall.sh`.
//!
//! Every test installs into a throw-away `HOME` (and, for staged system
//! installs, a throw-away `DESTDIR`), so nothing is written outside the test's
//! scratch directory and no part of the host system is touched. The scripts are
//! handed the freshly built test binary through `A_BINARY`, so no cargo build
//! happens inside the tests.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn compiler() -> &'static str {
    env!("CARGO_BIN_EXE_minus-a")
}

fn scripts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("scripts")
}

fn have_shell() -> bool {
    Command::new("sh")
        .arg("-c")
        .arg("exit 0")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run an installer script with an isolated HOME.
fn run_script(script: &str, home: &Path, args: &[&str]) -> Output {
    Command::new("sh")
        .arg(scripts_dir().join(script))
        .args(args)
        .env("HOME", home)
        .env("SHELL", "/bin/bash")
        .env("A_BINARY", compiler())
        .env_remove("DESTDIR")
        .output()
        .unwrap_or_else(|e| panic!("cannot run {}: {}", script, e))
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Every `-a` the test can find under `home`, excluding backups.
fn installed_binaries(home: &Path) -> Vec<PathBuf> {
    let bin = home.join(".local/bin");
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&bin) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "-a" {
                found.push(entry.path());
            }
        }
    }
    found
}

#[test]
fn install_user_puts_a_working_compiler_in_home() {
    if !have_shell() {
        eprintln!("skipping: no POSIX shell available");
        return;
    }
    let scratch = support::Scratch::new("install_user");
    let home = scratch.path("home");
    std::fs::create_dir_all(&home).unwrap();

    let out = run_script("install.sh", &home, &["--user"]);
    assert!(
        out.status.success(),
        "install failed:\nstdout: {}\nstderr: {}",
        stdout(&out),
        stderr(&out)
    );

    let target = home.join(".local/bin/-a");
    assert!(
        target.is_file(),
        "expected the compiler at {}",
        target.display()
    );

    // It must be a real, runnable executable that reports the product name.
    let version = Command::new(&target).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert!(
        stdout(&version).contains("-A Core Edition"),
        "got: {}",
        stdout(&version)
    );

    // And it must be usable from an unrelated directory, as `-a`, with only the
    // installed directory on PATH. The commands go in a script file, because
    // `sh -c '-a ...'` makes the shell parse its own argument as options.
    let work = scratch.path("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(
        work.join("use-it.sh"),
        "-a create hello.ma\n-a run hello.ma\n",
    )
    .unwrap();
    let bin = home.join(".local/bin");
    let shell = Command::new("sh")
        .arg("use-it.sh")
        .current_dir(&work)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(
        shell.status.success(),
        "the installed `-a` must work outside the source tree:\nstdout: {}\nstderr: {}",
        stdout(&shell),
        stderr(&shell)
    );
    assert!(
        stdout(&shell).contains("created hello.ma") && stdout(&shell).contains("Hello, world!"),
        "unexpected output: {}",
        stdout(&shell)
    );
    assert!(work.join("hello.ma").is_file());
}

#[test]
fn install_user_configures_path_once_and_only_once() {
    if !have_shell() {
        eprintln!("skipping: no POSIX shell available");
        return;
    }
    let scratch = support::Scratch::new("install_path");
    let home = scratch.path("home");
    std::fs::create_dir_all(&home).unwrap();

    let first = run_script("install.sh", &home, &["--user"]);
    assert!(first.status.success(), "install failed: {}", stderr(&first));

    let rc = home.join(".bashrc");
    let after_first = std::fs::read_to_string(&rc).expect("installer must configure .bashrc");
    assert!(
        after_first.contains("-A Core Edition"),
        "expected a marked PATH block, got:\n{}",
        after_first
    );
    assert!(
        after_first.contains(".local/bin"),
        "the block must add the install directory: {}",
        after_first
    );

    let second = run_script("install.sh", &home, &["--user"]);
    assert!(
        second.status.success(),
        "re-install failed: {}",
        stderr(&second)
    );
    let after_second = std::fs::read_to_string(&rc).unwrap();
    assert_eq!(
        after_first, after_second,
        "a second install must not add another PATH entry"
    );

    // --no-path must leave shell configuration alone.
    let other_home = scratch.path("home2");
    std::fs::create_dir_all(&other_home).unwrap();
    let quiet = run_script("install.sh", &other_home, &["--user", "--no-path"]);
    assert!(quiet.status.success(), "install failed: {}", stderr(&quiet));
    assert!(
        !other_home.join(".bashrc").exists(),
        "--no-path must not write any shell configuration"
    );
}

#[test]
fn install_user_backs_up_an_unrelated_a_binary() {
    if !have_shell() {
        eprintln!("skipping: no POSIX shell available");
        return;
    }
    let scratch = support::Scratch::new("install_collision");
    let home = scratch.path("home");
    let bin = home.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let squatter = bin.join("-a");
    std::fs::write(&squatter, "#!/bin/sh\necho not minus-a\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&squatter, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let out = run_script("install.sh", &home, &["--user"]);
    assert!(out.status.success(), "install failed: {}", stderr(&out));

    // The unrelated binary was preserved under a backup name …
    let backups: Vec<PathBuf> = std::fs::read_dir(&bin)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with("-a.bak."))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(
        backups.len(),
        1,
        "expected exactly one backup, found {:?}",
        backups
    );
    assert!(
        std::fs::read_to_string(&backups[0])
            .unwrap()
            .contains("not minus-a"),
        "the backup must contain the original file"
    );

    // … and the installed `-a` is now the compiler.
    let version = Command::new(&squatter).arg("--version").output().unwrap();
    assert!(stdout(&version).contains("-A Core Edition"));
}

#[test]
fn install_and_uninstall_user_round_trip() {
    if !have_shell() {
        eprintln!("skipping: no POSIX shell available");
        return;
    }
    let scratch = support::Scratch::new("install_round_trip");
    let home = scratch.path("home");
    std::fs::create_dir_all(&home).unwrap();

    assert!(run_script("install.sh", &home, &["--user"])
        .status
        .success());
    assert_eq!(installed_binaries(&home).len(), 1);
    let rc = home.join(".bashrc");
    assert!(std::fs::read_to_string(&rc)
        .unwrap()
        .contains("-A Core Edition"));

    let out = run_script("uninstall.sh", &home, &["--user"]);
    assert!(
        out.status.success(),
        "uninstall failed:\nstdout: {}\nstderr: {}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        installed_binaries(&home).is_empty(),
        "uninstall must remove the compiler"
    );
    let rc_after = std::fs::read_to_string(&rc).unwrap();
    assert!(
        !rc_after.contains("-A Core Edition"),
        "uninstall must remove the PATH block, got:\n{}",
        rc_after
    );

    // Uninstalling again is harmless.
    let again = run_script("uninstall.sh", &home, &["--user"]);
    assert!(again.status.success(), "second uninstall must succeed");
}

#[test]
fn install_system_stages_into_a_destdir_without_touching_path() {
    if !have_shell() {
        eprintln!("skipping: no POSIX shell available");
        return;
    }
    let scratch = support::Scratch::new("install_system");
    let home = scratch.path("home");
    let stage = scratch.path("stage");
    std::fs::create_dir_all(&home).unwrap();

    let out = Command::new("sh")
        .arg(scripts_dir().join("install.sh"))
        .args(["--system", "--destdir"])
        .arg(&stage)
        .env("HOME", &home)
        .env("A_BINARY", compiler())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "staged system install failed:\nstdout: {}\nstderr: {}",
        stdout(&out),
        stderr(&out)
    );

    let target = stage.join("usr/local/bin/-a");
    assert!(target.is_file(), "expected {}", target.display());
    let version = Command::new(&target).arg("--version").output().unwrap();
    assert!(stdout(&version).contains("-A Core Edition"));
    assert!(
        !home.join(".bashrc").exists(),
        "a staged installation must not edit shell configuration"
    );

    // Staged uninstall removes it again.
    let un = Command::new("sh")
        .arg(scripts_dir().join("uninstall.sh"))
        .args(["--system", "--destdir"])
        .arg(&stage)
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(
        un.status.success(),
        "staged uninstall failed: {}",
        stderr(&un)
    );
    assert!(!target.exists(), "staged uninstall must remove the binary");
}

#[test]
fn install_prefix_places_the_command_under_that_prefix() {
    if !have_shell() {
        eprintln!("skipping: no POSIX shell available");
        return;
    }
    let scratch = support::Scratch::new("install_prefix");
    let home = scratch.path("home");
    let prefix = scratch.path("prefix");
    std::fs::create_dir_all(&home).unwrap();

    let out = run_script(
        "install.sh",
        &home,
        &["--user", "--prefix", prefix.to_str().unwrap()],
    );
    assert!(out.status.success(), "install failed: {}", stderr(&out));
    assert!(prefix.join("bin/-a").is_file());
    let rc = std::fs::read_to_string(home.join(".bashrc")).unwrap();
    assert!(
        rc.contains(prefix.join("bin").to_str().unwrap()),
        "the PATH entry must point at the chosen prefix, got:\n{}",
        rc
    );
}

#[test]
fn doctor_sees_a_healthy_installation() {
    if !have_shell() || !support::have_toolchain() {
        eprintln!("skipping: needs a shell and an assembler/linker");
        return;
    }
    let scratch = support::Scratch::new("install_doctor");
    let home = scratch.path("home");
    std::fs::create_dir_all(&home).unwrap();
    assert!(run_script("install.sh", &home, &["--user"])
        .status
        .success());
    let bin = home.join(".local/bin");

    let out = Command::new(bin.join("-a"))
        .arg("doctor")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .output()
        .unwrap();
    let text = stdout(&out);
    assert!(
        text.contains("[ ok ] PATH"),
        "doctor must see the installed command on PATH:\n{}",
        text
    );
    assert!(
        text.contains("[ ok ] backend"),
        "doctor must verify the backend end to end:\n{}",
        text
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "a healthy installation must not report problems:\n{}",
        text
    );
}

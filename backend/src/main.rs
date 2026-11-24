//! `minus-a-backend` command-line interface: the -A Core Edition assembly
//! backend driver.
//!
//! Usage: `minus-a-backend <input.s> -o <executable>`
//!
//! The backend turns the x86-64 assembly emitted by the -A compiler into an
//! executable. It is a thin driver around the system assembler/linker, and it
//! never rewrites its input.

use minus_a_backend::assemble;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

const VERSION: &str = "-A Core Edition assembly backend 0.1.0 (minus-a-backend)";

/// Prefix for the backend's own messages. The backend is part of -A Core
/// Edition and identifies itself as such.
const PROG: &str = "-A Core Edition backend";

fn usage() -> &'static str {
    "-A Core Edition assembly backend (minus-a-backend)\n\
     \n\
     USAGE:\n\
     \x20   minus-a-backend <input.s> -o <executable>\n\
     \n\
     OPTIONS:\n\
     \x20   -o <path>  Output executable path\n\
     \x20   --help    Show this help\n\
     \x20   --version Show version\n\
     \n\
     Invokes: cc -x assembler -pthread <input> -o <output>\n\
     Set the CC environment variable to use a different compiler.\n"
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprint!("{}", usage());
        return ExitCode::from(2);
    }
    match args[1].as_str() {
        "--help" | "-h" | "help" => {
            print!("{}", usage());
            return ExitCode::from(0);
        }
        "--version" | "-V" => {
            println!("{}", VERSION);
            return ExitCode::from(0);
        }
        _ => {}
    }
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("{}: -o requires a path", PROG);
                    return ExitCode::from(2);
                }
                output = Some(PathBuf::from(&args[i]));
            }
            "--help" | "-h" => {
                print!("{}", usage());
                return ExitCode::from(0);
            }
            other if other.starts_with('-') => {
                eprintln!("{}: unknown option '{}'", PROG, other);
                eprint!("{}", usage());
                return ExitCode::from(2);
            }
            other => {
                if input.is_some() {
                    eprintln!("{}: unexpected extra argument '{}'", PROG, other);
                    return ExitCode::from(2);
                }
                input = Some(PathBuf::from(other));
            }
        }
        i += 1;
    }
    let input = match input {
        Some(p) => p,
        None => {
            eprint!("{}: missing input file\n{}", PROG, usage());
            return ExitCode::from(2);
        }
    };
    let output = match output {
        Some(p) => p,
        None => {
            let stem = input
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "program".to_string());
            PathBuf::from(stem)
        }
    };
    match assemble(&input, &output) {
        Ok(()) => {
            eprintln!("{}: wrote {}", PROG, output.display());
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("{}: error: {}", PROG, e);
            ExitCode::from(1)
        }
    }
}

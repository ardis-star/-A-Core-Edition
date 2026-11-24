//! The -A Core Edition command-line interface.
//!
//! This module is the whole user-facing surface of the compiler:
//!
//! ```text
//! -a create  <name>            create a new .ma source file
//! -a run     <input.ma>        compile, assemble, link and execute
//! -a build   <input.ma>        produce a persistent executable
//! -a compile <input.ma>        produce x86-64 assembly (.s)
//! -a check   <input.ma>        parse and type-check only
//! -a doctor                    diagnose the installation and toolchain
//! -a --version | --help
//! ```
//!
//! The installed executable is named `-a`; the Cargo binary target keeps the
//! Rust-friendly name `minus-a`. Everything below behaves identically whichever
//! name was used to start it.
//!
//! Exit status: `0` success, `1` the program was rejected / assembly, linking or
//! execution failed / `doctor` found a problem, `2` usage error.

use crate::diagnostics::{Diagnostic, Span};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub mod create;
pub mod doctor;
pub mod toolchain;

/// Product name used in every message.
pub const PROG: &str = "-A Core Edition";
/// Version string, e.g. `-A Core Edition 0.1.0`.
pub const VERSION: &str = concat!("-A Core Edition ", env!("CARGO_PKG_VERSION"));
/// The only accepted -A source extension.
pub const SOURCE_EXT: &str = "ma";

/// What the user asked for.
pub enum Command {
    Help,
    Version,
    Create {
        name: String,
        force: bool,
    },
    Run {
        input: PathBuf,
        args: Vec<String>,
    },
    Build {
        input: PathBuf,
        output: Option<PathBuf>,
        options: crate::Options,
    },
    Compile {
        input: PathBuf,
        output: Option<PathBuf>,
        options: crate::Options,
    },
    Check {
        input: PathBuf,
    },
    Doctor,
}

/// Usage text; kept in one place so `--help`, usage errors and the docs agree.
pub fn usage() -> String {
    format!(
        "{prog} — compiler for the -A language\n\
         \n\
         USAGE:\n\
         \x20   -a create  <name>                  Create a new .ma source file\n\
         \x20   -a run     <input.ma>              Compile and run a program\n\
         \x20   -a build   <input.ma> [-o <exe>]   Build an executable\n\
         \x20   -a compile <input.ma> [-o <asm>]   Emit x86-64 assembly (.s)\n\
         \x20   -a check   <input.ma>              Parse and type-check only\n\
         \x20   -a doctor                         Diagnose the installation\n\
         \n\
         OPTIONS:\n\
         \x20   -o <path>   Output path (default: <input stem>.s for compile,\n\
         \x20               <input stem> next to the source for build)\n\
         \x20   --force     create: overwrite an existing file\n\
         \x20   --no-opt    Disable the optimizer\n\
         \x20   --            run: everything after this is passed to the program\n\
         \x20   --help      Show this help\n\
         \x20   --version   Show version\n\
         \n\
         SOURCE FILES:\n\
         \x20   -A sources use the .ma extension ({ext} is the only accepted one).\n\
         \n\
         EXAMPLES:\n\
         \x20   -a create hello.ma\n\
         \x20   -a run hello.ma\n\
         \x20   -a build hello.ma -o hello\n\
         \x20   -a run ./project/main.ma\n\
         \n\
         EXIT STATUS:\n\
         \x20   0  success\n\
         \x20   1  rejected program, failed assembly/linking/execution, or doctor problems\n\
         \x20   2  usage error\n",
        prog = PROG,
        ext = SOURCE_EXT
    )
}

/// Parse `args` (without the program name) into a [`Command`].
///
/// Usage problems are reported as `E_USAGE` diagnostics so that the CLI prints
/// them the same way it prints compiler errors; the caller maps that code to
/// exit status 2.
pub fn parse(args: &[String]) -> Result<Command, Diagnostic> {
    let Some(first) = args.first() else {
        return Ok(Command::Help);
    };
    match first.as_str() {
        "--help" | "-h" | "help" | "-help" => Ok(Command::Help),
        "--version" | "-V" | "version" | "-version" => Ok(Command::Version),
        "create" => {
            let mut name = None;
            let mut force = false;
            for arg in &args[1..] {
                match arg.as_str() {
                    "--force" | "-f" => force = true,
                    "--help" | "-h" => return Ok(Command::Help),
                    other if other.starts_with('-') => {
                        return Err(usage_error(format!("unknown option '{}'", other)))
                    }
                    other => {
                        if name.is_some() {
                            return Err(usage_error(format!(
                                "create takes one file name, found an extra argument '{}'",
                                other
                            )));
                        }
                        name = Some(other.to_string());
                    }
                }
            }
            let name = name.ok_or_else(|| usage_error("create needs a file name"))?;
            Ok(Command::Create { name, force })
        }
        "run" => {
            let mut input = None;
            let mut program_args = Vec::new();
            let mut rest = false;
            for arg in &args[1..] {
                if rest {
                    program_args.push(arg.clone());
                    continue;
                }
                match arg.as_str() {
                    "--" => rest = true,
                    "--help" | "-h" => return Ok(Command::Help),
                    other if other.starts_with('-') => {
                        return Err(usage_error(format!("unknown option '{}'", other)))
                    }
                    other => {
                        if input.is_some() {
                            return Err(usage_error(format!(
                                "run takes one source file, found an extra argument '{}'",
                                other
                            )));
                        }
                        input = Some(PathBuf::from(other));
                    }
                }
            }
            let input = input.ok_or_else(|| usage_error("run needs a source file"))?;
            Ok(Command::Run {
                input,
                args: program_args,
            })
        }
        "build" | "compile" => {
            let mut input = None;
            let mut output = None;
            let mut options = crate::Options::default();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "-o" => {
                        i += 1;
                        let Some(value) = args.get(i) else {
                            return Err(usage_error("-o requires a path"));
                        };
                        output = Some(PathBuf::from(value));
                    }
                    "--no-opt" => options.optimize = false,
                    "--help" | "-h" => return Ok(Command::Help),
                    other if other.starts_with('-') => {
                        return Err(usage_error(format!("unknown option '{}'", other)))
                    }
                    other => {
                        if input.is_some() {
                            return Err(usage_error(format!(
                                "{} takes one source file, found an extra argument '{}'",
                                first, other
                            )));
                        }
                        input = Some(PathBuf::from(other));
                    }
                }
                i += 1;
            }
            let input =
                input.ok_or_else(|| usage_error(format!("{} needs a source file", first)))?;
            if first == "build" {
                Ok(Command::Build {
                    input,
                    output,
                    options,
                })
            } else {
                Ok(Command::Compile {
                    input,
                    output,
                    options,
                })
            }
        }
        "check" => {
            let mut input = None;
            for arg in &args[1..] {
                match arg.as_str() {
                    "--help" | "-h" => return Ok(Command::Help),
                    other if other.starts_with('-') => {
                        return Err(usage_error(format!("unknown option '{}'", other)))
                    }
                    other => {
                        if input.is_some() {
                            return Err(usage_error(format!(
                                "check takes one source file, found an extra argument '{}'",
                                other
                            )));
                        }
                        input = Some(PathBuf::from(other));
                    }
                }
            }
            let input = input.ok_or_else(|| usage_error("check needs a source file"))?;
            Ok(Command::Check { input })
        }
        "doctor" => {
            for arg in &args[1..] {
                match arg.as_str() {
                    "--help" | "-h" => return Ok(Command::Help),
                    other => {
                        return Err(usage_error(format!(
                            "doctor takes no arguments, found '{}'",
                            other
                        )))
                    }
                }
            }
            Ok(Command::Doctor)
        }
        other => Err(usage_error(format!(
            "unknown command '{}'; run '-a --help' for the command list",
            other
        ))),
    }
}

fn usage_error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_USAGE", message, Span::dummy())
}

/// Run one invocation and produce the process exit code.
pub fn run(args: &[String]) -> ExitCode {
    match parse(args) {
        Ok(Command::Help) => {
            print!("{}", usage());
            ExitCode::from(0)
        }
        Ok(Command::Version) => {
            println!("{}", VERSION);
            ExitCode::from(0)
        }
        Ok(command) => match execute(command) {
            Ok(code) => ExitCode::from(code),
            Err(d) => {
                eprint!("{}", d.report());
                if d.code == "E_USAGE" {
                    eprintln!("\n{}: run '-a --help' for usage", PROG);
                    ExitCode::from(2)
                } else {
                    ExitCode::from(1)
                }
            }
        },
        Err(d) => {
            eprint!("{}", d.report());
            eprintln!("\n{}: run '-a --help' for usage", PROG);
            ExitCode::from(2)
        }
    }
}

/// Execute a parsed command, returning the process exit status.
pub fn execute(command: Command) -> Result<u8, Diagnostic> {
    match command {
        Command::Help => {
            print!("{}", usage());
            Ok(0)
        }
        Command::Version => {
            println!("{}", VERSION);
            Ok(0)
        }
        Command::Create { name, force } => create::create(&name, force),
        Command::Check { input } => check(&input),
        Command::Compile {
            input,
            output,
            options,
        } => compile(&input, output, options),
        Command::Build {
            input,
            output,
            options,
        } => build(&input, output, options),
        Command::Run { input, args } => toolchain::run_program(&input, &args),
        Command::Doctor => doctor::doctor(),
    }
}

// ---------------------------------------------------------------- helpers

/// Reject anything that is not a `.ma` source file, with a hint that names the
/// official extension.
pub fn validate_source(path: &Path) -> Result<(), Diagnostic> {
    match path.extension().and_then(|e| e.to_str()) {
        Some(SOURCE_EXT) => Ok(()),
        other => {
            let found = match other {
                Some(ext) => format!(".{}", ext),
                None => "no extension".to_string(),
            };
            let suggestion = match path.file_stem() {
                Some(stem) => format!(
                    "rename it to '{}.ma', or create a new file with `-a create {}`",
                    stem.to_string_lossy(),
                    stem.to_string_lossy()
                ),
                None => "rename it to use the .ma extension".to_string(),
            };
            Err(Diagnostic::new(
                "E_IO",
                format!(
                    "'{}' is not a -A source file (found {}); -A sources use the .ma extension",
                    path.display(),
                    found
                ),
                Span::dummy(),
            )
            .with_file(path.display().to_string())
            .with_hint(suggestion))
        }
    }
}

/// The input file's stem, used to derive default output names.
pub fn stem_of(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "program".to_string());
    PathBuf::from(stem)
}

fn read_source(path: &Path) -> Result<String, Diagnostic> {
    std::fs::read_to_string(path).map_err(|e| {
        Diagnostic::new(
            "E_IO",
            format!("cannot read '{}': {}", path.display(), e),
            Span::dummy(),
        )
        .with_file(path.display().to_string())
        .with_hint("check the path; -A sources end in .ma")
    })
}

fn write_file(path: &Path, contents: &str) -> Result<(), Diagnostic> {
    std::fs::write(path, contents).map_err(|e| {
        Diagnostic::new(
            "E_IO",
            format!("cannot write '{}': {}", path.display(), e),
            Span::dummy(),
        )
        .with_file(path.display().to_string())
    })
}

/// `-a check`
pub fn check(input: &Path) -> Result<u8, Diagnostic> {
    validate_source(input)?;
    read_source(input)?;
    crate::check_file(input).map_err(|d| attach_file(d, input))?;
    eprintln!("{}: {}: OK", PROG, input.display());
    Ok(0)
}

/// `-a compile`
pub fn compile(
    input: &Path,
    output: Option<PathBuf>,
    options: crate::Options,
) -> Result<u8, Diagnostic> {
    validate_source(input)?;
    read_source(input)?;
    let assembly = crate::compile_file_opts(input, options).map_err(|d| attach_file(d, input))?;
    let output = output.unwrap_or_else(|| {
        let mut p = stem_of(input);
        p.set_extension("s");
        p
    });
    write_file(&output, &assembly)?;
    eprintln!("{}: wrote {}", PROG, output.display());
    Ok(0)
}

/// `-a build`
pub fn build(
    input: &Path,
    output: Option<PathBuf>,
    options: crate::Options,
) -> Result<u8, Diagnostic> {
    validate_source(input)?;
    read_source(input)?;
    let assembly = crate::compile_file_opts(input, options).map_err(|d| attach_file(d, input))?;
    let exe = output.unwrap_or_else(|| stem_of(input));
    // The intermediate assembly sits next to the executable it belongs to, so a
    // build leaves exactly two predictable files behind.
    let mut asm_path = exe.clone();
    asm_path.set_extension("s");
    write_file(&asm_path, &assembly)?;
    toolchain::assemble_and_link(&asm_path, &exe)?;
    eprintln!(
        "{}: wrote {} (via {})",
        PROG,
        exe.display(),
        asm_path.display()
    );
    Ok(0)
}

/// Attach the source file name to a diagnostic so the report can name it.
pub fn attach_file(d: Diagnostic, input: &Path) -> Diagnostic {
    if d.file.is_empty() {
        d.with_file(input.display().to_string())
    } else {
        d
    }
}

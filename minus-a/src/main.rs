//! The `minus-a` binary: a thin shell around [`minus_a::cli`].
//!
//! The installed executable is named `-a` (see `scripts/install.sh`); the Cargo
//! target keeps the Rust-friendly name so `cargo run`, `cargo test` and
//! `CARGO_BIN_EXE_minus-a` keep working.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    minus_a::cli::run(&args)
}

// provenance: original
//! `cargo xtask`: unshipped developer tooling for klein-science.
//!
//! Each command group owns its sub-enum in its own module; `main` only dispatches.

mod store;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Errors are plain messages: xtask reports them and exits non-zero.
pub type Res<T> = Result<T, String>;

#[derive(Parser)]
#[command(name = "xtask", about = "klein-science developer tooling (unshipped)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Content-addressed artifact store (local cache, remote OCI store, recipes).
    #[command(subcommand)]
    Store(store::StoreCmd),
}

fn main() -> ExitCode {
    let result = match Cli::parse().cmd {
        Cmd::Store(cmd) => store::run(cmd),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
    }
}

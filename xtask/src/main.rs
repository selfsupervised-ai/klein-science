// provenance: original
//! `cargo xtask`: unshipped developer tooling for klein-science.
//!
//! Each command group owns its sub-enum in its own module; `main` only dispatches.

mod bench;
mod check;
mod ci;
mod depinfo;
mod host;
mod oracle;
mod release;
mod store;
mod workspace;

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
    /// Pinned host tools and the heavy-job guard.
    #[command(subcommand)]
    Tools(host::ToolsCmd),
    /// Repository guards.
    #[command(subcommand)]
    Check(check::CheckCmd),
    /// Run the CI steps in order under the heavy-job guard.
    Ci,
    /// Offline oracle images and the one hardened container runner.
    #[command(subcommand)]
    Oracle(oracle::OracleCmd),
    /// Build the shipped artifacts (R0 component, R2 worker) and their digests.
    #[command(subcommand)]
    Release(release::ReleaseCmd),
    /// Benchmarks and probes under a fixed-box manifest.
    #[command(subcommand)]
    Bench(bench::BenchCmd),
}

fn main() -> ExitCode {
    let result = match Cli::parse().cmd {
        Cmd::Store(cmd) => store::run(cmd).map(|()| ExitCode::SUCCESS),
        Cmd::Tools(cmd) => host::run(cmd),
        Cmd::Check(cmd) => check::run(cmd),
        Cmd::Ci => ci::run(),
        Cmd::Oracle(cmd) => oracle::run(cmd),
        Cmd::Release(cmd) => release::run(cmd),
        Cmd::Bench(cmd) => bench::run(cmd),
    };
    match result {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
    }
}

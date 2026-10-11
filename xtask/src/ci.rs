// provenance: original
//! `cargo xtask ci`: the ordered CI steps, run under the heavy-job guard (D-04).
//!
//! Every step is a cargo command, run by the absolute path [`host::cargo`] resolves and
//! version-checks once before the first step: never a `cargo` found on `PATH`.

use std::process::ExitCode;

use crate::Res;
use crate::host::{self, heavy_job};
use crate::workspace;

/// The CI steps, in order: `(name, argv)`. Each argv starts with `cargo`, which runs as the
/// resolved cargo; a step that needs another tool reaches it through `cargo xtask`, which runs
/// pinned tools by absolute path. Later plans append steps here.
pub const STEPS: &[(&str, &[&str])] = &[
    ("tools check", &["cargo", "xtask", "tools", "check"]),
    ("fmt", &["cargo", "fmt", "--all", "--check"]),
    (
        "clippy",
        &[
            "cargo",
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    ),
    // LIC-01, DET-08: licences, banned crates, advisories and sources of the shipped graph
    // (deny.toml).
    (
        "deny",
        &[
            "cargo",
            "deny",
            "check",
            "advisories",
            "bans",
            "licenses",
            "sources",
        ],
    ),
    (
        "store fetch rfc8785",
        &[
            "cargo",
            "xtask",
            "store",
            "fetch",
            "conformance/corpora/rfc8785.toml",
        ],
    ),
    (
        "nextest",
        &["cargo", "nextest", "run", "--workspace", "--locked"],
    ),
    ("check no-corpus", &["cargo", "xtask", "check", "no-corpus"]),
    // LIC-02: provenance headers against the licence policy and NOTICE, on the files rustc
    // read. It runs its own all-targets, all-features `cargo check` into target/provenance, so
    // it depends on no earlier step's build; it runs last, after the cheaper guards.
    (
        "check provenance",
        &["cargo", "xtask", "check", "provenance"],
    ),
];

/// Runs every step from the workspace root and stops at the first failure.
pub fn run() -> Res<ExitCode> {
    let root = workspace::root()?;
    let cargo = host::cargo()?;
    let shown = cargo.path.display();
    println!("ci: cargo is {shown}");
    let _held = heavy_job()?;
    for (name, argv) in STEPS {
        let (program, args) = argv.split_first().ok_or("empty CI step")?;
        if *program != "cargo" {
            return Err(format!(
                "CI step {name} runs {program}; steps are cargo commands only"
            ));
        }
        println!("ci: run {name}: {shown} {}", args.join(" "));
        let status = cargo
            .command()
            .args(args)
            .current_dir(&root)
            .status()
            .map_err(|e| format!("run {shown}: {e}"))?;
        if !status.success() {
            eprintln!("ci: FAILED {name} ({status})");
            return Ok(ExitCode::FAILURE);
        }
        println!("ci: ok {name}");
    }
    println!("ci: all {} steps passed", STEPS.len());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_steps_are_cargo_commands() {
        for (name, argv) in STEPS {
            assert_eq!(argv.first(), Some(&"cargo"), "step {name}");
        }
    }
}

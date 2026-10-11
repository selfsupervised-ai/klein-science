// provenance: original
//! `cargo xtask ci`: the ordered CI steps, run under the heavy-job guard (D-04).

use std::process::{Command, ExitCode};

use crate::Res;
use crate::host::heavy_job;
use crate::store::workspace_root;

/// The CI steps, in order: `(name, argv)`. Later plans append steps here.
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
];

/// Runs every step from the workspace root and stops at the first failure.
pub fn run() -> Res<ExitCode> {
    let _held = heavy_job()?;
    let root = workspace_root();
    for (name, argv) in STEPS {
        println!("ci: run {name}: {}", argv.join(" "));
        let (program, args) = argv.split_first().ok_or("empty CI step")?;
        let status = Command::new(program)
            .args(args)
            .current_dir(&root)
            .status()
            .map_err(|e| format!("run {program}: {e}"))?;
        if !status.success() {
            eprintln!("ci: FAILED {name} ({status})");
            return Ok(ExitCode::FAILURE);
        }
        println!("ci: ok {name}");
    }
    println!("ci: all {} steps passed", STEPS.len());
    Ok(ExitCode::SUCCESS)
}

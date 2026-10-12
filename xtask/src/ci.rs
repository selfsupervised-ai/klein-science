// provenance: original
//! `cargo xtask ci`: every Phase 1 guard, in order, under the heavy-job guard (D-04). The same
//! command runs on the Mac and in `.github/workflows/ci.yml` on the VM (D-01).
//!
//! Every Rust step is a cargo command, run by the absolute path [`host::cargo`] resolves and
//! version-checks once before the first step: never a `cargo` found on `PATH`. The Python suites
//! run on the dev venv's own interpreter (`.venv/bin/python` of the checkout), which the
//! `tools venv` step creates and syncs to the hash-locked dev.lock just before them. They run
//! against the shipped worker (`KSCI_WORKER`, [`release::shipped_worker`]): the bytes the
//! `release dist` step has just built twice and gated.
//!
//! `ci` holds the heavy-job lock for the whole run, and tells each step so
//! ([`host::HEAVY_JOB_HELD`]): a step that takes the guard itself (`release dist`) then runs
//! instead of waiting on its own parent.

use std::process::{Command, ExitCode};

use crate::Res;
use crate::host::{self, heavy_job};
use crate::{release, workspace};

/// What a step runs.
pub enum Program {
    /// The resolved cargo ([`host::cargo`]).
    Cargo,
    /// The dev venv's Python, `.venv/bin/python` of the checkout.
    VenvPython,
}

/// One CI step.
pub struct Step {
    /// The name `ci` prints.
    pub name: &'static str,
    /// What it runs.
    pub program: Program,
    /// Its arguments.
    pub args: &'static [&'static str],
    /// Extra environment variables, set for this step only.
    pub env: &'static [(&'static str, &'static str)],
}

const fn cargo(name: &'static str, args: &'static [&'static str]) -> Step {
    Step {
        name,
        program: Program::Cargo,
        args,
        env: &[],
    }
}

/// The CI steps, in order. A step that needs a pinned tool reaches it through `cargo xtask`,
/// which runs pinned tools by absolute path.
pub const STEPS: &[Step] = &[
    cargo("tools check", &["xtask", "tools", "check"]),
    cargo("fmt", &["fmt", "--all", "--check"]),
    cargo(
        "clippy",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    ),
    // clippy only checks, and the test builds do not link the cdylib: this step links every host
    // artifact, ksci-component included.
    cargo(
        "build",
        &["build", "--workspace", "--all-targets", "--locked"],
    ),
    // LIC-01, DET-08: licences, banned crates, advisories and sources of the shipped graph
    // (deny.toml).
    cargo(
        "deny",
        &["deny", "check", "advisories", "bans", "licenses", "sources"],
    ),
    // LIC-02: provenance headers against the licence policy and NOTICE, on the files rustc read
    // in the host compile and the shipped R0 compile.
    cargo("check provenance", &["xtask", "check", "provenance"]),
    cargo("check no-corpus", &["xtask", "check", "no-corpus"]),
    cargo(
        "store fetch rfc8785",
        &[
            "xtask",
            "store",
            "fetch",
            "conformance/corpora/rfc8785.toml",
        ],
    ),
    // PLUG-01: `release dist --twice` builds the committed tree from two paths, compares every
    // digest, runs the import and relaxed-SIMD gates, and ships target/dist for the tests below.
    cargo("release dist", &["xtask", "release", "dist", "--twice"]),
    // The Rust workspace tests, ksci-conform against target/dist included.
    cargo("nextest", &["nextest", "run", "--workspace", "--locked"]),
    cargo("tools venv", &["xtask", "tools", "venv"]),
    Step {
        name: "standin python",
        program: Program::VenvPython,
        args: &[
            "-m", "unittest", "discover", "-s", "standin", "-t", "standin",
        ],
        env: &[],
    },
    // The runner tests import the stand-in mirror.
    Step {
        name: "conformance python",
        program: Program::VenvPython,
        args: &[
            "-m",
            "unittest",
            "discover",
            "-s",
            "conformance",
            "-t",
            "conformance",
        ],
        env: &[("PYTHONPATH", "standin")],
    },
];

/// Runs every step from the workspace root and stops at the first failure.
pub fn run() -> Res<ExitCode> {
    let root = workspace::root()?;
    let cargo = host::cargo()?;
    let python = host::venv_python_path(&root);
    let worker = release::shipped_worker(&root)?;
    println!("ci: cargo is {}", cargo.path.display());
    let _held = heavy_job()?;
    for step in STEPS {
        let mut cmd = match step.program {
            Program::Cargo => cargo.command(),
            Program::VenvPython => {
                let mut cmd = Command::new(&python);
                cmd.env("KSCI_WORKER", &worker);
                cmd
            }
        };
        cmd.args(step.args)
            .envs(step.env.iter().copied())
            .env(host::HEAVY_JOB_HELD, "1")
            .current_dir(&root);
        let shown = cmd.get_program().to_string_lossy().into_owned();
        println!("ci: run {}: {shown} {}", step.name, step.args.join(" "));
        let status = cmd.status().map_err(|e| format!("run {shown}: {e}"))?;
        if !status.success() {
            eprintln!("ci: FAILED {} ({status})", step.name);
            return Ok(ExitCode::FAILURE);
        }
        println!("ci: ok {}", step.name);
    }
    println!("ci: all {} steps passed", STEPS.len());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_steps_are_named_once() {
        let mut names: Vec<&str> = STEPS.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), STEPS.len());
    }

    #[test]
    fn ci_reproducibility_precedes_the_tests_that_read_dist() {
        let at = |name: &str| STEPS.iter().position(|s| s.name == name).unwrap();
        assert!(at("release dist") < at("nextest"));
        assert!(at("tools venv") < at("standin python"));
        assert!(at("tools venv") < at("conformance python"));
        assert_eq!(STEPS.last().map(|s| s.name), Some("conformance python"));
    }
}

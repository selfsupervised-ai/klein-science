// provenance: original
//! The klein-science checkout that xtask acts on, resolved at run time.
//!
//! [`root`] is the one resolver: the git top level of the current directory, which must be a
//! klein-science workspace ([`check_workspace`]). Everything that reads or writes the checkout
//! goes through it: `rust-toolchain.toml`, `<target>/corpora`, `check no-corpus` and `ci`. It is
//! never the tree xtask was compiled from, so a binary run in another checkout acts on that
//! checkout, and anywhere else it fails closed.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::Res;
use crate::host;

/// The klein-science checkout of the current directory, resolved once per process.
pub fn root() -> Res<PathBuf> {
    static ROOT: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    ROOT.get_or_init(resolve).clone()
}

fn resolve() -> Res<PathBuf> {
    let cwd = env::current_dir().map_err(|e| format!("current directory: {e}"))?;
    let outside = |why: String| {
        format!(
            "{} is not inside a klein-science checkout: {why}",
            cwd.display()
        )
    };
    let out = host::git_output(&cwd, &["rev-parse", "--show-toplevel"]).map_err(outside)?;
    let top = String::from_utf8(out).map_err(|e| outside(format!("git rev-parse: {e}")))?;
    let top = top.trim_end_matches(['\n', '\r']);
    if top.is_empty() {
        return Err(outside("git names no work tree".to_owned()));
    }
    let top = PathBuf::from(top);
    check_workspace(&top)?;
    Ok(top)
}

/// Checks that `dir` is a klein-science workspace: it holds a `rust-toolchain.toml`, and a
/// `Cargo.toml` whose `[workspace] members` lists `xtask`. The error says what is missing.
pub fn check_workspace(dir: &Path) -> Res<()> {
    let not = |why: &str| format!("{} is not a klein-science workspace: {why}", dir.display());
    if !dir.join("rust-toolchain.toml").is_file() {
        return Err(not("it has no rust-toolchain.toml"));
    }
    let text =
        fs::read_to_string(dir.join("Cargo.toml")).map_err(|e| not(&format!("Cargo.toml: {e}")))?;
    let doc: toml::Table = toml::from_str(&text).map_err(|e| not(&format!("Cargo.toml: {e}")))?;
    let has_xtask = doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .is_some_and(|m| m.iter().any(|v| v.as_str() == Some("xtask")));
    if has_xtask {
        Ok(())
    } else {
        Err(not("its Cargo.toml has no `xtask` workspace member"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh scratch directory per test, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = env::temp_dir().join(format!("ksci-xtask-ws-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn write(&self, name: &str, text: &str) {
            fs::write(self.0.join(name), text).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn workspace_markers() {
        let s = Scratch::new("markers");
        let err = check_workspace(&s.0).unwrap_err();
        assert!(err.contains("no rust-toolchain.toml"), "{err}");
        s.write("rust-toolchain.toml", "[toolchain]\nchannel = \"1.99.0\"\n");
        let err = check_workspace(&s.0).unwrap_err();
        assert!(err.contains("Cargo.toml"), "{err}");
        s.write("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
        let err = check_workspace(&s.0).unwrap_err();
        assert!(err.contains("no `xtask` workspace member"), "{err}");
        s.write("Cargo.toml", "[package]\nname = \"xtask\"\n");
        assert!(check_workspace(&s.0).is_err());
        s.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\", \"xtask\"]\n",
        );
        check_workspace(&s.0).unwrap();
    }

    #[test]
    fn workspace_root_is_this_checkout() {
        // Unit tests run in xtask/, inside this checkout.
        let root = root().unwrap();
        assert!(root.is_absolute(), "{}", root.display());
        assert!(
            root.join("xtask")
                .join("src")
                .join("workspace.rs")
                .is_file()
        );
    }
}

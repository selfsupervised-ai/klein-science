// provenance: original
//! The one artifact-store client (D-01, D-05).
//!
//! Every blob is addressed by its SHA-256 and re-hashed with [`ksci_wire::sha256_hex`] after every
//! transfer. Bytes land in a per-process `.partial` file first and are renamed into place only after
//! the re-hash passes, so an interrupted or concurrent transfer never leaves an unverified blob.
//!
//! - Local cache: `$KSCI_CACHE` (default `$HOME/.cache/klein-science`), blobs at `blobs/sha256/<hex>`.
//! - Recipes (`ksci.recipe.v1`, TOML) pin each file of a corpus by digest; `store fetch` materialises
//!   them under `$KSCI_CORPORA/<name>/` (default `<target>/corpora/<name>/`).

use std::env;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use clap::Subcommand;
use serde::Deserialize;

use crate::Res;

/// `cargo xtask store ...`
#[derive(Subcommand)]
pub enum StoreCmd {
    /// Fetch every file of each recipe into the cache (re-hashed) and materialise it under the
    /// corpora directory.
    Fetch {
        /// Recipe files (`ksci.recipe.v1` TOML).
        #[arg(required = true)]
        recipes: Vec<PathBuf>,
    },
}

/// Runs one `store` command.
pub fn run(cmd: StoreCmd) -> Res<()> {
    let cache = Cache::from_env()?;
    match cmd {
        StoreCmd::Fetch { recipes } => {
            let corpora = corpora_dir();
            for recipe in &recipes {
                fetch_recipe(&cache, recipe, &corpora)?;
            }
            Ok(())
        }
    }
}

/// The recipe schema tag.
pub const RECIPE_SCHEMA: &str = "ksci.recipe.v1";

/// A fetch recipe: a named set of files, each pinned by SHA-256.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// Always [`RECIPE_SCHEMA`].
    pub schema: String,
    /// Directory name under the corpora directory.
    pub name: String,
    /// The pinned files.
    #[serde(rename = "file", default)]
    pub files: Vec<RecipeFile>,
}

/// One pinned file of a recipe.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeFile {
    /// Relative path under the recipe's directory.
    pub path: String,
    /// Lowercase-hex SHA-256 of the file's bytes.
    pub sha256: String,
    /// `https://` URL of the upstream bytes, if the file has a public origin.
    pub upstream: Option<String>,
}

impl Recipe {
    /// Parses and validates a recipe.
    pub fn parse(text: &str) -> Res<Recipe> {
        let recipe: Recipe = toml::from_str(text).map_err(|e| format!("recipe: {e}"))?;
        recipe.validate()?;
        Ok(recipe)
    }

    fn validate(&self) -> Res<()> {
        if self.schema != RECIPE_SCHEMA {
            return Err(format!(
                "recipe schema is {:?}, expected {RECIPE_SCHEMA:?}",
                self.schema
            ));
        }
        check_name(&self.name)?;
        if self.files.is_empty() {
            return Err(format!("recipe {} lists no files", self.name));
        }
        let mut seen = std::collections::BTreeSet::new();
        for file in &self.files {
            check_rel_path(&file.path)?;
            check_digest(&file.sha256)?;
            if let Some(url) = &file.upstream {
                check_upstream(url)?;
            }
            if !seen.insert(file.path.as_str()) {
                return Err(format!("recipe {} lists {} twice", self.name, file.path));
            }
        }
        Ok(())
    }
}

/// Accepts exactly 64 lowercase hex digits; cache file names derive only from these.
pub fn check_digest(hex: &str) -> Res<()> {
    let ok = hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if ok {
        Ok(())
    } else {
        Err(format!("not a lowercase-hex sha256: {hex:?}"))
    }
}

fn check_name(name: &str) -> Res<()> {
    let ok = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(format!("recipe name must match [a-z0-9_-]+: {name:?}"))
    }
}

/// A recipe path must be relative, made only of normal components (no `..`, `.`, root or prefix).
fn check_rel_path(path: &str) -> Res<()> {
    let p = Path::new(path);
    let ok = !path.is_empty()
        && !path.contains('\\')
        && p.components().all(|c| matches!(c, Component::Normal(_)));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "recipe path must be relative without `..`: {path:?}"
        ))
    }
}

fn check_upstream(url: &str) -> Res<()> {
    if url.starts_with("https://") && !url.contains(char::is_whitespace) {
        Ok(())
    } else {
        Err(format!("upstream must be an https:// URL: {url:?}"))
    }
}

/// Lowercase-hex SHA-256 of a file's bytes, through the one digest helper.
pub fn file_sha256(path: &Path) -> Res<String> {
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(ksci_wire::sha256_hex(&bytes))
}

/// The local content-addressed cache.
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    /// `$KSCI_CACHE`, or `$HOME/.cache/klein-science`.
    pub fn from_env() -> Res<Cache> {
        if let Some(dir) = env::var_os("KSCI_CACHE") {
            return Ok(Cache::at(PathBuf::from(dir)));
        }
        let home = env::var_os("HOME").ok_or("neither KSCI_CACHE nor HOME is set")?;
        Ok(Cache::at(
            PathBuf::from(home).join(".cache").join("klein-science"),
        ))
    }

    /// A cache rooted at `root`.
    pub fn at(root: PathBuf) -> Cache {
        Cache { root }
    }

    /// Where the blob with digest `hex` lives (it may not exist).
    pub fn blob_path(&self, hex: &str) -> PathBuf {
        self.root.join("blobs").join("sha256").join(hex)
    }

    /// A fresh per-process partial file for a transfer of `hex`.
    pub fn partial_path(&self, hex: &str) -> Res<PathBuf> {
        let tmp = self.root.join("tmp");
        fs::create_dir_all(&tmp).map_err(|e| format!("create {}: {e}", tmp.display()))?;
        Ok(tmp.join(format!("{hex}.partial.{}", std::process::id())))
    }

    /// A verified cache hit: the blob exists and re-hashes to `hex`. A blob that does not is
    /// removed, so the caller fetches it again.
    pub fn lookup(&self, hex: &str) -> Res<Option<PathBuf>> {
        check_digest(hex)?;
        let blob = self.blob_path(hex);
        if !blob.is_file() {
            return Ok(None);
        }
        let actual = file_sha256(&blob)?;
        if actual == hex {
            return Ok(Some(blob));
        }
        eprintln!("warning: cached blob sha256:{hex} re-hashes to sha256:{actual}; removed");
        fs::remove_file(&blob).map_err(|e| format!("remove {}: {e}", blob.display()))?;
        Ok(None)
    }

    /// Re-hashes `partial` and renames it to the blob for `hex`. On a mismatch the partial is
    /// deleted and the error names both digests.
    pub fn admit(&self, partial: &Path, hex: &str, from: &str) -> Res<PathBuf> {
        let blob = self.blob_path(hex);
        verify_and_rename(partial, hex, &blob, from)?;
        Ok(blob)
    }
}

/// Re-hashes `partial`; on a match renames it to `dest`, otherwise deletes it and fails.
fn verify_and_rename(partial: &Path, hex: &str, dest: &Path, from: &str) -> Res<()> {
    let actual = match file_sha256(partial) {
        Ok(actual) => actual,
        Err(e) => {
            let _ = fs::remove_file(partial);
            return Err(e);
        }
    };
    if actual != hex {
        let _ = fs::remove_file(partial);
        return Err(format!(
            "digest mismatch from {from}: expected sha256:{hex}, got sha256:{actual}; discarded"
        ));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    fs::rename(partial, dest).map_err(|e| format!("rename into {}: {e}", dest.display()))
}

/// Where a blob came from.
#[derive(Clone, Copy)]
enum Source {
    Cache,
    Upstream,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Source::Cache => "cache",
            Source::Upstream => "upstream",
        })
    }
}

/// The workspace root (the parent of `xtask/`).
pub fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap_or(manifest).to_path_buf()
}

/// `$CARGO_TARGET_DIR`, or `<workspace>/target`.
pub fn target_dir() -> PathBuf {
    match env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => workspace_root().join("target"),
    }
}

/// `$KSCI_CORPORA`, or `<target>/corpora`. Tests resolve the same directory.
pub fn corpora_dir() -> PathBuf {
    match env::var_os("KSCI_CORPORA") {
        Some(dir) => PathBuf::from(dir),
        None => target_dir().join("corpora"),
    }
}

fn fetch_recipe(cache: &Cache, recipe_path: &Path, corpora: &Path) -> Res<()> {
    let text = fs::read_to_string(recipe_path)
        .map_err(|e| format!("read {}: {e}", recipe_path.display()))?;
    let recipe = Recipe::parse(&text).map_err(|e| format!("{}: {e}", recipe_path.display()))?;
    let dir = corpora.join(&recipe.name);
    for file in &recipe.files {
        let (source, blob) = obtain(cache, file)?;
        let dest = dir.join(&file.path);
        let partial = dest.with_file_name(format!(
            "{}.partial.{}",
            dest.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
            std::process::id()
        ));
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        fs::copy(&blob, &partial).map_err(|e| format!("copy to {}: {e}", partial.display()))?;
        verify_and_rename(&partial, &file.sha256, &dest, "cache copy")?;
        println!("{source:<8} sha256:{} {}", file.sha256, dest.display());
    }
    Ok(())
}

/// Gets a recipe file's blob into the cache: a verified cache hit, else the upstream URL.
fn obtain(cache: &Cache, file: &RecipeFile) -> Res<(Source, PathBuf)> {
    if let Some(blob) = cache.lookup(&file.sha256)? {
        return Ok((Source::Cache, blob));
    }
    let Some(url) = &file.upstream else {
        return Err(format!(
            "sha256:{} ({}) is not in the cache and the recipe names no upstream",
            file.sha256, file.path
        ));
    };
    let partial = cache.partial_path(&file.sha256)?;
    if let Err(e) = download(url, &partial) {
        let _ = fs::remove_file(&partial);
        return Err(e);
    }
    Ok((Source::Upstream, cache.admit(&partial, &file.sha256, url)?))
}

/// HTTPS-only download (redirects included) with curl.
fn download(url: &str, out: &Path) -> Res<()> {
    let status = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--proto", "=https", "--proto-redir", "=https", "--tlsv1.2"])
        .arg("--output")
        .arg(out)
        .arg(url)
        .status()
        .map_err(|e| format!("run curl: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("curl {url}: {status}"))
    }
}

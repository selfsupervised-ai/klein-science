// provenance: original
//! The one artifact-store client (D-01, D-05).
//!
//! Every blob is addressed by its SHA-256 and re-hashed with [`ksci_wire::sha256_hex`] after every
//! transfer. Bytes land in a per-process `.partial` file first and are renamed into place only after
//! the re-hash passes, so an interrupted or concurrent transfer never leaves an unverified blob.
//!
//! - Local cache: `$KSCI_CACHE` (default `$HOME/.cache/klein-science`), blobs at `blobs/sha256/<hex>`.
//! - Remote: `$KSCI_STORE_REMOTE`, an OCI repository reached with the pinned `oras`
//!   (default `ghcr.io/selfsupervised-ai/klein-science-store`), or `oci-layout:<dir>` for offline
//!   use. Authentication is the operator's own `oras login`; xtask never reads, prints or passes a
//!   token. Pushing to the registry is outward-facing and needs the operator's go-ahead (D-05).
//! - Recipes (`ksci.recipe.v1`, TOML) pin each file of a corpus by digest; `store fetch` materialises
//!   them under `$KSCI_CORPORA/<name>/` (default `<target>/corpora/<name>/`).
//!
//! Fetch order for a blob: cache, then remote, then the recipe's upstream URL.

use std::env;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use clap::Subcommand;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Res;
use crate::host::tool_path;

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
    /// Copy a file into the cache (re-hashed) and print its sha256.
    Put {
        /// The file to store.
        file: PathBuf,
    },
    /// Write one blob to a path: from the cache, else the remote, re-hashed.
    Get {
        /// Lowercase-hex sha256 of the blob.
        sha256: String,
        /// Output path.
        out: PathBuf,
    },
    /// Push cached blobs to the remote, plus one manifest that references them as a set.
    Push {
        /// Lowercase-hex sha256 digests of cached blobs.
        #[arg(required = true)]
        digests: Vec<String>,
    },
}

/// Runs one `store` command.
pub fn run(cmd: StoreCmd) -> Res<()> {
    let cache = Cache::from_env()?;
    let remote = Remote::from_env();
    match cmd {
        StoreCmd::Fetch { recipes } => {
            let corpora = corpora_dir();
            for recipe in &recipes {
                fetch_recipe(&cache, &remote, recipe, &corpora)?;
            }
            Ok(())
        }
        StoreCmd::Put { file } => {
            println!("{}", put(&cache, &file)?);
            Ok(())
        }
        StoreCmd::Get { sha256, out } => {
            let (source, blob) = fetch_blob(&cache, &sha256, None, Some(&remote))?;
            copy_verified(&blob, &sha256, &out)?;
            println!("{source:<8} sha256:{sha256} {}", out.display());
            Ok(())
        }
        StoreCmd::Push { digests } => {
            let pushed = push(&cache, &remote, &digests)?;
            println!(
                "pushed {} blobs to {} as {} (manifest sha256:{})",
                pushed.blobs, remote, pushed.tag, pushed.manifest
            );
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

    /// The cache root (`heavy.lock` lives here).
    pub fn root(&self) -> &Path {
        &self.root
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A verified local cache hit.
    Cache,
    /// The remote OCI store.
    Remote,
    /// The recipe's upstream URL.
    Upstream,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Source::Cache => "cache",
            Source::Remote => "remote",
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

fn fetch_recipe(cache: &Cache, remote: &Remote, recipe_path: &Path, corpora: &Path) -> Res<()> {
    let text = fs::read_to_string(recipe_path)
        .map_err(|e| format!("read {}: {e}", recipe_path.display()))?;
    let recipe = Recipe::parse(&text).map_err(|e| format!("{}: {e}", recipe_path.display()))?;
    let dir = corpora.join(&recipe.name);
    for file in &recipe.files {
        let (source, blob) =
            fetch_blob(cache, &file.sha256, file.upstream.as_deref(), Some(remote))
                .map_err(|e| format!("{}: {e}", file.path))?;
        let dest = dir.join(&file.path);
        copy_verified(&blob, &file.sha256, &dest)?;
        println!("{source:<8} sha256:{} {}", file.sha256, dest.display());
    }
    Ok(())
}

/// Copies a cached blob to `dest` through a per-process partial file, re-hashed before the rename.
fn copy_verified(blob: &Path, hex: &str, dest: &Path) -> Res<()> {
    let name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let partial = dest.with_file_name(format!("{name}.partial.{}", std::process::id()));
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    fs::copy(blob, &partial).map_err(|e| format!("copy to {}: {e}", partial.display()))?;
    verify_and_rename(&partial, hex, dest, "cache copy")
}

/// Gets the blob `hex` into the cache, re-hashed: a verified cache hit, else the remote (when
/// given), else `upstream` (when given). A remote failure falls through to `upstream` only when
/// there is one. This is the one fetch path; recipes, `store get` and tool tarballs all use it.
pub fn fetch_blob(
    cache: &Cache,
    hex: &str,
    upstream: Option<&str>,
    remote: Option<&Remote>,
) -> Res<(Source, PathBuf)> {
    if let Some(blob) = cache.lookup(hex)? {
        return Ok((Source::Cache, blob));
    }
    let remote_error = match remote {
        None => None,
        Some(remote) => {
            let partial = cache.partial_path(hex)?;
            match remote.fetch(hex, &partial) {
                Ok(()) => {
                    let from = remote.to_string();
                    return Ok((Source::Remote, cache.admit(&partial, hex, &from)?));
                }
                Err(e) => {
                    let _ = fs::remove_file(&partial);
                    Some(e)
                }
            }
        }
    };
    let Some(url) = upstream else {
        return Err(match remote_error {
            Some(e) => format!("sha256:{hex} is not in the cache and the remote failed: {e}"),
            None => format!("sha256:{hex} is not in the cache and has no upstream"),
        });
    };
    if let Some(e) = remote_error {
        eprintln!("note: remote miss for sha256:{hex} ({e}); trying upstream");
    }
    let partial = cache.partial_path(hex)?;
    if let Err(e) = download(url, &partial) {
        let _ = fs::remove_file(&partial);
        return Err(e);
    }
    Ok((Source::Upstream, cache.admit(&partial, hex, url)?))
}

/// Copies `file` into the cache (re-hashed after the copy) and returns its sha256.
pub fn put(cache: &Cache, file: &Path) -> Res<String> {
    let hex = file_sha256(file)?;
    if cache.lookup(&hex)?.is_none() {
        let partial = cache.partial_path(&hex)?;
        fs::copy(file, &partial).map_err(|e| format!("copy {}: {e}", file.display()))?;
        cache.admit(&partial, &hex, &file.display().to_string())?;
    }
    Ok(hex)
}

/// The default remote store (D-05).
pub const DEFAULT_REMOTE: &str = "ghcr.io/selfsupervised-ai/klein-science-store";
/// `artifactType` of a store set manifest.
pub const SET_ARTIFACT_TYPE: &str = "application/vnd.klein-science.store.v1+json";
const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const EMPTY_MEDIA_TYPE: &str = "application/vnd.oci.empty.v1+json";
/// The OCI empty descriptor's content (`{}`).
const EMPTY_CONTENT: &[u8] = b"{}";

/// The remote OCI store: a registry repository or a local OCI image layout.
pub enum Remote {
    /// A registry repository such as `ghcr.io/org/repo`.
    Registry(String),
    /// An OCI image layout directory (`oci-layout:<dir>`), for offline tests.
    Layout(PathBuf),
}

impl fmt::Display for Remote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Remote::Registry(repo) => f.write_str(repo),
            Remote::Layout(dir) => write!(f, "oci-layout:{}", dir.display()),
        }
    }
}

/// The result of [`push`].
pub struct Pushed {
    /// Number of data blobs pushed.
    pub blobs: usize,
    /// The set tag, `set-<16 hex>`.
    pub tag: String,
    /// sha256 of the set manifest.
    pub manifest: String,
}

impl Remote {
    /// `$KSCI_STORE_REMOTE`, or [`DEFAULT_REMOTE`].
    pub fn from_env() -> Remote {
        let spec = env::var("KSCI_STORE_REMOTE").unwrap_or_else(|_| DEFAULT_REMOTE.to_owned());
        Remote::parse(&spec)
    }

    /// `oci-layout:<dir>` or a registry repository.
    pub fn parse(spec: &str) -> Remote {
        match spec.strip_prefix("oci-layout:") {
            Some(dir) => Remote::Layout(PathBuf::from(dir)),
            None => Remote::Registry(spec.to_owned()),
        }
    }

    /// The oras target reference, with a `@sha256:<hex>` or `:<tag>` suffix (or none).
    fn target(&self, suffix: &str) -> String {
        match self {
            Remote::Registry(repo) => format!("{repo}{suffix}"),
            Remote::Layout(dir) => format!("{}{suffix}", dir.display()),
        }
    }

    /// Runs the pinned oras (with `--oci-layout` for a layout remote) and returns its stdout.
    fn oras(&self, args: &[&str], target: &str, file: Option<&Path>) -> Res<String> {
        let out = self.oras_command(args, target, file)?.output();
        let out = out.map_err(|e| format!("run oras: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("oras {}: {}", args.join(" "), err.trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// The pinned oras invocation, with `--oci-layout` for a layout remote.
    fn oras_command(&self, args: &[&str], target: &str, file: Option<&Path>) -> Res<Command> {
        let oras = tool_path("oras")?;
        if !oras.is_file() {
            return Err(format!(
                "{} is missing; run: cargo xtask tools install",
                oras.display()
            ));
        }
        let mut cmd = Command::new(&oras);
        cmd.args(args);
        if let Remote::Layout(_) = self {
            cmd.arg("--oci-layout");
        }
        cmd.arg(target);
        if let Some(file) = file {
            cmd.arg(file);
        }
        Ok(cmd)
    }

    /// Fetches blob `hex` into `out`, unverified: the caller re-hashes it before admitting it.
    ///
    /// oras streams the blob to stdout, which goes straight into `out`. If oras rejects the bytes
    /// (it checks digests too), `out` still holds what the remote delivered, and the error names
    /// both the expected digest and the digest of those bytes.
    fn fetch(&self, hex: &str, out: &Path) -> Res<()> {
        let sink = fs::File::create(out).map_err(|e| format!("create {}: {e}", out.display()))?;
        let target = self.target(&format!("@sha256:{hex}"));
        let mut cmd = self.oras_command(&["blob", "fetch", "--output", "-"], &target, None)?;
        let result = cmd
            .stdout(sink)
            .output()
            .map_err(|e| format!("run oras: {e}"))?;
        if result.status.success() {
            return Ok(());
        }
        let err = String::from_utf8_lossy(&result.stderr);
        let delivered = fs::metadata(out).map(|m| m.len()).unwrap_or(0);
        if delivered == 0 {
            return Err(format!("oras blob fetch: {}", err.trim()));
        }
        Err(format!(
            "digest mismatch from {self}: expected sha256:{hex}, got sha256:{} ({}); discarded",
            file_sha256(out)?,
            err.trim()
        ))
    }

    /// Pushes one file and checks that the digest and size oras reports match the local ones.
    fn push_blob(&self, hex: &str, file: &Path) -> Res<u64> {
        let size = fs::metadata(file)
            .map_err(|e| format!("stat {}: {e}", file.display()))?
            .len();
        let out = self.oras(
            &["blob", "push", "--descriptor"],
            &self.target(""),
            Some(file),
        )?;
        let desc: Value =
            serde_json::from_str(&out).map_err(|e| format!("oras descriptor: {e}"))?;
        let want = format!("sha256:{hex}");
        if desc["digest"] != want.as_str() || desc["size"] != size {
            return Err(format!(
                "remote reports {} ({} bytes) for local {want} ({size} bytes)",
                desc["digest"], desc["size"]
            ));
        }
        Ok(size)
    }
}

/// The set tag for a digest list: `set-` and the first 16 hex of the sha256 of the JCS encoding of
/// the sorted, de-duplicated list of lowercase-hex digests.
pub fn set_tag(digests: &[String]) -> Res<String> {
    let mut sorted = digests.to_vec();
    sorted.sort();
    sorted.dedup();
    let bytes = ksci_wire::to_jcs(&sorted).map_err(|e| e.to_string())?;
    Ok(format!("set-{}", &ksci_wire::sha256_hex(&bytes)[..16]))
}

/// Pushes the cached blobs `digests`, then one OCI image manifest that lists them as layers and
/// is tagged with [`set_tag`]. The manifest keeps the blobs referenced, so the registry does not
/// garbage-collect them. Pushing the same set twice yields the same tag and manifest.
pub fn push(cache: &Cache, remote: &Remote, digests: &[String]) -> Res<Pushed> {
    let mut sorted = digests.to_vec();
    sorted.sort();
    sorted.dedup();
    let mut layers = Vec::with_capacity(sorted.len());
    for hex in &sorted {
        check_digest(hex)?;
        let blob = cache.lookup(hex)?.ok_or_else(|| {
            format!("sha256:{hex} is not in the cache; run: cargo xtask store put <file>")
        })?;
        let size = remote.push_blob(hex, &blob)?;
        layers.push(json!({
            "mediaType": "application/octet-stream",
            "digest": format!("sha256:{hex}"),
            "size": size,
        }));
    }

    let empty_hex = ksci_wire::sha256_hex(EMPTY_CONTENT);
    let empty = write_scratch(cache, &empty_hex, EMPTY_CONTENT)?;
    let pushed_empty = remote.push_blob(&empty_hex, &empty);
    let _ = fs::remove_file(&empty);
    pushed_empty?;

    let manifest = json!({
        "schemaVersion": 2,
        "mediaType": MANIFEST_MEDIA_TYPE,
        "artifactType": SET_ARTIFACT_TYPE,
        "config": {
            "mediaType": EMPTY_MEDIA_TYPE,
            "digest": format!("sha256:{empty_hex}"),
            "size": EMPTY_CONTENT.len(),
        },
        "layers": layers,
    });
    let bytes = ksci_wire::to_jcs(&manifest).map_err(|e| e.to_string())?;
    let manifest_hex = ksci_wire::sha256_hex(&bytes);
    let tag = set_tag(&sorted)?;
    let file = write_scratch(cache, &manifest_hex, &bytes)?;
    let out = remote.oras(
        &[
            "manifest",
            "push",
            "--descriptor",
            "--media-type",
            MANIFEST_MEDIA_TYPE,
        ],
        &remote.target(&format!(":{tag}")),
        Some(&file),
    );
    let _ = fs::remove_file(&file);
    let desc: Value = serde_json::from_str(&out?).map_err(|e| format!("oras descriptor: {e}"))?;
    let want = format!("sha256:{manifest_hex}");
    if desc["digest"] != want.as_str() {
        return Err(format!(
            "remote reports manifest {} for local {want}",
            desc["digest"]
        ));
    }
    Ok(Pushed {
        blobs: sorted.len(),
        tag,
        manifest: manifest_hex,
    })
}

/// Writes `bytes` to a per-process scratch file in the cache.
fn write_scratch(cache: &Cache, hex: &str, bytes: &[u8]) -> Res<PathBuf> {
    let path = cache.partial_path(hex)?;
    fs::write(&path, bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh scratch directory per test, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = env::temp_dir().join(format!("ksci-xtask-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn cache(&self, name: &str) -> Cache {
            Cache::at(self.0.join(name))
        }

        fn layout(&self) -> Remote {
            Remote::Layout(self.0.join("layout"))
        }

        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, bytes).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// The oras-dependent tests skip, saying why, only when the pinned oras is not installed;
    /// `cargo xtask tools check` is the guard that it is.
    fn oras_installed() -> bool {
        let ok = tool_path("oras").map(|p| p.is_file()).unwrap_or(false);
        if !ok {
            eprintln!("skipped: pinned oras is not installed; run: cargo xtask tools install");
        }
        ok
    }

    #[test]
    fn store_put_push_fetch_roundtrip() {
        if !oras_installed() {
            return;
        }
        let s = Scratch::new("roundtrip");
        let (a, b, remote) = (s.cache("a"), s.cache("b"), s.layout());
        let hex = put(&a, &s.file("x.bin", b"roundtrip bytes\n")).unwrap();
        assert_eq!(hex, ksci_wire::sha256_hex(b"roundtrip bytes\n"));
        push(&a, &remote, std::slice::from_ref(&hex)).unwrap();

        let (source, blob) = fetch_blob(&b, &hex, None, Some(&remote)).unwrap();
        assert_eq!(source, Source::Remote);
        assert_eq!(fs::read(&blob).unwrap(), b"roundtrip bytes\n");
        let (again, _) = fetch_blob(&b, &hex, None, Some(&remote)).unwrap();
        assert_eq!(again, Source::Cache);
    }

    #[test]
    fn store_push_twice_same_digest() {
        if !oras_installed() {
            return;
        }
        let s = Scratch::new("twice");
        let (cache, remote) = (s.cache("c"), s.layout());
        let x = put(&cache, &s.file("x", b"x")).unwrap();
        let y = put(&cache, &s.file("y", b"y")).unwrap();
        let first = push(&cache, &remote, &[x.clone(), y.clone()]).unwrap();
        let second = push(&cache, &remote, &[y, x]).unwrap();
        assert_eq!(first.tag, second.tag);
        assert_eq!(first.manifest, second.manifest);
        assert_eq!(first.blobs, 2);
    }

    #[test]
    fn store_push_writes_set_manifest() {
        if !oras_installed() {
            return;
        }
        let s = Scratch::new("manifest");
        let (cache, remote) = (s.cache("c"), s.layout());
        let mut digests: Vec<String> = ["one", "two", "three"]
            .iter()
            .map(|n| put(&cache, &s.file(n, n.as_bytes())).unwrap())
            .collect();
        let pushed = push(&cache, &remote, &digests).unwrap();

        let index: Value =
            serde_json::from_slice(&fs::read(s.0.join("layout/index.json")).unwrap()).unwrap();
        let entry = index["manifests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["annotations"]["org.opencontainers.image.ref.name"] == pushed.tag.as_str())
            .expect("the set tag is in index.json");
        assert_eq!(
            entry["digest"],
            format!("sha256:{}", pushed.manifest).as_str()
        );

        let bytes = fs::read(s.0.join("layout/blobs/sha256").join(&pushed.manifest)).unwrap();
        assert_eq!(ksci_wire::sha256_hex(&bytes), pushed.manifest);
        let manifest: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(manifest["artifactType"], SET_ARTIFACT_TYPE);
        assert_eq!(manifest["config"]["mediaType"], EMPTY_MEDIA_TYPE);
        let layers: Vec<String> = manifest["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                assert_eq!(l["mediaType"], "application/octet-stream");
                l["digest"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("sha256:")
                    .unwrap()
                    .to_owned()
            })
            .collect();
        digests.sort();
        assert_eq!(layers, digests);
        assert_eq!(pushed.tag, set_tag(&digests).unwrap());
    }

    #[test]
    fn store_fetch_rejects_tampered_blob() {
        if !oras_installed() {
            return;
        }
        let s = Scratch::new("tampered");
        let (a, b, remote) = (s.cache("a"), s.cache("b"), s.layout());
        let hex = put(&a, &s.file("x", b"genuine")).unwrap();
        push(&a, &remote, std::slice::from_ref(&hex)).unwrap();
        // oras writes layout blobs read-only; replace the file rather than writing through it.
        let stored = s.0.join("layout/blobs/sha256").join(&hex);
        fs::remove_file(&stored).unwrap();
        fs::write(&stored, b"tampered").unwrap();

        let err = fetch_blob(&b, &hex, None, Some(&remote)).unwrap_err();
        let bad = ksci_wire::sha256_hex(b"tampered");
        assert!(
            err.contains(&hex) && err.contains(&bad),
            "error must name both digests: {err}"
        );
        assert!(b.lookup(&hex).unwrap().is_none(), "no blob was admitted");
        let leftovers = fs::read_dir(b.root().join("tmp"))
            .map(|d| d.count())
            .unwrap_or(0);
        assert_eq!(leftovers, 0, "no partial file is left behind");
    }

    fn recipe(path: &str, upstream: &str) -> String {
        format!(
            "schema = \"ksci.recipe.v1\"\nname = \"t\"\n[[file]]\npath = \"{path}\"\nsha256 = \"{}\"\nupstream = \"{upstream}\"\n",
            "a".repeat(64)
        )
    }

    #[test]
    fn store_recipe_rejects_traversal() {
        assert!(Recipe::parse(&recipe("ok/x.json", "https://example.org/x")).is_ok());
        for bad in ["../x", "/etc/x", "a/../../x", "./x", "", "a\\\\b"] {
            let err = Recipe::parse(&recipe(bad, "https://example.org/x")).unwrap_err();
            assert!(err.contains("relative"), "{bad:?}: {err}");
        }
    }

    #[test]
    fn store_recipe_rejects_http() {
        let err = Recipe::parse(&recipe("x", "http://example.org/x")).unwrap_err();
        assert!(err.contains("https://"), "{err}");
        let err = Recipe::parse(&recipe("x", "file:///etc/passwd")).unwrap_err();
        assert!(err.contains("https://"), "{err}");
    }
}

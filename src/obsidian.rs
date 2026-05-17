//! Obsidian launch and vault discovery.
//!
//! Obsidian opens content via the `obsidian://` URI scheme rather than a normal
//! CLI. This module reads Obsidian's own config to discover vaults, then assembles
//! the right URI based on what `path` it's handed:
//!
//! - vault root → `obsidian://open?vault=<Name>`
//! - file inside a vault → `obsidian://open?vault=<Name>&file=<relative>`
//! - anything else → `obsidian://open?path=<absolute>` (Obsidian decides)

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// An Obsidian vault, as registered in Obsidian's config.
///
/// Crate-private: "vault" is Obsidian-domain terminology and intentionally
/// absent from path-opener's public vocabulary.
#[derive(Debug, Clone)]
pub(crate) struct Vault {
    /// Internal ID Obsidian assigns. Stable across vault renames.
    #[allow(dead_code)]
    pub(crate) id: String,
    /// Vault display name — basename of `path`. This is what `vault=` in URIs expects.
    pub(crate) name: String,
    /// Absolute path to the vault root directory.
    pub(crate) path: PathBuf,
}

#[derive(Deserialize)]
struct ObsidianConfig {
    #[serde(default)]
    vaults: HashMap<String, ConfigVault>,
}

#[derive(Deserialize)]
struct ConfigVault {
    path: PathBuf,
}

fn config_file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("obsidian").join("obsidian.json"))
}

/// List vaults Obsidian knows about on this machine.
///
/// Crate-private: callers go through `open(path, "obsidian")`, which routes
/// internally to [`build_command`] — they never see vault metadata directly.
///
/// Reads `obsidian.json` from the user-config directory:
/// - macOS: `~/Library/Application Support/obsidian/obsidian.json`
/// - Linux: `~/.config/obsidian/obsidian.json`
/// - Windows: `%APPDATA%\obsidian\obsidian.json`
///
/// Returns an empty `Vec` if Obsidian isn't installed, has never been launched,
/// or `obsidian.json` is missing/unreadable/malformed.
pub(crate) fn discover_vaults() -> Vec<Vault> {
    let Some(path) = config_file() else { return Vec::new() };
    let Ok(bytes) = std::fs::read(&path) else { return Vec::new() };
    let Ok(cfg) = serde_json::from_slice::<ObsidianConfig>(&bytes) else { return Vec::new() };

    cfg.vaults
        .into_iter()
        .filter_map(|(id, v)| {
            let name = v.path.file_name()?.to_str()?.to_string();
            Some(Vault { id, name, path: v.path })
        })
        .collect()
}

/// Build a `Command` that opens `path` in Obsidian via the `obsidian://` URI scheme.
pub(crate) fn build_command(path: &Path) -> io::Result<Command> {
    let uri = build_uri(path, &discover_vaults())?;
    Ok(uri_launcher(&uri))
}

/// Pure URI-building logic, factored out for testability.
///
/// Walks the strategy ladder:
/// 1. `path` matches a known vault root → `obsidian://open?vault=<Name>`
/// 2. `path` lives inside a known vault → `obsidian://open?vault=<Name>&file=<rel>`
/// 3. Otherwise → `obsidian://open?path=<abs>`
fn build_uri(path: &Path, vaults: &[Vault]) -> io::Result<String> {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    if let Some(v) = vaults.iter().find(|v| v.path == abs) {
        return Ok(format!("obsidian://open?vault={}", encode(&v.name)));
    }

    if let Some((v, rel)) = vaults.iter().find_map(|v| abs.strip_prefix(&v.path).ok().map(|r| (v, r))) {
        let rel_str = rel.to_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "non-utf8 path"))?;
        return Ok(format!("obsidian://open?vault={}&file={}", encode(&v.name), encode(rel_str)));
    }

    let abs_str = abs.to_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "non-utf8 path"))?;
    Ok(format!("obsidian://open?path={}", encode(abs_str)))
}

#[cfg(target_os = "macos")]
fn uri_launcher(uri: &str) -> Command {
    let mut c = Command::new("open");
    c.arg(uri);
    c
}

#[cfg(target_os = "linux")]
fn uri_launcher(uri: &str) -> Command {
    let mut c = Command::new("xdg-open");
    c.arg(uri);
    c
}

#[cfg(target_os = "windows")]
fn uri_launcher(uri: &str) -> Command {
    // `start` is a cmd builtin; the empty "" fills the title slot so a quoted URI isn't mis-parsed.
    let mut c = Command::new("cmd");
    c.args(["/C", "start", "", uri]);
    c
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn uri_launcher(uri: &str) -> Command {
    let _ = uri;
    Command::new("false")
}

fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*b as char),
            other => out.push_str(&format!("%{:02X}", other)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    #[test]
    fn encode_keeps_unreserved() {
        assert_eq!(encode("hello"), "hello");
        assert_eq!(encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn encode_percent_escapes_others() {
        assert_eq!(encode("hello world"), "hello%20world");
        assert_eq!(encode("a/b.md"), "a%2Fb.md");
        assert_eq!(encode("café"), "caf%C3%A9");
    }

    #[test]
    fn discover_does_not_panic() {
        let _ = discover_vaults();
    }

    #[test]
    fn parse_handles_missing_unreadable_malformed_inputs() {
        // discover_vaults swallows three failure modes into an empty Vec:
        //   1. missing config file        (let Ok(bytes) = read(&path))
        //   2. unreadable bytes           (same branch)
        //   3. malformed JSON             (let Ok(cfg) = serde_json::from_slice)
        //
        // (3) is the only one we can exercise without writing to the real
        // user-config dir; the parse branch is the most fragile of the three.
        let bad = b"this is not json at all";
        let result: Result<ObsidianConfig, _> = serde_json::from_slice(bad);
        assert!(result.is_err(), "garbage bytes must fail to parse");

        let empty = b"";
        let result: Result<ObsidianConfig, _> = serde_json::from_slice(empty);
        assert!(result.is_err(), "empty bytes must fail to parse");

        // A valid JSON object with no `vaults` field is fine — serde defaults
        // give us an empty map, which discover_vaults reports as no vaults.
        let no_vaults = br#"{"other_field": 42}"#;
        let cfg: ObsidianConfig = serde_json::from_slice(no_vaults).expect("parses");
        assert!(cfg.vaults.is_empty());
    }

    /// Helper: build a real on-disk directory so `canonicalize()` resolves
    /// without surprises, and return the canonical path. Tests using this
    /// must hold the returned `tempfile::TempDir` for the duration of the test.
    fn make_real_dir(parent: &Path, name: &str) -> PathBuf {
        let p = parent.join(name);
        fs::create_dir_all(&p).expect("create vault dir");
        p.canonicalize().expect("canonicalize")
    }

    fn make_real_file(parent: &Path, rel: &str) -> PathBuf {
        let p = parent.join(rel);
        if let Some(parent_dir) = p.parent() {
            fs::create_dir_all(parent_dir).expect("create parent");
        }
        fs::write(&p, "").expect("create file");
        p.canonicalize().expect("canonicalize file")
    }

    #[test]
    fn uri_for_vault_root_uses_vault_query_only() {
        let tmp = env::temp_dir().join(format!("path-opener-test-vault-root-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let vault_dir = make_real_dir(&tmp, "MyVault");

        let vaults = vec![Vault { id: "v1".into(), name: "MyVault".into(), path: vault_dir.clone() }];

        let uri = build_uri(&vault_dir, &vaults).expect("build_uri");
        assert_eq!(uri, "obsidian://open?vault=MyVault");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn uri_for_file_inside_vault_uses_vault_and_file() {
        let tmp = env::temp_dir().join(format!("path-opener-test-vault-file-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let vault_dir = make_real_dir(&tmp, "Notes");
        let file = make_real_file(&vault_dir, "sub/note.md");

        let vaults = vec![Vault { id: "v1".into(), name: "Notes".into(), path: vault_dir.clone() }];

        let uri = build_uri(&file, &vaults).expect("build_uri");
        assert_eq!(uri, "obsidian://open?vault=Notes&file=sub%2Fnote.md");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn uri_for_path_outside_any_vault_falls_through_to_path_query() {
        let tmp = env::temp_dir().join(format!("path-opener-test-outside-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let vault_dir = make_real_dir(&tmp, "Registered");
        let other_dir = make_real_dir(&tmp, "Outside");

        let vaults = vec![Vault { id: "v1".into(), name: "Registered".into(), path: vault_dir.clone() }];

        let uri = build_uri(&other_dir, &vaults).expect("build_uri");
        assert!(uri.starts_with("obsidian://open?path="), "got: {uri}");
        assert!(uri.contains("Outside"), "should contain the path: {uri}");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn uri_with_no_known_vaults_falls_through_to_path_query() {
        // Equivalent to obsidian.json missing entirely — discover_vaults() returns [].
        let tmp = env::temp_dir().join(format!("path-opener-test-no-vaults-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let dir = make_real_dir(&tmp, "Anywhere");

        let vaults: Vec<Vault> = vec![];

        let uri = build_uri(&dir, &vaults).expect("build_uri");
        assert!(uri.starts_with("obsidian://open?path="), "got: {uri}");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_command_does_not_panic_on_missing_path() {
        // The path doesn't have to exist — canonicalize falls back to the input.
        let uri = build_uri(Path::new("/definitely/does/not/exist"), &[]).expect("build_uri");
        assert!(uri.starts_with("obsidian://open?path="));
    }
}

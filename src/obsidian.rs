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
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let vaults = discover_vaults();

    let uri = if let Some(v) = vaults.iter().find(|v| v.path == abs) {
        format!("obsidian://open?vault={}", encode(&v.name))
    } else if let Some((v, rel)) = vaults.iter().find_map(|v| abs.strip_prefix(&v.path).ok().map(|r| (v, r))) {
        let rel_str = rel.to_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "non-utf8 path"))?;
        format!("obsidian://open?vault={}&file={}", encode(&v.name), encode(rel_str))
    } else {
        let abs_str = abs.to_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "non-utf8 path"))?;
        format!("obsidian://open?path={}", encode(abs_str))
    };

    Ok(uri_launcher(&uri))
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
}

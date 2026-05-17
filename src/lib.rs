//! Detect installed apps and open file paths with them, cross-platform.
//!
//! `path-opener` scans your system for known editors, terminals, file managers,
//! and Markdown apps, then lets you launch any of them on a given path. It
//! handles macOS `.app` bundles, PATH lookups on Linux/Windows, and the
//! platform-native "just open it" command.
//!
//! Most apps launch with a simple `command + path` shell call. A few — notably
//! Obsidian — need app-specific launch logic (URI schemes, vault lookup). For
//! those, prefer [`open_with`] over [`open_path`].
//!
//! ```rust
//! use std::path::Path;
//! use path_opener::{detect_installed_apps, open_with, open_default};
//!
//! // See what's installed
//! let apps = detect_installed_apps();
//! for app in &apps {
//!     if app.is_available {
//!         println!("{} ({})", app.name, app.command);
//!     }
//! }
//!
//! // Open a path with a detected app — honors per-app launch quirks
//! // (e.g. Obsidian's URI scheme + vault discovery).
//! // if let Some(vscode) = apps.iter().find(|a| a.app_id == "vscode" && a.is_available) {
//! //     open_with(vscode, Path::new("/my/project")).unwrap();
//! // }
//!
//! // Or just use the system default
//! // open_default("/my/project").unwrap();
//! ```
//!
//! Apps with non-CLI launch (Obsidian's `obsidian://` URI scheme, etc.) are
//! dispatched internally — callers just hand path-opener a `(path, app_id)` pair.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;
use std::process::Command;

pub(crate) mod obsidian;

/// An app that can open file/directory paths.
///
/// You get these from [`detect_installed_apps`]. Each one tells you the app's
/// name, the shell command to invoke it, and whether it's actually installed.
/// The `is_default`, `is_hidden`, and `sort_order` fields are there for you to
/// manage user preferences on top — they always start as false/None.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PathOpener {
    /// Short stable ID like "vscode", "finder", "terminal".
    pub app_id: String,
    /// Human-friendly name, e.g. "Visual Studio Code".
    pub name: String,
    /// Shell command to open a path with this app.
    pub command: String,
    /// `true` if we detected it on this machine.
    pub is_available: bool,
    /// For your UI — mark one as the user's preferred default.
    pub is_default: bool,
    /// For your UI — let users hide openers they don't care about.
    pub is_hidden: bool,
    /// For your UI — custom sort position.
    pub sort_order: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Os {
    MacOS,
    Linux,
    Windows,
}

// How we figure out if something's installed.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum Detection {
    AlwaysAvailable,            // Ships with the OS (Finder, Explorer)
    MacAppBundle(&'static str), // Look for Foo.app in /Applications
    PathLookup,                 // `which`/`where` on PATH
}

#[derive(Debug, Clone)]
struct PlatformEntry {
    os: Os,
    command: &'static str,
    detection: Detection,
}

#[derive(Debug, Clone)]
struct KnownApp {
    app_id: &'static str,
    name: &'static str,
    platforms: &'static [PlatformEntry],
    launch: Launch,
}

// How `open_with` should turn an opener + path into a Command.
#[derive(Debug, Clone, Copy)]
enum Launch {
    // Default: split the platform's `command` on whitespace, append path as last arg.
    Argv,
    // Custom builder for apps that need more than argv-append (URI schemes, vault lookup, etc.).
    Custom(fn(&Path) -> io::Result<Command>),
}

// Shorthand for apps that use the same command on every OS and are found via PATH.
macro_rules! cross_platform_app {
    ($id:expr, $name:expr, $cmd:expr) => {
        KnownApp {
            app_id: $id,
            name: $name,
            platforms: &[
                PlatformEntry { os: Os::MacOS, command: $cmd, detection: Detection::PathLookup },
                PlatformEntry { os: Os::Linux, command: $cmd, detection: Detection::PathLookup },
                PlatformEntry { os: Os::Windows, command: $cmd, detection: Detection::PathLookup },
            ],
            launch: Launch::Argv,
        }
    };
}

// Same thing but with a macOS .app bundle check instead of PATH on mac.
macro_rules! cross_platform_app_with_mac_bundle {
    ($id:expr, $name:expr, $cmd:expr, $bundle:expr) => {
        KnownApp {
            app_id: $id,
            name: $name,
            platforms: &[
                PlatformEntry { os: Os::MacOS, command: $cmd, detection: Detection::MacAppBundle($bundle) },
                PlatformEntry { os: Os::Linux, command: $cmd, detection: Detection::PathLookup },
                PlatformEntry { os: Os::Windows, command: $cmd, detection: Detection::PathLookup },
            ],
            launch: Launch::Argv,
        }
    };
}

const KNOWN_APPS: &[KnownApp] = &[
    // -- File managers / system default --
    KnownApp {
        app_id: "finder",
        name: "Finder",
        platforms: &[PlatformEntry { os: Os::MacOS, command: "open", detection: Detection::AlwaysAvailable }],
        launch: Launch::Argv,
    },
    KnownApp {
        app_id: "file-manager",
        name: "File Manager",
        platforms: &[PlatformEntry { os: Os::Linux, command: "xdg-open", detection: Detection::PathLookup }],
        launch: Launch::Argv,
    },
    KnownApp {
        app_id: "explorer",
        name: "Explorer",
        platforms: &[PlatformEntry { os: Os::Windows, command: "explorer", detection: Detection::AlwaysAvailable }],
        launch: Launch::Argv,
    },
    // -- Terminals --
    KnownApp {
        app_id: "terminal",
        name: "Terminal",
        platforms: &[PlatformEntry {
            os: Os::MacOS,
            command: "open -a Terminal",
            detection: Detection::MacAppBundle("Terminal.app"),
        }],
        launch: Launch::Argv,
    },
    KnownApp {
        app_id: "iterm",
        name: "iTerm",
        platforms: &[PlatformEntry {
            os: Os::MacOS,
            command: "open -a iTerm",
            detection: Detection::MacAppBundle("iTerm.app"),
        }],
        launch: Launch::Argv,
    },
    KnownApp {
        app_id: "gnome-terminal",
        name: "GNOME Terminal",
        platforms: &[PlatformEntry { os: Os::Linux, command: "gnome-terminal", detection: Detection::PathLookup }],
        launch: Launch::Argv,
    },
    KnownApp {
        app_id: "konsole",
        name: "Konsole",
        platforms: &[PlatformEntry { os: Os::Linux, command: "konsole", detection: Detection::PathLookup }],
        launch: Launch::Argv,
    },
    KnownApp {
        app_id: "alacritty",
        name: "Alacritty",
        platforms: &[
            PlatformEntry {
                os: Os::MacOS,
                command: "open -a Alacritty",
                detection: Detection::MacAppBundle("Alacritty.app"),
            },
            PlatformEntry { os: Os::Linux, command: "alacritty", detection: Detection::PathLookup },
            PlatformEntry { os: Os::Windows, command: "alacritty", detection: Detection::PathLookup },
        ],
        launch: Launch::Argv,
    },
    KnownApp {
        app_id: "kitty",
        name: "Kitty",
        platforms: &[
            PlatformEntry { os: Os::MacOS, command: "open -a Kitty", detection: Detection::MacAppBundle("kitty.app") },
            PlatformEntry { os: Os::Linux, command: "kitty", detection: Detection::PathLookup },
        ],
        launch: Launch::Argv,
    },
    KnownApp {
        app_id: "windows-terminal",
        name: "Windows Terminal",
        platforms: &[PlatformEntry { os: Os::Windows, command: "wt", detection: Detection::PathLookup }],
        launch: Launch::Argv,
    },
    KnownApp {
        app_id: "powershell",
        name: "PowerShell",
        platforms: &[PlatformEntry { os: Os::Windows, command: "pwsh", detection: Detection::PathLookup }],
        launch: Launch::Argv,
    },
    // -- Editors (cross-platform) --
    cross_platform_app_with_mac_bundle!("vscode", "Visual Studio Code", "code", "Visual Studio Code.app"),
    cross_platform_app_with_mac_bundle!("cursor", "Cursor", "cursor", "Cursor.app"),
    cross_platform_app_with_mac_bundle!("sublime-text", "Sublime Text", "subl", "Sublime Text.app"),
    cross_platform_app_with_mac_bundle!("zed", "Zed", "zed", "Zed.app"),
    cross_platform_app!("neovim", "Neovim", "nvim"),
    cross_platform_app!("webstorm", "WebStorm", "webstorm"),
    cross_platform_app!("intellij", "IntelliJ IDEA", "idea"),
    // -- Markdown --
    KnownApp {
        app_id: "obsidian",
        name: "Obsidian",
        platforms: &[
            PlatformEntry {
                os: Os::MacOS,
                command: "open -a Obsidian",
                detection: Detection::MacAppBundle("Obsidian.app"),
            },
            PlatformEntry { os: Os::Linux, command: "obsidian", detection: Detection::PathLookup },
            PlatformEntry { os: Os::Windows, command: "obsidian", detection: Detection::PathLookup },
        ],
        launch: Launch::Custom(obsidian::build_command),
    },
];

fn current_os() -> Option<Os> {
    if cfg!(target_os = "macos") {
        Some(Os::MacOS)
    } else if cfg!(target_os = "linux") {
        Some(Os::Linux)
    } else if cfg!(target_os = "windows") {
        Some(Os::Windows)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn is_macos_app_installed(bundle_name: &str) -> bool {
    let system_path = std::path::Path::new("/Applications").join(bundle_name);
    if system_path.exists() {
        return true;
    }
    if let Some(home) = dirs::home_dir() {
        let user_path = home.join("Applications").join(bundle_name);
        if user_path.exists() {
            return true;
        }
    }
    false
}

fn is_command_available(command: &str) -> bool {
    let binary = command.split_whitespace().next().unwrap_or(command);

    #[cfg(unix)]
    {
        std::process::Command::new("which")
            .arg(binary)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    #[cfg(windows)]
    {
        std::process::Command::new("where")
            .arg(binary)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = binary;
        false
    }
}

fn check_availability(detection: &Detection) -> bool {
    match detection {
        Detection::AlwaysAvailable => true,
        #[cfg(target_os = "macos")]
        Detection::MacAppBundle(bundle) => is_macos_app_installed(bundle),
        #[cfg(not(target_os = "macos"))]
        Detection::MacAppBundle(_) => false,
        Detection::PathLookup => false, // resolved per-entry in detect_installed_apps
    }
}

/// Scan the system and return every known app for this platform.
///
/// Each result tells you whether the app is actually installed (`is_available`).
/// You'll get entries for editors, terminals, and file managers — basically
/// anything that knows how to open a file or directory path.
pub fn detect_installed_apps() -> Vec<PathOpener> {
    let Some(os) = current_os() else {
        return Vec::new();
    };

    KNOWN_APPS
        .iter()
        .filter_map(|app| {
            let entry = app.platforms.iter().find(|p| p.os == os)?;

            let is_available = match entry.detection {
                Detection::PathLookup => is_command_available(entry.command),
                ref d => check_availability(d),
            };

            Some(PathOpener {
                app_id: app.app_id.to_string(),
                name: app.name.to_string(),
                command: entry.command.to_string(),
                is_available,
                is_default: false,
                is_hidden: false,
                sort_order: None,
            })
        })
        .collect()
}

/// Open a path the way a double-click would — using the OS default handler.
///
/// Runs `open` on macOS, `xdg-open` on Linux, `explorer` on Windows.
pub fn open_default(path: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "linux")]
    let mut cmd = std::process::Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("explorer");
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return Err(io::Error::new(io::ErrorKind::Unsupported, "unsupported platform"));

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        cmd.arg(path).spawn()?;
        Ok(())
    }
}

/// Open a path with a specific command string.
///
/// Pass something like `"code"` or `"open -a iTerm"` — the command gets split
/// on whitespace, and your path is tacked on as the last argument. This is a
/// dumb argv-based launcher; it doesn't know about app-specific quirks (e.g.
/// Obsidian's URI scheme). For those, use [`open_with`].
pub fn open_path(command: &str, path: &str) -> io::Result<()> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty command string"));
    }

    let mut cmd = Command::new(parts[0]);
    for part in &parts[1..] {
        cmd.arg(part);
    }
    cmd.arg(path);
    cmd.spawn()?;
    Ok(())
}

/// Open `path` using a [`PathOpener`] returned from [`detect_installed_apps`].
///
/// Unlike [`open_path`], this honors per-app launch strategies — e.g. Obsidian
/// is launched via its `obsidian://` URI scheme so the right vault and file
/// open. For most apps the behavior is the same as [`open_path`].
pub fn open_with(opener: &PathOpener, path: &Path) -> io::Result<()> {
    let known = KNOWN_APPS.iter().find(|a| a.app_id == opener.app_id);
    let launch = known.map(|a| a.launch).unwrap_or(Launch::Argv);

    let mut cmd = match launch {
        Launch::Custom(builder) => builder(path)?,
        Launch::Argv => {
            let parts: Vec<&str> = opener.command.split_whitespace().collect();
            if parts.is_empty() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty command string"));
            }
            let mut cmd = Command::new(parts[0]);
            for part in &parts[1..] {
                cmd.arg(part);
            }
            cmd.arg(path);
            cmd
        }
    };
    cmd.spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_non_empty_list() {
        let apps = detect_installed_apps();
        assert!(!apps.is_empty(), "should detect at least one app");
    }

    #[test]
    fn all_openers_have_defaults() {
        let apps = detect_installed_apps();
        for app in &apps {
            assert!(!app.is_default);
            assert!(!app.is_hidden);
            assert!(app.sort_order.is_none());
        }
    }

    #[test]
    fn known_apps_have_unique_ids() {
        let mut ids: Vec<&str> = KNOWN_APPS.iter().map(|a| a.app_id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), KNOWN_APPS.len(), "app_ids must be unique");
    }

    #[test]
    fn open_path_rejects_empty_command() {
        let result = open_path("", "/tmp");
        assert!(result.is_err());
    }
}

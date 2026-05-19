#![doc = include_str!("../README.md")]

use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;
use std::process::Command;

pub(crate) mod obsidian;

/// What kinds of files an opener accepts.
///
/// Use together with [`PathOpener::accepts_directories`] to decide which
/// openers to show for a given path in your UI. Neither field is consulted
/// by [`open`] itself — they are pure metadata for callers.
///
/// Extensions are stored without the leading dot and are matched
/// case-insensitively against the path's extension via [`FileSupport::accepts_extension`].
///
/// # Examples
///
/// ```
/// use path_opener::FileSupport;
///
/// // A general-purpose opener accepts any file.
/// let editor = FileSupport::Any;
/// assert!(editor.accepts_extension("rs"));
///
/// // A terminal does not open files at all.
/// let terminal = FileSupport::NotSupported;
/// assert!(!terminal.accepts_extension("rs"));
///
/// // A specialized opener accepts only certain extensions.
/// let markdown = FileSupport::Extensions(vec!["md".into(), "markdown".into()]);
/// assert!(markdown.accepts_extension("md"));
/// assert!(markdown.accepts_extension("MD")); // case-insensitive
/// assert!(!markdown.accepts_extension("txt"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", content = "extensions", rename_all = "snake_case")]
pub enum FileSupport {
    /// Opener accepts any file (general editors, file managers).
    Any,
    /// Opener does not open files (terminals).
    NotSupported,
    /// Opener accepts only files with one of the listed extensions
    /// (without the leading dot, lowercase).
    Extensions(Vec<String>),
}

impl FileSupport {
    /// Returns `true` if a file with the given extension can be opened.
    ///
    /// `extension` should be the bare extension without a leading dot
    /// (e.g. `"md"`, not `".md"`). The match is case-insensitive.
    pub fn accepts_extension(&self, extension: &str) -> bool {
        match self {
            FileSupport::Any => true,
            FileSupport::NotSupported => false,
            FileSupport::Extensions(xs) => xs.iter().any(|x| x.eq_ignore_ascii_case(extension)),
        }
    }
}

// Internal sibling of `FileSupport` used inside the static `KNOWN_APPS` table.
// Consts can't allocate, so the registry uses `&'static [&'static str]`; we
// convert to the owned public `FileSupport` at `detect_installed_apps` time.
#[derive(Debug, Clone, Copy)]
enum FileSupportSpec {
    Any,
    NotSupported,
    Extensions(&'static [&'static str]),
}

impl FileSupportSpec {
    fn to_owned(self) -> FileSupport {
        match self {
            FileSupportSpec::Any => FileSupport::Any,
            FileSupportSpec::NotSupported => FileSupport::NotSupported,
            FileSupportSpec::Extensions(xs) => FileSupport::Extensions(xs.iter().map(|s| (*s).to_string()).collect()),
        }
    }
}

/// An app that can open file/directory paths.
///
/// You get these from [`detect_installed_apps`]. Each one tells you the app's
/// name, the shell command to invoke it, whether it's actually installed, and
/// what shape of path it accepts (`accepts_directories` + `file_support`).
///
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
    /// Whether this opener can open a directory path.
    pub accepts_directories: bool,
    /// What files this opener accepts. See [`FileSupport`].
    pub file_support: FileSupport,
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
    AlwaysUnavailable,          // Detection not yet implemented for this OS
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
    accepts_directories: bool,
    file_support: FileSupportSpec,
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
// Every built-in must declare `accepts_directories` and `file_support` explicitly —
// silent defaults are how registries drift.
macro_rules! cross_platform_app {
    (
        $id:expr,
        $name:expr,
        $cmd:expr,
        accepts_directories: $accepts_dirs:expr,
        file_support: $file_support:expr $(,)?
    ) => {
        KnownApp {
            app_id: $id,
            name: $name,
            platforms: &[
                PlatformEntry { os: Os::MacOS, command: $cmd, detection: Detection::PathLookup },
                PlatformEntry { os: Os::Linux, command: $cmd, detection: Detection::PathLookup },
                PlatformEntry { os: Os::Windows, command: $cmd, detection: Detection::PathLookup },
            ],
            launch: Launch::Argv,
            accepts_directories: $accepts_dirs,
            file_support: $file_support,
        }
    };
}

// Same thing but with a macOS .app bundle check instead of PATH on mac.
macro_rules! cross_platform_app_with_mac_bundle {
    (
        $id:expr,
        $name:expr,
        $cmd:expr,
        $bundle:expr,
        accepts_directories: $accepts_dirs:expr,
        file_support: $file_support:expr $(,)?
    ) => {
        KnownApp {
            app_id: $id,
            name: $name,
            platforms: &[
                PlatformEntry { os: Os::MacOS, command: $cmd, detection: Detection::MacAppBundle($bundle) },
                PlatformEntry { os: Os::Linux, command: $cmd, detection: Detection::PathLookup },
                PlatformEntry { os: Os::Windows, command: $cmd, detection: Detection::PathLookup },
            ],
            launch: Launch::Argv,
            accepts_directories: $accepts_dirs,
            file_support: $file_support,
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
        accepts_directories: true,
        file_support: FileSupportSpec::Any,
    },
    KnownApp {
        app_id: "file-manager",
        name: "File Manager",
        platforms: &[PlatformEntry { os: Os::Linux, command: "xdg-open", detection: Detection::PathLookup }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::Any,
    },
    KnownApp {
        app_id: "explorer",
        name: "Explorer",
        platforms: &[PlatformEntry { os: Os::Windows, command: "explorer", detection: Detection::AlwaysAvailable }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::Any,
    },
    // -- Terminals --
    // Terminals accept a directory to cd into, but do not open files.
    KnownApp {
        app_id: "terminal",
        name: "Terminal",
        platforms: &[PlatformEntry {
            os: Os::MacOS,
            command: "open -a Terminal",
            detection: Detection::MacAppBundle("Terminal.app"),
        }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::NotSupported,
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
        accepts_directories: true,
        file_support: FileSupportSpec::NotSupported,
    },
    KnownApp {
        app_id: "gnome-terminal",
        name: "GNOME Terminal",
        platforms: &[PlatformEntry { os: Os::Linux, command: "gnome-terminal", detection: Detection::PathLookup }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::NotSupported,
    },
    KnownApp {
        app_id: "konsole",
        name: "Konsole",
        platforms: &[PlatformEntry { os: Os::Linux, command: "konsole", detection: Detection::PathLookup }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::NotSupported,
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
        accepts_directories: true,
        file_support: FileSupportSpec::NotSupported,
    },
    KnownApp {
        app_id: "kitty",
        name: "Kitty",
        platforms: &[
            PlatformEntry { os: Os::MacOS, command: "open -a Kitty", detection: Detection::MacAppBundle("kitty.app") },
            PlatformEntry { os: Os::Linux, command: "kitty", detection: Detection::PathLookup },
        ],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::NotSupported,
    },
    KnownApp {
        app_id: "windows-terminal",
        name: "Windows Terminal",
        platforms: &[PlatformEntry { os: Os::Windows, command: "wt", detection: Detection::PathLookup }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::NotSupported,
    },
    KnownApp {
        app_id: "powershell",
        name: "PowerShell",
        platforms: &[PlatformEntry { os: Os::Windows, command: "pwsh", detection: Detection::PathLookup }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::NotSupported,
    },
    // The `cmux` CLI opens a directory in a new workspace; the GUI app must be
    // installed for the daemon to run. macOS only for now (see Info.plist).
    KnownApp {
        app_id: "cmux",
        name: "cmux",
        platforms: &[PlatformEntry { os: Os::MacOS, command: "cmux", detection: Detection::MacAppBundle("cmux.app") }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::NotSupported,
    },
    // -- Editors (cross-platform) --
    cross_platform_app_with_mac_bundle!(
        "vscode", "Visual Studio Code", "code", "Visual Studio Code.app",
        accepts_directories: true, file_support: FileSupportSpec::Any,
    ),
    cross_platform_app_with_mac_bundle!(
        "cursor", "Cursor", "cursor", "Cursor.app",
        accepts_directories: true, file_support: FileSupportSpec::Any,
    ),
    cross_platform_app_with_mac_bundle!(
        "sublime-text", "Sublime Text", "subl", "Sublime Text.app",
        accepts_directories: true, file_support: FileSupportSpec::Any,
    ),
    cross_platform_app_with_mac_bundle!(
        "zed", "Zed", "zed", "Zed.app",
        accepts_directories: true, file_support: FileSupportSpec::Any,
    ),
    cross_platform_app!(
        "neovim", "Neovim", "nvim",
        accepts_directories: true, file_support: FileSupportSpec::Any,
    ),
    cross_platform_app!(
        "webstorm", "WebStorm", "webstorm",
        accepts_directories: true, file_support: FileSupportSpec::Any,
    ),
    cross_platform_app!(
        "intellij", "IntelliJ IDEA", "idea",
        accepts_directories: true, file_support: FileSupportSpec::Any,
    ),
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
            // TODO: Linux/Windows Obsidian detection — currently reports unavailable.
            // See task 2026-05-16-path-opener-uri-scheme-and-obsidian (Out of scope).
            PlatformEntry { os: Os::Linux, command: "obsidian", detection: Detection::AlwaysUnavailable },
            PlatformEntry { os: Os::Windows, command: "obsidian", detection: Detection::AlwaysUnavailable },
        ],
        launch: Launch::Custom(obsidian::build_command),
        accepts_directories: true,
        file_support: FileSupportSpec::Extensions(&["md", "markdown", "canvas"]),
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
        Detection::AlwaysUnavailable => false,
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
                accepts_directories: app.accepts_directories,
                file_support: app.file_support.to_owned(),
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
/// is launched via its `obsidian://` URI scheme. For most apps the behavior is
/// the same as [`open_path`].
///
/// Prefer the higher-level [`open`] when you only have an `app_id`.
pub fn open_with(opener: &PathOpener, path: &Path) -> io::Result<()> {
    let known = KNOWN_APPS.iter().find(|a| a.app_id == opener.app_id);
    let launch = known.map(|a| a.launch).unwrap_or(Launch::Argv);
    spawn_for(launch, &opener.command, path)
}

/// Open `path` with the built-in opener identified by `app_id`.
///
/// This is the highest-level entry point: hand it a path and an app id
/// (e.g. `"vscode"`, `"obsidian"`, `"finder"`), and it dispatches to the
/// right launch strategy — argv-append for plain CLI apps, URI scheme for
/// apps like Obsidian.
///
/// Returns `io::ErrorKind::NotFound` if no built-in matches `app_id`.
///
/// ```no_run
/// use std::path::Path;
///
/// # fn main() -> std::io::Result<()> {
/// path_opener::open(Path::new("/Users/me/notes"), "obsidian")?;
/// # Ok(())
/// # }
/// ```
pub fn open(path: &Path, app_id: &str) -> io::Result<()> {
    let Some(known) = KNOWN_APPS.iter().find(|a| a.app_id == app_id) else {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("unknown app_id: {app_id}")));
    };

    let Some(os) = current_os() else {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "unsupported platform"));
    };
    let Some(entry) = known.platforms.iter().find(|p| p.os == os) else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("app_id {app_id:?} has no entry for this platform"),
        ));
    };

    spawn_for(known.launch, entry.command, path)
}

/// What `open` would spawn for a given path + `app_id`, without actually
/// spawning it. Useful when you want to surface the effective command in
/// a UI ("copy effective command") or log it before launching.
///
/// The `program` is the command name as passed to `std::process::Command::new`;
/// `args` is the argv list (excluding `program`). On Obsidian-style launches,
/// `program` will be the platform URI launcher (`open` / `xdg-open` / `cmd`)
/// and `args` will end with the `obsidian://…` URI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CommandPreview {
    /// The program name as passed to `Command::new`.
    pub program: String,
    /// The argv list (does not include `program`).
    pub args: Vec<String>,
}

/// Return what [`open`] would spawn, without spawning anything.
///
/// Errors mirror [`open`]:
/// - `io::ErrorKind::NotFound` if `app_id` is not a known built-in.
/// - `io::ErrorKind::Unsupported` if the app has no entry for the current platform.
/// - Any error returned by the underlying command builder (rare — only for
///   `Launch::Custom` builders that themselves fail).
///
/// ```no_run
/// use std::path::Path;
///
/// # fn main() -> std::io::Result<()> {
/// let preview = path_opener::preview_command(Path::new("/Users/me/notes"), "vscode")?;
/// println!("{} {:?}", preview.program, preview.args);
/// # Ok(())
/// # }
/// ```
pub fn preview_command(path: &Path, app_id: &str) -> io::Result<CommandPreview> {
    let Some(known) = KNOWN_APPS.iter().find(|a| a.app_id == app_id) else {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("unknown app_id: {app_id}")));
    };

    let Some(os) = current_os() else {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "unsupported platform"));
    };
    let Some(entry) = known.platforms.iter().find(|p| p.os == os) else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("app_id {app_id:?} has no entry for this platform"),
        ));
    };

    let cmd = build_command_for(known.launch, entry.command, path)?;
    let program = cmd.get_program().to_string_lossy().into_owned();
    let args = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    Ok(CommandPreview { program, args })
}

fn spawn_for(launch: Launch, command: &str, path: &Path) -> io::Result<()> {
    let mut cmd = build_command_for(launch, command, path)?;
    cmd.spawn()?;
    Ok(())
}

// Construct (but do not spawn) the Command. Extracted so cfg(test) shims can
// inspect what we'd run without launching a process.
fn build_command_for(launch: Launch, command: &str, path: &Path) -> io::Result<Command> {
    match launch {
        Launch::Custom(builder) => builder(path),
        Launch::Argv => {
            let parts: Vec<&str> = command.split_whitespace().collect();
            if parts.is_empty() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty command string"));
            }
            let mut cmd = Command::new(parts[0]);
            for part in &parts[1..] {
                cmd.arg(part);
            }
            cmd.arg(path);
            Ok(cmd)
        }
    }
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

    // Audit: every built-in declares the expected (accepts_directories, file_support)
    // shape per the registry's category. If you add a new built-in, add it here.
    #[test]
    fn every_known_app_declares_expected_metadata() {
        for app in KNOWN_APPS {
            let (expected_accepts_dirs, expected_file_support): (bool, FileSupport) = match app.app_id {
                // File managers / system defaults
                "finder" | "file-manager" | "explorer" => (true, FileSupport::Any),
                // Terminals: accept a directory to cd into, do not open files.
                // cmux is grouped here: `cmux <path>` opens a directory in a new workspace.
                "terminal" | "iterm" | "gnome-terminal" | "konsole" | "alacritty" | "kitty" | "windows-terminal"
                | "powershell" | "cmux" => (true, FileSupport::NotSupported),
                // Editors
                "vscode" | "cursor" | "sublime-text" | "zed" | "neovim" | "webstorm" | "intellij" => {
                    (true, FileSupport::Any)
                }
                // Obsidian
                "obsidian" => (true, FileSupport::Extensions(vec!["md".into(), "markdown".into(), "canvas".into()])),
                other => panic!("unknown built-in {other:?} in audit table — please update the test"),
            };

            assert_eq!(app.accepts_directories, expected_accepts_dirs, "{}: accepts_directories mismatch", app.app_id,);
            assert_eq!(app.file_support.to_owned(), expected_file_support, "{}: file_support mismatch", app.app_id,);
        }
    }

    #[test]
    fn detected_openers_carry_metadata() {
        let apps = detect_installed_apps();
        for app in &apps {
            // Sanity check that the fields are populated on the public struct,
            // not just on the internal table.
            let known = KNOWN_APPS.iter().find(|k| k.app_id == app.app_id).expect("registered");
            assert_eq!(app.accepts_directories, known.accepts_directories);
            assert_eq!(app.file_support, known.file_support.to_owned());
        }
    }

    #[test]
    fn file_support_accepts_extension_is_case_insensitive() {
        let fs = FileSupport::Extensions(vec!["md".into(), "canvas".into()]);
        assert!(fs.accepts_extension("md"));
        assert!(fs.accepts_extension("MD"));
        assert!(fs.accepts_extension("Md"));
        assert!(fs.accepts_extension("canvas"));
        assert!(!fs.accepts_extension("txt"));
        assert!(FileSupport::Any.accepts_extension("anything"));
        assert!(!FileSupport::NotSupported.accepts_extension("md"));
    }

    #[test]
    fn preview_command_for_argv_app_returns_program_and_path() {
        // `vscode` uses Launch::Argv on every supported platform with command "code".
        // The argv we'd run is exactly: code <path>.
        let path = Path::new("/tmp/path-opener-preview-argv");
        let preview = preview_command(path, "vscode").expect("preview_command for vscode");
        assert_eq!(preview.program, "code", "argv launches use the registered command as program");
        assert_eq!(preview.args.last().map(String::as_str), Some("/tmp/path-opener-preview-argv"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn preview_command_for_obsidian_returns_uri() {
        use std::env;
        use std::fs;

        // canonicalize requires the path to exist, so set up a real dir first.
        let tmp = env::temp_dir().join(format!("path-opener-preview-obsidian-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).expect("create tmp dir");

        let preview = preview_command(&tmp, "obsidian").expect("preview_command for obsidian");
        assert_eq!(preview.program, "open", "macOS Obsidian launches via `open <uri>`");
        let last = preview.args.last().expect("at least one arg");
        assert!(last.starts_with("obsidian://open?"), "last arg should be obsidian URI, got: {last}");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn preview_command_for_unknown_app_id_returns_not_found() {
        let err = preview_command(Path::new("/tmp/anything"), "definitely-not-a-real-app-id")
            .expect_err("unknown app_id must error");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}

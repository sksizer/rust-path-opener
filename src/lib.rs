#![doc = include_str!("../README.md")]

use serde::{Deserialize, Serialize};
use std::ffi::OsString;
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
    /// Whether this opener honors a [`Target`] (line/column). `true` for the
    /// GUI editors that can jump to a location inside a file (VS Code, Cursor,
    /// Sublime Text, Zed); `false` for everything else. Use it to decide when a
    /// "jump to line" affordance is worth showing, instead of hardcoding a list.
    pub accepts_target: bool,
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
    // Set for GUI editors that accept a `Target` (line/column) and which — on
    // macOS — must be launched via `open -a` because their CLI shim is
    // unreliable on PATH (see `Editor`). `None` for everything else.
    editor: Option<Editor>,
}

// How `open_with` should turn an opener + path into a Command.
#[derive(Debug, Clone, Copy)]
enum Launch {
    // Default: split the platform's `command` on whitespace, append path as last arg.
    Argv,
    // Custom builder for apps that need more than argv-append (URI schemes, vault lookup, etc.).
    Custom(fn(&Path) -> io::Result<Command>),
}

// How an editor's CLI encodes a `Target` (file + line[:column]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GotoStyle {
    // `<cli> --goto <file>:<line>[:<col>]` — VS Code, Cursor.
    Goto,
    // `<cli> <file>:<line>[:<col>]` — Sublime Text, Zed.
    Suffix,
}

// A GUI editor: the common opener that can navigate *inside* a file to a
// `Target`. Two problems it solves, both surfaced on macOS where these apps are
// detected by their `.app` bundle but shipped with a CLI shim that is frequently
// not on PATH (and a GUI-launched process has a stripped PATH anyway):
//
//   1. A plain open must not depend on the shim — on macOS we launch via
//      `open -a "<AppName>"`, which resolves through LaunchServices.
//   2. Honoring a `Target` *needs* the shim, so we resolve it from a known
//      location inside the bundle first, then PATH, and fall back to a
//      marker-less `open -a` if neither resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Editor {
    // CLI basename to resolve on PATH (Linux/Windows, and macOS fallback).
    cli: &'static str,
    // Location of the CLI shim inside the macOS `.app` bundle, relative to the
    // bundle root. Only read on macOS.
    #[allow(dead_code)]
    mac_cli_in_bundle: &'static str,
    // How this editor's CLI encodes a `Target`.
    goto: GotoStyle,
}

impl Editor {
    // Resolve the CLI to invoke for a `Target` jump. On macOS, prefer the shim
    // inside the app bundle (PATH-independent), then fall back to PATH. On other
    // platforms, use PATH. Returns `None` when nothing resolves — callers then
    // degrade to a marker-less plain open.
    fn resolve_cli(&self, app: &KnownApp) -> Option<OsString> {
        #[cfg(target_os = "macos")]
        if let Some(bundle) = app.mac_bundle()
            && let Some(root) = macos_bundle_path(bundle)
        {
            let shim = root.join(self.mac_cli_in_bundle);
            if shim.exists() {
                return Some(shim.into_os_string());
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = app;

        is_command_available(self.cli).then(|| OsString::from(self.cli))
    }

    // The argv (after the program) that opens `path` at `target`. The caller
    // guarantees `target` carries at least a line.
    fn goto_args(&self, path: &Path, target: &Target) -> Vec<OsString> {
        let mut spec = OsString::from(path.as_os_str());
        if let Some(line) = target.line {
            spec.push(":");
            spec.push(line.to_string());
            if let Some(column) = target.column {
                spec.push(":");
                spec.push(column.to_string());
            }
        }
        match self.goto {
            GotoStyle::Goto => vec![OsString::from("--goto"), spec],
            GotoStyle::Suffix => vec![spec],
        }
    }
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
            editor: None,
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
        file_support: $file_support:expr,
        editor: $editor:expr $(,)?
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
            editor: $editor,
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
        editor: None,
    },
    KnownApp {
        app_id: "file-manager",
        name: "File Manager",
        platforms: &[PlatformEntry { os: Os::Linux, command: "xdg-open", detection: Detection::PathLookup }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::Any,
        editor: None,
    },
    KnownApp {
        app_id: "explorer",
        name: "Explorer",
        platforms: &[PlatformEntry { os: Os::Windows, command: "explorer", detection: Detection::AlwaysAvailable }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::Any,
        editor: None,
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
        editor: None,
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
        editor: None,
    },
    KnownApp {
        app_id: "gnome-terminal",
        name: "GNOME Terminal",
        platforms: &[PlatformEntry { os: Os::Linux, command: "gnome-terminal", detection: Detection::PathLookup }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::NotSupported,
        editor: None,
    },
    KnownApp {
        app_id: "konsole",
        name: "Konsole",
        platforms: &[PlatformEntry { os: Os::Linux, command: "konsole", detection: Detection::PathLookup }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::NotSupported,
        editor: None,
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
        editor: None,
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
        editor: None,
    },
    KnownApp {
        app_id: "windows-terminal",
        name: "Windows Terminal",
        platforms: &[PlatformEntry { os: Os::Windows, command: "wt", detection: Detection::PathLookup }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::NotSupported,
        editor: None,
    },
    KnownApp {
        app_id: "powershell",
        name: "PowerShell",
        platforms: &[PlatformEntry { os: Os::Windows, command: "pwsh", detection: Detection::PathLookup }],
        launch: Launch::Argv,
        accepts_directories: true,
        file_support: FileSupportSpec::NotSupported,
        editor: None,
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
        editor: None,
    },
    // -- Editors (cross-platform) --
    // The four GUI editors carry an `EditorLaunch`: on macOS they launch via
    // `open -a` (their CLI shim is unreliable on PATH) and they can jump to a
    // `Target` line/column through their resolved CLI. VS Code / Cursor take
    // `--goto file:line:col`; Sublime / Zed take a `file:line:col` suffix.
    cross_platform_app_with_mac_bundle!(
        "vscode", "Visual Studio Code", "code", "Visual Studio Code.app",
        accepts_directories: true, file_support: FileSupportSpec::Any,
        editor: Some(Editor {
            cli: "code",
            mac_cli_in_bundle: "Contents/Resources/app/bin/code",
            goto: GotoStyle::Goto,
        }),
    ),
    cross_platform_app_with_mac_bundle!(
        "cursor", "Cursor", "cursor", "Cursor.app",
        accepts_directories: true, file_support: FileSupportSpec::Any,
        editor: Some(Editor {
            cli: "cursor",
            mac_cli_in_bundle: "Contents/Resources/app/bin/cursor",
            goto: GotoStyle::Goto,
        }),
    ),
    cross_platform_app_with_mac_bundle!(
        "sublime-text", "Sublime Text", "subl", "Sublime Text.app",
        accepts_directories: true, file_support: FileSupportSpec::Any,
        editor: Some(Editor {
            cli: "subl",
            mac_cli_in_bundle: "Contents/SharedSupport/bin/subl",
            goto: GotoStyle::Suffix,
        }),
    ),
    cross_platform_app_with_mac_bundle!(
        "zed", "Zed", "zed", "Zed.app",
        accepts_directories: true, file_support: FileSupportSpec::Any,
        editor: Some(Editor {
            cli: "zed",
            mac_cli_in_bundle: "Contents/MacOS/cli",
            goto: GotoStyle::Suffix,
        }),
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
        editor: None,
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

// Locate an installed `.app` bundle: `/Applications` first, then `~/Applications`.
#[cfg(target_os = "macos")]
fn macos_bundle_path(bundle_name: &str) -> Option<std::path::PathBuf> {
    let system_path = std::path::Path::new("/Applications").join(bundle_name);
    if system_path.exists() {
        return Some(system_path);
    }
    if let Some(home) = dirs::home_dir() {
        let user_path = home.join("Applications").join(bundle_name);
        if user_path.exists() {
            return Some(user_path);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn is_macos_app_installed(bundle_name: &str) -> bool {
    macos_bundle_path(bundle_name).is_some()
}

// The `open -a "<name>"` app name for a bundle, e.g. "Visual Studio Code.app"
// → "Visual Studio Code". LaunchServices resolves this regardless of PATH.
#[cfg(target_os = "macos")]
fn bundle_app_name(bundle: &str) -> &str {
    bundle.strip_suffix(".app").unwrap_or(bundle)
}

impl KnownApp {
    // The macOS `.app` bundle this app is detected by, if any.
    #[cfg(target_os = "macos")]
    fn mac_bundle(&self) -> Option<&'static str> {
        self.platforms.iter().find_map(|p| match p.detection {
            Detection::MacAppBundle(bundle) if p.os == Os::MacOS => Some(bundle),
            _ => None,
        })
    }
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
                accepts_target: app.editor.is_some(),
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

/// A location *within* the resource an opener opens — a bundle of
/// "sub-application markers".
///
/// Today it carries a `line` and `column`, honored by the GUI editors that can
/// jump to a spot inside a file (VS Code, Cursor, Sublime Text, Zed). Openers
/// that don't understand a marker ignore it, so passing a `Target` to a
/// terminal or file manager is harmless — it just opens the path.
///
/// This is the extension point for future markers (an anchor, a page, …): add a
/// field here rather than a new `open_*` function per coordinate.
///
/// # Examples
///
/// ```
/// use path_opener::Target;
///
/// let at_line = Target::line(42);
/// let at_cell = Target::at(42, 8);
/// assert_eq!(at_line.line, Some(42));
/// assert_eq!(at_cell.column, Some(8));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Target {
    /// 1-based line to jump to.
    pub line: Option<u32>,
    /// 1-based column, paired with `line` where the editor supports it.
    pub column: Option<u32>,
}

impl Target {
    /// A target at `line` (1-based), no column.
    pub fn line(line: u32) -> Self {
        Target { line: Some(line), column: None }
    }

    /// A target at `line` and `column` (both 1-based).
    pub fn at(line: u32, column: u32) -> Self {
        Target { line: Some(line), column: Some(column) }
    }

    /// Whether this target carries any marker to act on.
    fn is_empty(&self) -> bool {
        self.line.is_none()
    }
}

/// Open `path` using a [`PathOpener`] returned from [`detect_installed_apps`].
///
/// Unlike [`open_path`], this honors per-app launch strategies — e.g. Obsidian
/// is launched via its `obsidian://` URI scheme, and macOS GUI editors launch
/// via `open -a`. For most apps the behavior is the same as [`open_path`].
///
/// Prefer the higher-level [`open`] when you only have an `app_id`.
pub fn open_with(opener: &PathOpener, path: &Path) -> io::Result<()> {
    let known = KNOWN_APPS.iter().find(|a| a.app_id == opener.app_id);
    let mut cmd = build_command(known, &opener.command, path, &Target::default())?;
    cmd.spawn()?;
    Ok(())
}

/// Open `path` with the built-in opener identified by `app_id`.
///
/// This is the highest-level entry point: hand it a path and an app id
/// (e.g. `"vscode"`, `"obsidian"`, `"finder"`), and it dispatches to the
/// right launch strategy — argv-append for plain CLI apps, `open -a` for macOS
/// GUI editors, URI scheme for apps like Obsidian.
///
/// Returns `io::ErrorKind::NotFound` if no built-in matches `app_id`.
///
/// To also jump to a location inside a file, use [`open_at`].
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
    let mut cmd = build_open_command(path, app_id, &Target::default())?;
    cmd.spawn()?;
    Ok(())
}

/// Open `path` with `app_id`, navigating to `target` when the opener supports it.
///
/// Editors that accept a [`Target`] (VS Code, Cursor, Sublime Text, Zed —
/// see [`PathOpener::accepts_target`]) jump to the given line/column via their
/// CLI. Every other opener ignores the target and opens the path normally, so
/// this is always safe to call.
///
/// On macOS the editor CLI is resolved from inside the app bundle first, then
/// PATH; if neither resolves, the file still opens (via `open -a`) but without
/// the jump.
///
/// Errors mirror [`open`].
///
/// ```no_run
/// use std::path::Path;
/// use path_opener::Target;
///
/// # fn main() -> std::io::Result<()> {
/// path_opener::open_at(Path::new("/src/main.rs"), "vscode", &Target::line(42))?;
/// # Ok(())
/// # }
/// ```
pub fn open_at(path: &Path, app_id: &str, target: &Target) -> io::Result<()> {
    let mut cmd = build_open_command(path, app_id, target)?;
    cmd.spawn()?;
    Ok(())
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
    preview_command_at(path, app_id, &Target::default())
}

/// Return what [`open_at`] would spawn for `target`, without spawning anything.
///
/// Same as [`preview_command`], but reflects the [`Target`] jump for editors
/// that accept one. Note the preview is resolved against the current machine:
/// for an editor target, `program` is the editor CLI when it resolves, or the
/// `open -a` fallback when it does not.
///
/// Errors mirror [`preview_command`].
pub fn preview_command_at(path: &Path, app_id: &str, target: &Target) -> io::Result<CommandPreview> {
    let cmd = build_open_command(path, app_id, target)?;
    let program = cmd.get_program().to_string_lossy().into_owned();
    let args = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    Ok(CommandPreview { program, args })
}

// Resolve `app_id` to its current-platform entry and build the Command that
// opens `path` at `target`. Shared by open / open_at / preview_command*.
fn build_open_command(path: &Path, app_id: &str, target: &Target) -> io::Result<Command> {
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

    build_command(Some(known), entry.command, path, target)
}

// Construct (but do not spawn) the Command that opens `path` at `target`.
//
// `app` is the resolved built-in when known (`None` for a caller's custom
// opener, which always launches argv-style). The dispatch order is:
//   1. an editor `Target` jump, when the app is an editor, the target carries a
//      marker, and the editor CLI resolves;
//   2. the app's plain-open strategy: a `Launch::Custom` builder, `open -a` for
//      a macOS GUI editor, or argv-append.
fn build_command(app: Option<&KnownApp>, command: &str, path: &Path, target: &Target) -> io::Result<Command> {
    // 1. Editor `Target` jump — only when the CLI resolves; otherwise fall
    //    through to a marker-less plain open below.
    if !target.is_empty()
        && let Some(app) = app
        && let Some(editor) = app.editor
        && let Some(program) = editor.resolve_cli(app)
    {
        let mut cmd = Command::new(program);
        cmd.args(editor.goto_args(path, target));
        return Ok(cmd);
    }

    // 2. Plain open.
    if let Some(app) = app {
        match app.launch {
            Launch::Custom(builder) => return builder(path),
            Launch::Argv => {
                #[cfg(target_os = "macos")]
                if app.editor.is_some()
                    && let Some(bundle) = app.mac_bundle()
                {
                    let mut cmd = Command::new("open");
                    cmd.arg("-a").arg(bundle_app_name(bundle)).arg(path);
                    return Ok(cmd);
                }
            }
        }
    }

    build_argv_command(command, path)
}

// Split `command` on whitespace and append `path` as the last argument.
fn build_argv_command(command: &str, path: &Path) -> io::Result<Command> {
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

    // Audit: exactly the four GUI editors carry an `Editor`, each with the
    // expected goto style. If you add a new editor, add it here.
    #[test]
    fn every_known_app_declares_expected_editor() {
        for app in KNOWN_APPS {
            let expected_goto: Option<GotoStyle> = match app.app_id {
                "vscode" | "cursor" => Some(GotoStyle::Goto),
                "sublime-text" | "zed" => Some(GotoStyle::Suffix),
                _ => None,
            };
            assert_eq!(app.editor.map(|e| e.goto), expected_goto, "{}: editor goto style mismatch", app.app_id);
            // An app's CLI basename, when it has one, should match its command.
            if let Some(editor) = app.editor {
                let mac = app.platforms.iter().find(|p| p.os == Os::MacOS);
                if let Some(entry) = mac {
                    assert_eq!(editor.cli, entry.command, "{}: editor cli should match macOS command", app.app_id);
                }
            }
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
            assert_eq!(app.accepts_target, known.editor.is_some(), "{}: accepts_target mismatch", app.app_id);
        }
    }

    #[test]
    fn accepts_target_is_set_only_for_editors() {
        let apps = detect_installed_apps();
        for app in &apps {
            let expected = matches!(app.app_id.as_str(), "vscode" | "cursor" | "sublime-text" | "zed");
            assert_eq!(app.accepts_target, expected, "{}: accepts_target", app.app_id);
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
        // `neovim` uses Launch::Argv with PathLookup on every platform and has no
        // `Editor` (no `open -a`, no goto), so the argv is exactly `nvim <path>`
        // on all platforms.
        let path = Path::new("/tmp/path-opener-preview-argv");
        let preview = preview_command(path, "neovim").expect("preview_command for neovim");
        assert_eq!(preview.program, "nvim", "argv launches use the registered command as program");
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

    // -- Fix 1: macOS GUI editors launch via `open -a`, not their CLI shim. --

    #[cfg(target_os = "macos")]
    #[test]
    fn preview_command_for_mac_gui_editor_uses_open_dash_a() {
        // On macOS a plain open of a bundle editor must not depend on the CLI
        // shim (frequently missing from PATH). It launches via `open -a "<name>"`.
        let preview = preview_command(Path::new("/tmp/proj"), "vscode").expect("preview");
        assert_eq!(preview.program, "open");
        assert_eq!(preview.args, vec!["-a".to_string(), "Visual Studio Code".to_string(), "/tmp/proj".to_string()]);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn preview_command_for_gui_editor_uses_cli_off_macos() {
        // Off macOS there's no bundle; the editor launches via its PATH command.
        let preview = preview_command(Path::new("/tmp/proj"), "vscode").expect("preview");
        assert_eq!(preview.program, "code");
        assert_eq!(preview.args, vec!["/tmp/proj".to_string()]);
    }

    // -- Fix 2: Target markers. --

    #[test]
    fn target_constructors_populate_markers() {
        assert_eq!(Target::line(42), Target { line: Some(42), column: None });
        assert_eq!(Target::at(42, 8), Target { line: Some(42), column: Some(8) });
        assert!(Target::default().is_empty());
        assert!(!Target::line(1).is_empty());
    }

    #[test]
    fn goto_args_goto_style_uses_flag() {
        let editor = Editor { cli: "code", mac_cli_in_bundle: "x", goto: GotoStyle::Goto };
        assert_eq!(
            editor.goto_args(Path::new("/src/main.rs"), &Target::line(42)),
            vec![OsString::from("--goto"), OsString::from("/src/main.rs:42")]
        );
        assert_eq!(
            editor.goto_args(Path::new("/src/main.rs"), &Target::at(42, 8)),
            vec![OsString::from("--goto"), OsString::from("/src/main.rs:42:8")]
        );
    }

    #[test]
    fn goto_args_suffix_style_appends_target() {
        let editor = Editor { cli: "subl", mac_cli_in_bundle: "x", goto: GotoStyle::Suffix };
        assert_eq!(
            editor.goto_args(Path::new("/src/main.rs"), &Target::line(7)),
            vec![OsString::from("/src/main.rs:7")]
        );
        assert_eq!(
            editor.goto_args(Path::new("/src/main.rs"), &Target::at(7, 3)),
            vec![OsString::from("/src/main.rs:7:3")]
        );
    }

    #[test]
    fn open_at_ignores_target_for_non_editor() {
        // A non-editor drops the target and opens the path normally, so a
        // preview with a target matches a plain preview.
        let with_target = preview_command_at(Path::new("/tmp/f.rs"), "neovim", &Target::at(10, 2)).expect("preview");
        let plain = preview_command(Path::new("/tmp/f.rs"), "neovim").expect("preview");
        assert_eq!(with_target, plain);
    }

    #[test]
    fn open_at_ignores_empty_target() {
        // An empty target behaves exactly like `open`, even for an editor.
        let empty = preview_command_at(Path::new("/tmp/proj"), "vscode", &Target::default()).expect("preview");
        let plain = preview_command(Path::new("/tmp/proj"), "vscode").expect("preview");
        assert_eq!(empty, plain);
    }

    #[test]
    fn preview_command_at_for_unknown_app_id_returns_not_found() {
        let err = preview_command_at(Path::new("/tmp/x"), "nope", &Target::line(3)).expect_err("must error");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}

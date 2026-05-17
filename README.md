# path-opener

Detects installed apps (editors, terminals, file managers) and opens paths with them. Cross-platform.

You give it a file or directory path, it figures out what's installed and lets you open it with whatever you want. Works on macOS, Linux, and Windows.

## What it knows about

Out of the box it looks for:

- **File managers** -- Finder, Explorer, xdg-open
- **Terminals** -- Terminal.app, iTerm, Alacritty, Kitty, GNOME Terminal, Konsole, Windows Terminal, PowerShell
- **Editors** -- VS Code, Cursor, Sublime Text, Zed, Neovim, WebStorm, IntelliJ
- **Markdown** -- Obsidian (with vault-aware launching, see below)

On macOS it checks for `.app` bundles in `/Applications` and `~/Applications`. Everywhere else it checks PATH.

## Usage

```rust
use std::path::Path;
use path_opener::{detect_installed_apps, open_path, open_with, open_default};

// See what's installed
let apps = detect_installed_apps();
for app in &apps {
    if app.is_available {
        println!("{} -- {}", app.name, app.command);
    }
}

// Recommended: open via a detected PathOpener -- honors per-app launch quirks
let vscode = apps.iter().find(|a| a.app_id == "vscode" && a.is_available).unwrap();
open_with(vscode, Path::new("/my/project")).unwrap();

// Or pass a raw command string (dumb argv-based; use only when you don't have a PathOpener)
open_path("code", "/my/project").unwrap();

// Or just use the system default (like double-clicking)
open_default("/my/project").unwrap();
```

### `open_with` vs `open_path`

`open_with(opener, path)` is the recommended entry point: it dispatches on the app's launch strategy. Most apps just get argv-append (same as `open_path`), but a few have quirks. For example, Obsidian doesn't take a CLI path argument -- it speaks the `obsidian://` URI scheme. `open_with` knows that; `open_path` does not.

## Obsidian (experimental)

Obsidian is launched via its `obsidian://` URI scheme. When you call `open_with(obsidian, path)`:

- if `path` is a known vault root -> `obsidian://open?vault=<Name>`
- if `path` is inside a known vault -> `obsidian://open?vault=<Name>&file=<relative>`
- otherwise -> `obsidian://open?path=<absolute>` (Obsidian decides what to do)

Vault list is read from Obsidian's own config:

- macOS: `~/Library/Application Support/obsidian/obsidian.json`
- Linux: `~/.config/obsidian/obsidian.json`
- Windows: `%APPDATA%\obsidian\obsidian.json`

You can also enumerate vaults yourself:

```rust
for vault in path_opener::obsidian::discover_vaults() {
    println!("{} -- {}", vault.name, vault.path.display());
}
```

> **Note:** `obsidian::discover_vaults()` is **experimental**. It's a first sketch of a more general "discover an app's project-like contexts" pattern (vaults for Obsidian, recent projects for JetBrains, workspaces for VS Code, etc.). The shape may change before 1.0.

## The `PathOpener` struct

Each detected app comes back as a `PathOpener`:

```rust
PathOpener {
    app_id: "vscode",           // stable ID
    name: "Visual Studio Code", // display name
    command: "code",            // shell command
    is_available: true,         // actually installed?
    is_default: false,          // for your UI to manage
    is_hidden: false,           // for your UI to manage
    sort_order: None,           // for your UI to manage
}
```

The `is_default`, `is_hidden`, and `sort_order` fields are always initialized to false/None -- they're there so you can layer user preferences on top without needing a wrapper type.

## Features

- **`specta`** -- Derives `specta::Type` on `PathOpener` for TypeScript binding generation. Off by default.

## Install

```toml
[dependencies]
path-opener = "0.1"
```

## License

MIT

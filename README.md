# path-opener

Detects installed apps (editors, terminals, file managers, Markdown apps) and opens paths with them. Cross-platform.

Hand it a `(path, app_id)` and it figures out the rest -- including app-specific quirks like Obsidian's `obsidian://` URI scheme and vault lookup. Works on macOS, Linux, and Windows.

## Install

```toml
[dependencies]
path-opener = "0.5"
```

## Quickstart

```rust
use std::path::Path;
use path_opener::{detect_installed_apps, open, open_default};

# fn main() -> std::io::Result<()> {
// Primary entry point: hand it a path and an app id.
open(Path::new("/Users/me/notes"), "obsidian")?;

// Or look at what's installed first.
for app in detect_installed_apps() {
    if app.is_available {
        println!("{} ({})", app.name, app.app_id);
    }
}

// Or fall back to the OS default ("just open it" / double-click).
open_default("/Users/me/notes")?;
# Ok(())
# }
```

## What it knows about

Out of the box it looks for:

- **File managers** -- Finder, Explorer, xdg-open
- **Terminals** -- Terminal.app, iTerm, Alacritty, Kitty, GNOME Terminal, Konsole, Windows Terminal, PowerShell
- **Editors** -- VS Code, Cursor, Sublime Text, Zed, Neovim, WebStorm, IntelliJ
- **Markdown** -- Obsidian (with internal vault-aware launching, see below)

On macOS it checks for `.app` bundles in `/Applications` and `~/Applications`. Elsewhere it checks PATH.

## What can each opener handle?

Each `PathOpener` declares two flat metadata fields the caller uses to build "what can I open this with?" UIs. Neither is consulted by `open()` itself.

- `accepts_directories: bool` -- can this opener open a directory path?
- `file_support: FileSupport` -- which files it accepts.

`FileSupport` has three variants:

| Variant | Used by | Meaning |
|---|---|---|
| `FileSupport::Any` | Editors (VS Code, Cursor, Zed, Sublime, Neovim, WebStorm, IntelliJ); file managers (Finder, Explorer, xdg-open) | Accepts any file -- no extension restriction. |
| `FileSupport::NotSupported` | Terminals (Terminal.app, iTerm, Alacritty, Kitty, GNOME Terminal, Konsole, Windows Terminal, PowerShell) | Accepts a directory (to cd into) but does not open files. |
| `FileSupport::Extensions(Vec<String>)` | Obsidian (`["md", "markdown", "canvas"]`); future specialized apps (Bear, Logseq, Typora) slot in here | Accepts only the listed extensions. |

The "what can I open this with?" filter for a given path looks like this:

```rust
use std::path::Path;
use path_opener::{detect_installed_apps, PathOpener};

fn openers_for(path: &Path) -> Vec<PathOpener> {
    let is_dir = path.is_dir();
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    detect_installed_apps()
        .into_iter()
        .filter(|app| app.is_available)
        .filter(|app| {
            if is_dir {
                app.accepts_directories
            } else {
                app.file_support.accepts_extension(ext)
            }
        })
        .collect()
}
```

`FileSupport::accepts_extension(ext)` matches case-insensitively and treats `Any` as a yes for everything, `NotSupported` as a no for everything.

## Public API

The public surface is small on purpose:

- `open(path, app_id)` -- primary dispatch; resolves the built-in opener and launches.
- `open_at(path, app_id, &Target)` -- like `open`, but navigates to a location inside the file (see [Targets](#targets-jump-to-a-line)).
- `open_default(path)` -- system default ("just open it"), like a double-click.
- `open_with(opener, path)` -- lower-level form when you already hold a `PathOpener`.
- `preview_command(path, app_id)` / `preview_command_at(path, app_id, &Target)` -- what `open`/`open_at` would spawn, without spawning it.
- `detect_installed_apps() -> Vec<PathOpener>` -- registry walk.
- `PathOpener { app_id, name, command, is_available, accepts_directories, file_support, accepts_target, is_default, is_hidden, sort_order }`.
- `enum FileSupport { Any, NotSupported, Extensions(Vec<String>) }`.
- `struct Target { line: Option<u32>, column: Option<u32> }` with `Target::line(n)` / `Target::at(line, col)`.

(`open_path(command, path)` from `0.1.x` is still present as a primitive that takes a raw command string; prefer `open(path, app_id)` for new code.)

URI schemes, vault metadata, CLI-shim resolution, and per-app launch strategies are implementation details -- they do not appear on the public API.

## Targets: jump to a line

`open_at(path, app_id, &Target)` opens a file and navigates to a location inside it. A `Target` is a small bundle of "sub-application markers":

```rust,no_run
use std::path::Path;
use path_opener::{open_at, Target};

# fn main() -> std::io::Result<()> {
open_at(Path::new("/src/main.rs"), "vscode", &Target::line(42))?;
open_at(Path::new("/src/main.rs"), "zed", &Target::at(42, 8))?; // line + column
# Ok(())
# }
```

Targets are honored by the GUI editors that can jump to a spot inside a file -- **VS Code, Cursor, Sublime Text, Zed** -- via their CLI (`--goto file:line:col` or a `file:line:col` suffix). Check `PathOpener::accepts_target` to know which detected openers qualify, instead of hardcoding a list:

```rust
use path_opener::detect_installed_apps;

let jump_capable: Vec<_> =
    detect_installed_apps().into_iter().filter(|a| a.is_available && a.accepts_target).collect();
```

Any opener that doesn't understand a marker (a terminal, a file manager, Obsidian) ignores it and just opens the path -- so `open_at` is always safe to call. `Target` is the extension point for future markers: new coordinates become new fields, not a new function per coordinate.

## macOS launching

On macOS the GUI editors are detected by their `.app` bundle but ship a CLI shim (`code`, `subl`, …) that is often not symlinked onto PATH -- and a GUI-launched process inherits a stripped PATH anyway. So a plain `open`/`open_with` launches these editors through `open -a "<App Name>"` (LaunchServices, PATH-independent) rather than the bare shim. `open_at` still needs the shim to pass the line, so it resolves the shim from inside the app bundle first, then PATH, and falls back to a marker-less `open -a` if neither resolves.

## Obsidian (experimental)

Obsidian doesn't take a CLI path argument -- it speaks the `obsidian://` URI scheme. When you call `open(path, "obsidian")`, path-opener internally:

1. reads Obsidian's own `obsidian.json` to discover the directories Obsidian has registered as vaults;
2. picks a URI based on where `path` falls:
   - `path` is a registered vault root -> `obsidian://open?vault=<Name>`
   - `path` is inside a registered vault -> `obsidian://open?vault=<Name>&file=<relative>`
   - otherwise -> `obsidian://open?path=<absolute>` (Obsidian decides)
3. invokes the platform URI launcher (`open` on macOS, `xdg-open` on Linux, `start` on Windows).

Vault discovery reads:

- macOS: `~/Library/Application Support/obsidian/obsidian.json`
- Linux: `~/.config/obsidian/obsidian.json`
- Windows: `%APPDATA%\obsidian\obsidian.json`

If `obsidian.json` is missing or unreadable, the URI falls through to `?path=<absolute>` and Obsidian decides what to do.

> **Note:** the vault-discovery routing is experimental. The shape and exact URI strategy may evolve before 1.0. "Vault" is Obsidian-specific terminology used inside this opener -- it is not part of path-opener's general vocabulary, and never appears on the public API. Pin a minor version if you depend on the specifics.

### Obsidian detection follow-up

On macOS, availability is determined by the presence of `Obsidian.app` in `/Applications` or `~/Applications`. On Linux and Windows, `is_available` currently returns `false` -- detection (`.desktop` files on Linux, registry/`AppData` lookup on Windows) is a planned follow-up. The opener still appears in `detect_installed_apps()`; it just reports as unavailable on those platforms.

## The `PathOpener` struct

Each detected app comes back as a `PathOpener`:

```rust,no_run
use path_opener::{FileSupport, PathOpener};

let opener = PathOpener {
    app_id: "vscode".into(),
    name: "Visual Studio Code".into(),
    command: "code".into(),
    is_available: true,
    accepts_directories: true,
    file_support: FileSupport::Any,
    accepts_target: true,      // honors a Target (line/column) — see below
    is_default: false,         // for your UI to manage
    is_hidden: false,          // for your UI to manage
    sort_order: None,          // for your UI to manage
};
```

The `is_default`, `is_hidden`, and `sort_order` fields are always initialized to false/None -- they're there so you can layer user preferences on top without a wrapper type.

## Features

- **`specta`** -- Derives `specta::Type` on `PathOpener` and `FileSupport` for TypeScript binding generation. Off by default.

## Migration from 0.4.x

- `PathOpener` gained `accepts_target: bool`. Code that constructs `PathOpener` literals by hand needs to fill it in (`true` only for VS Code, Cursor, Sublime Text, Zed).
- macOS launch behavior changed: `open`/`open_with` now launch the GUI editors via `open -a "<App Name>"` instead of their CLI shim. This fixes spurious `NotFound` (`os error 2`) failures when the shim isn't on PATH. `preview_command` reflects the new argv.
- New, additive: `open_at`, `preview_command_at`, and the `Target` type. Existing calls are unaffected.

## Migration from 0.1.x

- `PathOpener` gained `accepts_directories: bool` and `file_support: FileSupport`. Code that constructed `PathOpener` literals by hand needs to fill them in.
- `FileSupport::Extensions` carries a `Vec<String>` (not `&'static [&'static str]`) -- this is the owned shape that round-trips through serde.
- `obsidian::discover_vaults()` is no longer a public API. Use `open(path, "obsidian")` and let path-opener route internally. Vault metadata never crosses the public boundary.
- New entry point `open(path, app_id)` is preferred over `open_with(opener, path)` for callers that only have an app id.

## License

MIT

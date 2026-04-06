# path-opener

Detects installed apps (editors, terminals, file managers) and opens paths with them. Cross-platform.

You give it a file or directory path, it figures out what's installed and lets you open it with whatever you want. Works on macOS, Linux, and Windows.

## What it knows about

Out of the box it looks for:

- **File managers** -- Finder, Explorer, xdg-open
- **Terminals** -- Terminal.app, iTerm, Alacritty, Kitty, GNOME Terminal, Konsole, Windows Terminal, PowerShell
- **Editors** -- VS Code, Cursor, Sublime Text, Zed, Neovim, WebStorm, IntelliJ

On macOS it checks for `.app` bundles in `/Applications` and `~/Applications`. Everywhere else it checks PATH.

## Usage

```rust
use path_opener::{detect_installed_apps, open_path, open_default};

// See what's installed
let apps = detect_installed_apps();
for app in &apps {
    if app.is_available {
        println!("{} -- {}", app.name, app.command);
    }
}

// Open a project in VS Code
open_path("code", "/my/project").unwrap();

// Or just use the system default (like double-clicking)
open_default("/my/project").unwrap();
```

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

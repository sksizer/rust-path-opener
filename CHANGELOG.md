# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.5.0] - 2026-07-09

### Fixed

- macOS: GUI editors (VS Code, Cursor, Sublime Text, Zed) are detected by their `.app` bundle but were launched via their CLI shim (`code`, `subl`, …). That shim is frequently not on PATH — and a GUI-launched process inherits a stripped PATH — so `open`/`open_with` could fail with `NotFound` (`os error 2`) for an app that detection reported as available. They now launch via `open -a "<App Name>"` (LaunchServices, PATH-independent). Reported against the [Annado](https://github.com/ABeehive/Annado/pull/21) integration.

### Added

- `open_at(path, app_id, &Target)` — open a file and navigate to a location inside it. Honored by the GUI editors that accept a `Target` (VS Code, Cursor, Sublime Text, Zed) via their CLI (`--goto file:line:col` or a `file:line:col` suffix); every other opener ignores the target and opens the path normally.
- `Target { line: Option<u32>, column: Option<u32> }` — a bundle of "sub-application markers", with `Target::line(n)` and `Target::at(line, col)` constructors. This is the extension point for future markers: new coordinates become new fields, not a new `open_*` function per coordinate. Derives `specta::Type` behind the `specta` feature.
- `PathOpener.accepts_target: bool` — whether an opener honors a `Target`. Lets callers find jump-capable editors instead of hardcoding a list.
- `preview_command_at(path, app_id, &Target)` — the `Target`-aware companion to `preview_command`.
- For `open_at` on macOS, the editor CLI is resolved from inside the app bundle first (PATH-independent), then PATH, falling back to a marker-less `open -a` if neither resolves.

### Compatibility

- `PathOpener` gained `accepts_target: bool` — hand-built `PathOpener` literals must fill it in.
- macOS `open`/`open_with`/`preview_command` now spawn `open -a "<App Name>"` for the GUI editors instead of the bare CLI. Behavior is more robust; the effective argv changed.

## [0.4.0] - 2026-05-20

### Changed

- **BREAKING-IF-OBSERVED:** Obsidian URIs now use the vault's internal id (`vault=<id>`) instead of the basename (`vault=<name>`). Behavior is identical when vault names are unique; for users with duplicate-named vaults (e.g. `~/work/notes` and `~/personal/notes`), the launch is now deterministic — Obsidian previously picked one of the colliding vaults non-deterministically. Downstream consumers that scrape the URI (via `preview_command` or otherwise) may need to update if they were parsing `vault=` as a human-readable name.

## [0.3.0] - 2026-05-19

### Added

- new `preview_command(path, app_id) -> io::Result<CommandPreview>` public API that returns the program + argv `open()` would spawn, without spawning anything. Use case: surfacing the effective command in a UI ("copy effective command") or logging it before launching.
- new public `CommandPreview { program: String, args: Vec<String> }` struct. Derives `specta::Type` behind the existing `specta` feature flag, mirroring `FileSupport` and `PathOpener`.

### Compatibility

Additive only — no breaking changes. `open()`, `open_with()`, `open_path()`, `open_default()`, and `detect_installed_apps()` are unchanged.

## [0.2.1] - 2026-05-17

### Added

- add cmux opener for opening directories in workspaces

## [0.2.0] - 2026-05-17

### Added

- obsidian module + Launch::Custom internal escape hatch
- add accepts_directories and FileSupport metadata to openers Adds two flat metadata fields on PathOpener that callers use to build 'what can I open this with?' UIs: - accepts_directories: bool — can this opener open a directory path? - file_support: FileSupport — Any | NotSupported | Extensions(Vec<String>) Every built-in declares both fields explicitly (no silent defaults). Editors and file managers get (true, Any); terminals get (true, NotSupported); Obsidian gets (true, Extensions(['md','markdown','canvas'])). The static registry uses a sibling FileSupportSpec with &'static [&'static str] (consts can't allocate); detect_installed_apps converts to the owned FileSupport on the public surface. This keeps serde Deserialize working on PathOpener — the alternative shape using &'static [&'static str] on FileSupport directly does not derive Deserialize. A FileSupport::accepts_extension(ext) helper provides case-insensitive extension matching, consistent with how Path::extension yields a bare extension. Adds AlwaysUnavailable Detection so Linux/Windows Obsidian reports is_available=false (detection follow-up tracked in the task).
- add public open(path, app_id) convenience Adds open(path, app_id) as the primary entry point — callers hand path-opener a (path, app_id) and let it own the strategy. Internally resolves the registered KnownApp, picks the platform entry, and dispatches through the same Launch::{Argv, Custom} machinery as open_with. open_with stays as the lower-level form for callers that already have a PathOpener struct in hand. Extracts a build_command_for helper so upcoming Obsidian strategy tests can inspect the constructed Command without spawning a process.

### Changed

- make obsidian module crate-private The obsidian module and its Vault struct were exposed as pub on the WIP commit, but vault is Obsidian-domain terminology that does not belong in path-opener's general vocabulary. Callers go through open(path, app_id) and never need to see vault metadata.

## [0.1.1] - 2026-04-29



# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.5.1] - 2026-07-14



### Added

- add CommandPreview struct and preview_command API
- add open_at line/column targets; fix macOS editor launch (os error 2)
- emit URIs with internal vault id

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



---
type: task
schema_version: 1
status: in-progress
created: '2026-05-19'
last_reviewed: '2026-05-20'
impact: high
complexity: small
tags:
- bugfix
- obsidian
- uri
related: []
---
# Emit Obsidian URIs using internal vault id (0.4)

## Goal

Stop ambiguous `obsidian://open?vault=<basename>` URIs from misrouting
to the wrong vault when two registered Obsidian vaults share a folder
basename. Switch to emitting the vault's internal id
(`vault=<id>`) so the launch is deterministic. Ships as
`path-opener` 0.4.0.

This task is the upstream half of polish task
`2026-05-19-obsidian-uri-vault-id` (see
`/Users/sksizer/Developer/polish/docs/planning/tasks/2026-05-19-obsidian-uri-vault-id.md`).
The downstream half (bumping `path-opener` in polish and verifying)
stays tracked in polish.

## Today

- `src/obsidian.rs::discover_vaults` reads `obsidian.json` and
  produces `Vault { id, name, path }` records, where `id` is the
  stable internal Obsidian identifier and `name` is the folder
  basename (`src/obsidian.rs:67-69`).
- `Vault.id` carries `#[allow(dead_code)]` (`src/obsidian.rs:25-27`)
  — it's parsed and discarded for URI purposes.
- `build_uri` (`src/obsidian.rs:86-100`) emits URIs using `name`:
  - vault root → `obsidian://open?vault=<name>`
  - file inside a vault → `obsidian://open?vault=<name>&file=<rel>`
  - outside any known vault → `obsidian://open?path=<abs>`
- Vault matching uses `v.path == abs` (post-`canonicalize`), which
  correctly picks ONE vault per path. The bug is downstream of that
  pick: if two vaults register with the same basename
  (`/Users/me/work/notes` and `/Users/me/personal/notes`), both
  legitimately produce `vault=notes`, and Obsidian's URI resolver
  picks whichever vault it considers first internally.
- Existing tests in `src/obsidian.rs:208-265` cover the three
  emission strategies but use unique vault names, so the ambiguity
  case is uncovered.
- Current published: 0.3.0 (preview_command API).

## Proposed

- Emit Obsidian URIs using the internal vault id unconditionally.
  The strategy ladder becomes:
  - vault root → `obsidian://open?vault=<id>`
  - file inside a vault → `obsidian://open?vault=<id>&file=<rel>`
  - outside any known vault → `obsidian://open?path=<abs>` (unchanged)
- `Vault.id` loses its `#[allow(dead_code)]` annotation; the field
  is now load-bearing.
- Always-on, not conditional on detecting duplicate names:
  1. Obsidian's URI scheme accepts `vault=<id>` equivalently to
     `vault=<name>`.
  2. IDs are stable across vault renames, so launches stay correct
     even when the user renames a folder.
  3. Conditional behavior — "use id when ambiguous, name otherwise" —
     creates two code paths and a third state (the ambiguity
     detection itself) for negligible win.
  4. The `preview_command` API (0.3) exposes the URI to downstream
     consumers; ids are still human-readable enough in that surface.

## Approach

1. **Switch URI emission to `vault=<id>`.** In
   `src/obsidian.rs::build_uri` (lines 86-100):

   ```rust
   // before
   if let Some(v) = vaults.iter().find(|v| v.path == abs) {
       return Ok(format!("obsidian://open?vault={}", encode(&v.name)));
   }
   if let Some((v, rel)) = vaults.iter().find_map(...) {
       let rel_str = rel.to_str().ok_or_else(...)?;
       return Ok(format!("obsidian://open?vault={}&file={}", encode(&v.name), encode(rel_str)));
   }

   // after
   if let Some(v) = vaults.iter().find(|v| v.path == abs) {
       return Ok(format!("obsidian://open?vault={}", encode(&v.id)));
   }
   if let Some((v, rel)) = vaults.iter().find_map(...) {
       let rel_str = rel.to_str().ok_or_else(...)?;
       return Ok(format!("obsidian://open?vault={}&file={}", encode(&v.id), encode(rel_str)));
   }
   ```

   Remove `#[allow(dead_code)]` from `Vault.id` (`src/obsidian.rs:25`).
   The `path` query fallback (line 99) is unchanged.

2. **Update tests.** Adjust the three existing assertion tests in
   `src/obsidian.rs:208-265` so expected URIs use the vault id, not
   the name. Add a new test that builds two vaults with the same
   `name` but different `id` and asserts both produce distinct URIs
   (proves the bug is fixed). Keep the URI-encoding tests as they are.

3. **Release 0.4.0.** Bump `Cargo.toml` to `0.4.0`. Update
   `CHANGELOG.md` with a clearly-marked BREAKING-IF-OBSERVED note:
   *"Obsidian URIs now use the vault's internal id (`vault=<id>`)
   instead of the basename (`vault=<name>`). Behavior is identical
   when vault names are unique; for users with duplicate-named vaults,
   the launch is now deterministic. Downstream consumers that scrape
   the URI may need to update."* The user-visible behavior change
   warrants a minor-version bump (0.3 → 0.4) rather than a patch.
   `cargo publish`.

## Files to touch

- `src/obsidian.rs` — change `build_uri` to emit `vault=<id>` in
  both the vault-root and file-in-vault arms; drop
  `#[allow(dead_code)]` on `Vault.id`; update existing tests;
  add the duplicate-name disambiguation test.
- `Cargo.toml` — bump version to `0.4.0`.
- `CHANGELOG.md` — note the URI-emission change with the
  user-visible-behavior callout.

## Acceptance criteria

- [ ] AC-1: `build_uri` emits `vault=<id>` for both the vault-root
      and file-in-vault arms; the `path` fallback is unchanged.
- [ ] AC-2: New test: two `Vault` records with identical `name` but
      distinct `id` produce distinct URIs (the bug-fix proof).
- [ ] AC-3: Existing emission tests pass with updated expected URIs.
- [ ] AC-4: `Vault.id` is no longer `#[allow(dead_code)]` — the
      lint passes without the attribute.
- [ ] AC-5: `CHANGELOG.md` documents the 0.4.0 behavior change with
      the BREAKING-IF-OBSERVED callout.
- [ ] AC-6: `path-opener` 0.4.0 is published to crates.io.
- [ ] AC-7: `just full-check` passes.

## Out of scope

- Backwards-compatibility shim. We're not keeping the `vault=<name>`
  emission as a fallback or option. The id form is strictly more
  correct and Obsidian supports both equivalently.
- Migrating Obsidian's `obsidian.json` itself or interacting with
  Obsidian's vault registry beyond reading it.
- Exposing vault id information through path-opener's public API
  (it's still crate-private — `Vault` is `pub(crate)`).
- Detecting and warning about duplicate-name vaults inside any
  downstream consumer. Diagnostics are not the bug; the
  wrong-launch is.

## Dependencies

- None. 0.4 changes are non-conflicting with 0.3's preview_command
  API (different code paths).

## Discovery context

- Surfaced 2026-05-19 from downstream usage in
  `sksizer/polish`: user reported "open in Obsidian" launching the
  wrong vault and hypothesized that Obsidian was mishandling vaults
  with the same folder name. Investigation showed the path-opener
  URI builder emits `vault=<name>` which Obsidian resolves
  non-deterministically when names collide.
- Obsidian's URI scheme docs confirm `vault=<id>` is supported and
  is the recommended unambiguous form.

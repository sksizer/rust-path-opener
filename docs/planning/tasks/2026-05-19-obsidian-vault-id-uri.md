---
type: task
schema_version: 1
status: in-progress
created: '2026-05-19'
last_reviewed: '2026-05-20'
readiness_verified_at: '2026-05-20T04:37:11Z'
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

## Post-mortem

_Captured by /sdlc:task-work on 2026-05-20. PR: pending._

### Acceptance criteria coverage

- AC-1: auto — `cargo test obsidian::tests::` (both vault-root and file-in-vault arms have updated assertions; the path fallback is still covered by `uri_for_path_outside_any_vault_falls_through_to_path_query`).
- AC-2: auto — new test `uri_disambiguates_vaults_with_identical_basename` asserts distinct URIs from two same-name vaults.
- AC-3: auto — `cargo test` (all 20 unit tests + 6 doctests pass).
- AC-4: auto — `cargo clippy -- --deny warnings` (via `just full-check`) passes without `#[allow(dead_code)]` on `Vault.id`.
- AC-5: auto — CHANGELOG.md content visible in diff.
- AC-6: deferred-user — `cargo publish` is a live, irreversible release action that must be authorized by the maintainer.
- AC-7: auto — `just full-check` passes (see commit-time check log).

### What worked

- Spec included the literal before/after diff for `build_uri`. Implementation was mechanical; sub-agent delegation would have added overhead with no upside.
- `just full-check` + `just ci` caught nothing because there was nothing to catch — first run was clean, tests passed first time.

### Friction and automation gaps

- The 5b rebase (gate-then-flip) hit a frontmatter conflict because the verify-commit (on feat) and the start-commit (on main) both touched the YAML block, on adjacent lines. The conflict resolution is structurally always the union of fields. `/sdlc:task-work`'s skill text says "stop and ask" on rebase conflict, but this specific conflict is inherent to the design and asking the user is busywork — the skill could either (a) pre-resolve via a deterministic merge driver, or (b) have ensure-ready stamp on a separate line so the conflict is empty, or (c) document that this conflict shape is expected and auto-resolvable.
- `/sdlc:setup --obsidian` wrote `docs/planning/backlog/backlog.base` (singular) into a directory that doesn't otherwise exist, while the entity directory the same script created is `backlogs/` (plural). Required a manual move + filter edit. The plugin's `entities/backlog/base.yaml` template also has `file.inFolder("planning/backlog")` (singular) baked into its filter, which is inconsistent with the plural directory the structure step creates. Upstream plugin bug worth a follow-up.
- Spec said "drop `#[allow(dead_code)]` on `Vault.id`" but didn't address what happens to `Vault.name` once it's no longer read by production code. Had to make a judgment call (kept the field, moved the attribute) — a one-line note in the spec about the symmetric handling would have removed the decision.
- Project's `just setup-worktree` recipe doesn't exist (skill assumes a JS-flavored repo with pnpm + lefthook). For pure-Rust projects this step is a no-op. The skill could detect repo language and skip the init block when it doesn't apply, or split into a "JS init" sub-step that runs conditionally.
- The base session started with a pre-existing `M CHANGELOG.md` on main from an unrelated edit. The worktree branched from the committed state and was unaffected, but the leftover modification is still sitting in the main working tree and will need separate attention.

### Spawned follow-up tasks

Plugin-side gaps were filed as draft tasks in `sksizer/dev` (the plugin repo), not in this project. Tracked via PR https://github.com/sksizer/dev/pull/42:

- `2026-05-20-setup-obsidian-backlog-dir-mismatch` — fix `/sdlc:setup --obsidian` writing into singular `backlog/` dir.
- `2026-05-20-task-work-rebase-frontmatter-conflict` — eliminate Step 5b's deterministic rebase conflict.
- `2026-05-20-task-work-worktree-init-language-agnostic` — make Step 4's init block conditional on project type so it no-ops on pure-Rust repos.

Project-internal items not promoted to tasks:
- Spec gap re: `Vault.name` once `id` becomes load-bearing — handled inline by keeping `name` with `#[allow(dead_code)]` (it's still used by test fixtures and remains useful for diagnostics).
- Leftover `M CHANGELOG.md` on main from a prior session — unrelated to this work, will need a separate cleanup pass.

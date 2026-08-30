# Task 4 Report

Date: 2026-08-29
Worktree: `/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation`
Branch: `codex/mieli-implementation`
Task: Implement watcher translation and autosave eligibility

## Implementation

- Added `TabId` to [src/state.rs](/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation/src/state.rs).
- Added pure autosave primitives in [src/autosave.rs](/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation/src/autosave.rs):
  - `AutosaveKey`
  - `autosave_is_current`
- Added watcher primitives in [src/file/watcher.rs](/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation/src/file/watcher.rs):
  - `FileSystemEvent`
  - `WatchError`
  - `FileWatcherService`
  - deterministic notify event and error reduction helpers
- Wired module exports through [src/file/mod.rs](/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation/src/file/mod.rs) and [src/lib.rs](/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation/src/lib.rs).

## TDD Evidence

### Red

- Added failing tests first in `src/autosave.rs` and `src/file/watcher.rs`.
- The combined command from the brief was invalid on this Cargo version:
  - `cargo test autosave::tests::stale_generation_cannot_save_a_newer_edit file::watcher::tests::notify_events_are_reduced_to_changed_created_removed_or_error`
  - Result: `error: unexpected argument 'file::watcher::tests::notify_events_are_reduced_to_changed_created_removed_or_error' found`
- Ran separate focused red checks instead:
  - `cargo test autosave::tests::stale_generation_cannot_save_a_newer_edit`
  - `cargo test file::watcher::tests::notify_events_are_reduced_to_changed_created_removed_or_error`
- Verified expected compile failures for missing `TabId`, `AutosaveKey`, `autosave_is_current`, `FileSystemEvent`, and `translate_event`.

### Green

- Implemented the minimal production code to satisfy the red failures.
- Ran focused green verification:
  - `cargo test autosave::`
  - `cargo test file::watcher::`

## Tests And Results

- `cargo test autosave::` -> passed (`2 passed, 20 filtered out`)
- `cargo test file::watcher::` -> passed (`4 passed, 18 filtered out`)
- `cargo fmt --check` -> failed initially due to formatting only
- `cargo fmt` -> applied formatting
- `cargo fmt --check` -> passed
- `cargo test` -> passed (`22 passed`)

## Files

- [src/state.rs](/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation/src/state.rs)
- [src/autosave.rs](/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation/src/autosave.rs)
- [src/file/mod.rs](/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation/src/file/mod.rs)
- [src/file/watcher.rs](/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation/src/file/watcher.rs)
- [src/lib.rs](/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation/src/lib.rs)

## Self-Review

- Autosave logic is pure and validates tab id, generation, path, and dirty state with no timer ownership in this module.
- Watcher callbacks only translate `notify` results into domain events and send them over the channel; no app state mutation was added.
- Workspace watches are recursive, file-parent watches are non-recursive, watched directories are canonicalized and deduplicated, and the home directory root is rejected.
- Ordinary watcher failures return `Result` errors with readable messages; production code does not use `unwrap()` or `expect()`.

## Concerns

- `notify` kinds outside create/modify/remove are currently reduced to `Changed` so the later app-state layer can stay deterministic without platform-specific branching. If Task 5 needs finer behavior for access/meta events, that can be tightened there without changing the watcher ownership boundary.

## Post-Review Fixes

Review date: 2026-08-29

- Fixed watched-path deduplication so canonical path tracking now stores the effective `RecursiveMode` and upgrades an existing non-recursive watch to recursive by re-registering the live watcher.
- Fixed unknown `notify` kind handling so `EventKind::Any`, `EventKind::Access(_)`, and `EventKind::Other` now emit explicit `FileSystemEvent::Error` values instead of being treated as content changes.

### Post-Review TDD Evidence

- Added focused regression tests first:
  - `recursive_watch_upgrades_an_existing_non_recursive_watch`
  - `unknown_notify_kinds_are_reduced_to_error_events`
- Verified red:
  - first regression failed because only the initial non-recursive watch was recorded
  - second regression failed because `EventKind::Any` still mapped to `Changed`
- Implemented the minimal watcher changes in `src/file/watcher.rs`.
- Verified green:
  - `cargo test recursive_watch_upgrades_an_existing_non_recursive_watch` -> passed
  - `cargo test unknown_notify_kinds_are_reduced_to_error_events` -> passed

### Post-Review Verification

- `cargo test file::watcher::` -> passed (`6 passed, 18 filtered out`)
- `cargo test autosave::` -> passed (`2 passed, 22 filtered out`)
- `cargo fmt --check` -> passed
- `cargo test` -> passed (`24 passed`)

### Remaining Concerns

- None specific to the reviewed issues. Unsupported watcher kinds now surface explicitly and recursive workspace coverage is no longer blocked by an earlier non-recursive registration.

# Mieli

Mieli is a native desktop Markdown editor built on the Bezel stack. It opens individual Markdown files or recursive Markdown workspaces, keeps one tab per canonical file path, autosaves local edits, watches for external changes, and stores a Recent Files list in a small JSON config file.

## What it uses

- Bezel revision `0bd3940829787bfe7214f79c2ca779f63ac1e332`
- `bezel`: app shell, theme helpers, and shared UI primitives
- `bezel-editor` as `editor`: the editor entity used for each tab
- `bezel-markdown` as `markdown`: Markdown parse/serialize APIs used for the Task 9 fixed-point verification
- `bezel-gpui` and `bezel-gpui-platform` `=0.3.4`: native windowing, rendering, actions, menus, timers, and platform bootstrap
- `directories`: resolves the platform config directory for Recent Files persistence
- `notify`: watches workspace roots and file-parent directories for external create/change/remove events
- `rfd`: native open/save file and folder dialogs
- `serde` and `serde_json`: serialize the Recent Files JSON file; Markdown documents themselves are stored as UTF-8 text, not JSON

## Runtime architecture

The native entry point is [`src/main.rs`](/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation/src/main.rs). Startup registers Bezel fonts, initializes the Bezel appearance system, initializes the Bezel editor package, installs native actions and menus, and opens one GPUI window containing the `Mieli` root entity.

`Mieli` in [`src/app.rs`](/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation/src/app.rs) owns the application state:

- open tabs and the active tab
- the optional workspace root and recursive Markdown tree
- the Recent Files store
- autosave tasks
- the filesystem watcher
- modal and notification state

Each tab stores a Bezel `editor::Editor` entity plus Mieli-owned file metadata such as the canonical path, saved baseline, current disk version, dirty flag, disk state, and autosave generation counter.

## Data flow

1. Opening a file reads UTF-8 Markdown from disk with `read_markdown`, canonicalizes the path, deduplicates against already-open tabs, then creates a Bezel `editor::Editor` from the loaded source.
2. Editing happens inside the Bezel editor entity. Mieli observes editor changes, recomputes `dirty`, increments the autosave generation, and reschedules the tab's autosave task.
3. Saving or autosaving asks the editor for its current `source()` string and writes that UTF-8 text to disk with `write_markdown`. Mieli then updates the saved baseline, disk version, and dirty state only after the write succeeds.
4. External filesystem changes are delivered by `notify`, polled into the UI every 250 ms, deduplicated by path, and resolved against canonical open-tab paths.
5. Clean tabs reload from disk automatically. Dirty tabs never get overwritten automatically; they enter conflict state and require an explicit user choice.
6. Recent Files records successful canonical file opens and saves, persists up to 20 paths, and removes stale entries if a recent path no longer exists when reopened.

Mieli does not currently call `markdown::parse` and `markdown::serialize` in the save path. The runtime path is string-in and string-out through the Bezel editor. Task 9 adds an automated fixed-point test for the pinned Bezel Markdown APIs so the README can document normalization behavior without claiming byte-identical parser round-trips.

## Usage

### Open files and folders

- `Open File` opens one `.md` or `.markdown` file through a native dialog.
- `Open Folder` scans the selected directory recursively and shows only directories that contain Markdown descendants.
- Opening a file that is already open focuses the existing tab instead of opening a duplicate.

### Tabs and sidebar

- The sidebar is hidden by default.
- The file tree sorts directories before files, compares names case-insensitively, and uses the full path only as a final tie-breaker.
- The tab strip keeps one tab per canonical file path.
- Switching tabs attempts a synchronous save of the previously active dirty tab only when autosave is enabled.

### Save and Save As

- `Save` writes the current editor text back to the tab's canonical path.
- `Save As` writes first, then canonicalizes the destination and retargets the tab to that new canonical path.
- If the chosen Save As destination does not already end in `.md` or `.markdown`, Mieli appends `.md`.
- Save operations do not create missing parent directories.

## Autosave

- Autosave is enabled by default.
- Each tab uses a replaceable 800 ms debounce timer after the most recent edit.
- Autosave is eligible only when the tab is dirty, has a real file path, and is not blocked by an unresolved conflict or deletion state.
- Before writing, the autosave task revalidates the tab id, autosave generation, canonical path, and dirty state so a stale task cannot write into a newer tab or newer edit generation.
- If an autosave write fails, Mieli shows a notification and keeps the tab dirty.

## External changes, conflicts, and deletion

- Workspace roots are watched recursively.
- Files opened outside the active workspace are watched through their parent directories with non-recursive watches.
- The watcher refuses to watch the home directory root.
- Clean external changes reload the editor from disk automatically.
- Dirty external changes open a `File changed on disk` modal with `Reload from Disk`, `Keep Mine`, and `Cancel`.
- `Reload from Disk` replaces the editor content with the current disk file and clears conflict state.
- `Keep Mine` preserves local edits, keeps the tab mapped to the same file, unblocks autosave, and schedules the next valid autosave.
- `Cancel` preserves the conflict state and keeps autosave blocked until the user makes another explicit decision.
- External deletion opens a `File deleted on disk` modal with `Keep Open` and `Close`.
- `Keep Open` preserves the in-memory editor content, marks the tab dirty with `DiskState::Deleted`, and allows the file to be written back later to the same path.
- `Close` removes the tab immediately instead of asking for a second discard confirmation.

## Recent Files

- Recent Files stores canonical paths in `recent-files.json` under the platform config directory resolved by `directories::ProjectDirs::from("com", "Mieli", "Mieli")`.
- The list is most-recent-first and capped at 20 entries.
- Reopening a recent file refreshes its position.
- If a recent path is missing, Mieli removes it from the list and reports that cleanup outcome in the notification/error path.

## Supported files and text behavior

- Supported extensions: `.md` and `.markdown`, case-insensitive
- File contents must be valid UTF-8
- Invalid UTF-8 files are rejected with the exact message `The file is not valid UTF-8.`

## Bezel Markdown normalization

The pinned Bezel Markdown API under revision `0bd3940829787bfe7214f79c2ca779f63ac1e332` now has a regression test in [`src/file/io.rs`](/Users/zhaoxin/RustroverProjects/Mieli/.worktrees/mieli-implementation/src/file/io.rs) that exercises headings, emphasis, links, unordered and ordered lists, quotes, inline and fenced code, task items, tables, and blank lines.

The contract we verify is fixed-point stability:

```rust
let first = markdown::serialize(&markdown::parse(source));
let second = markdown::serialize(&markdown::parse(&first));
assert_eq!(first, second);
```

This is intentionally not a byte-for-byte preservation claim. Bezel may normalize layout details such as blank-line placement, indentation, or other serializer formatting choices before reaching its stable output form.

On the representative Task 9 corpus, the first serialization changed `*italic*` to `_italic_` and inserted a blank line between a plain bullet item and the following task-list block. A second parse/serialize pass produced identical output.

## Shortcuts

On macOS:

- `cmd-s`: Save
- `cmd-shift-s`: Save As
- `cmd-w`: Close Tab
- `cmd-shift-l`: Toggle Sidebar
- `cmd-q`: Quit
- `ctrl-tab`: Next Tab
- `ctrl-shift-tab`: Previous Tab

On non-macOS builds:

- `ctrl-s`: Save
- `ctrl-shift-s`: Save As
- `ctrl-w`: Close Tab
- `ctrl-shift-l`: Toggle Sidebar
- `ctrl-q`: Quit
- `ctrl-tab`: Next Tab
- `ctrl-shift-tab`: Previous Tab

The File menu also exposes `Open Recent`, `Refresh Tree`, `Save All`, and up to 20 direct recent-file actions.

## Verification status

As of 2026-08-30 on host target `aarch64-apple-darwin`, `cargo fmt --check`, `cargo check`, `cargo test`, and `cargo clippy --all-targets --all-features -- -D warnings` all pass. The only reported warning is the existing upstream future-incompatibility notice for `block v0.1.6`.

`rustup target list --installed` currently reports only the host target `aarch64-apple-darwin`, so there were no installed non-host desktop targets available for additional `cargo check --target ...` coverage in this run.

The full command record and the honest manual smoke-test status for this run are written to `.superpowers/sdd/2026-08-29-mieli-markdown-editor/task-9-report.md`.

## Known limitations

- Mieli currently supports only local UTF-8 Markdown files with `.md` or `.markdown` extensions.
- There is no user-facing `New File` action yet; the current flow starts from opening an existing file or folder.
- Save and Save As fail when the destination parent directory does not already exist.
- Filesystem watching is limited to the workspace root and parent directories of open files outside that workspace; Mieli does not watch arbitrary ancestor trees.
- Dirty external changes and external deletions are intentionally resolved by modal choice instead of automatic merge or restore logic.
- The Bezel Markdown round-trip guarantee documented here is fixed-point stability, not byte-identical preservation of the original source text.

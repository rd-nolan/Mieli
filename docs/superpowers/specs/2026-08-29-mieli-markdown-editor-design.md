# Mieli Markdown Editor Design

## Scope

Mieli is a native, local-first Markdown editor for macOS, Windows, and Linux.
The first release owns the complete local editing loop:

1. Open a Markdown file or folder.
2. Browse a recursive Markdown-only file tree.
3. Edit multiple files in independent tabs.
4. Save manually or through debounced autosave.
5. Detect external changes and protect unsaved content.
6. Reopen recently used files.

The Markdown file on disk remains the only document source of truth. The MVP
does not include login, cloud sync, AI, plugins, a database, export, preview
panes, split editing, or full-text search.

## Bezel research baseline

The implementation is based on the latest `crabtalk/bezel` checkout inspected
for this design, revision `0bd3940829787bfe7214f79c2ca779f63ac1e332`.

The relevant current APIs are:

- Native bootstrap: `gpui_platform::application().run`, `App`,
  `App::open_window`, `WindowOptions`, `WindowBounds`, `Bounds`, and `cx.activate`.
- Theme setup: `ui::register_fonts`,
  `theme::appearance::init(AppearanceMode::System, cx)`, and
  `theme::appearance::observe_window`.
- Editor setup: `editor::init(cx)` installs the Bezel editor keymap;
  `cx.new(|cx| editor::Editor::new(source, cx).with_scroll(scroll))` creates an
  editing entity; `Editor::source()` returns normalized Markdown; `Editor::doc`
  and `Editor::selection` expose read-only editor state.
- Change propagation: an owning entity can use `cx.observe(&editor, ...)` to
  react whenever the editor notifies after an edit.
- Dialog UI: `ui::popover::modal`, `dialog_card`, `dialog_title`,
  `dialog_body`, and `Theme::button` provide native GPUI modal surfaces.
- Close interception: `Window::on_window_should_close` can return `false` and
  keep the window open while a save or quit decision is pending; the callback
  is allowed to call into the root entity.
- Async timing: GPUI's executor exposes `timer(Duration)`, and
  `Context::spawn` supplies a weak entity handle for safe delayed work.

Bezel does not provide a native file picker in the inspected editor, UI, or
GPUI APIs. Mieli therefore uses `rfd` only for Open File, Open Folder, and Save
As dialogs. Bezel's `editor` and `markdown` crates remain the editing and
document model; Mieli does not introduce a second Markdown editor.

## Alternatives considered

### Pinned Git dependencies — selected

Mieli names Bezel's facade, editor, and Markdown crates through Git
dependencies pinned to the inspected revision. The app also names the matching
`bezel-gpui` and `bezel-gpui-platform` packages so every GPUI type belongs to
the same type universe. This keeps the application repository small while
tracking the current upstream API exactly.

### Vendored Bezel workspace

Copying or vendoring the whole Bezel workspace would make local source changes
easy, but would substantially enlarge Mieli, duplicate package ownership, and
make upgrades noisy. It is not needed because Mieli consumes the public crate
boundaries.

### Published Bezel crates without a revision

Using only the registry versions would be simpler, but could lag the requested
current source and would make API research less reproducible. It is not the
chosen approach for this first implementation.

## Architecture

The application is one GPUI root entity, `Mieli`, with focused supporting
modules:

```text
Mieli root entity
├── AppState
│   ├── workspace_root: Option<PathBuf>
│   ├── sidebar_visible: bool
│   ├── file_tree: Vec<FileTreeNode>
│   ├── tabs: Vec<EditorTab>
│   ├── active_tab: Option<TabId>
│   ├── recent_files: Vec<PathBuf>
│   ├── auto_save_enabled: bool
│   ├── modal: Option<Modal>
│   └── notification: Option<Notification>
├── file service
│   ├── UTF-8 read/write
│   ├── canonical path handling
│   └── recursive Markdown scanner
├── FileWatcherService
├── autosave tasks keyed by TabId
└── Bezel Editor entities keyed by EditorTab
```

`main.rs` remains bootstrap-only. Actions are declared centrally in
`actions.rs`, UI rendering is split into small modules, and filesystem/config
work is kept out of render functions.

### App state and tab state

Each `EditorTab` contains:

- a stable `TabId`;
- a canonical `PathBuf` and display title;
- an `Entity<editor::Editor>`;
- the last serialized source known to be saved;
- the last observed disk version;
- `dirty: bool`;
- a single `DiskState` enum;
- an autosave generation number.

`DiskState` is modeled as `Synced`, `ModifiedExternally`, `Deleted`, or
`Conflict`. A conflict state never silently reloads or overwrites the user's
local editor content.

Paths are canonicalized before tab deduplication and recent-file insertion.
Opening the same physical file again activates its existing tab.

### File tree

The scanner produces a recursive tree containing only Markdown files and
directories that contain at least one Markdown descendant. Markdown detection
is case-insensitive for `.md` and `.markdown`. Directories sort before files;
each group sorts by case-insensitive name with a deterministic path tie-breaker.

The sidebar is hidden by default and fixed at 240px when visible. It is a
scrollable GPUI element. Expanding or collapsing a directory changes only UI
state; typing in the editor never rescans the filesystem. Refreshes happen on
Open Folder, Save As, watcher events, and an explicit refresh action.

### Bezel editor integration

The editor is always an `editor::Editor` entity. Opening a file reads UTF-8
text and constructs:

```text
disk bytes
  → String
  → editor::Editor::new(source, cx)
  → Bezel editing and markdown document model
  → editor.source()
  → UTF-8 file write
```

Mieli does not mutate Bezel's private document fields and does not maintain a
parallel text buffer. Reloading an external clean file replaces the tab's
editor entity with a new `Editor::new` entity and resets the saved baseline.

The owner observes each editor entity. On notification it reads `source()`
once, compares it with the saved baseline, marks the tab dirty when they
differ, and schedules the tab's autosave generation. The editor's own keymap
continues to handle text editing, selection, undo, clipboard, and Markdown
marks.

### Actions and menus

`actions.rs` defines unit actions for Open File, Open Folder, Save, Save As,
Save All, Close Tab, Toggle Sidebar, Next Tab, Previous Tab, and Quit. The
action handlers live on the root entity so menu commands and keyboard
shortcuts share one state transition.

The native File menu contains Open File, Open Folder, Open Recent, Save, Save
As, Save All, Close Tab, and Exit. Open Recent entries are regenerated after a
successful open and use bounded unit actions for the 20 possible positions.

Bindings follow platform conventions:

| Action | macOS | Windows/Linux |
| --- | --- | --- |
| Save | Cmd+S | Ctrl+S |
| Save As | Cmd+Shift+S | Ctrl+Shift+S |
| Close Tab | Cmd+W | Ctrl+W |
| Toggle Sidebar | Cmd+Shift+L | Ctrl+Shift+L |
| Next Tab | Ctrl+Tab | Ctrl+Tab |
| Previous Tab | Ctrl+Shift+Tab | Ctrl+Shift+Tab |
| Quit | Cmd+Q | Ctrl+Q |

Bezel's `editor::init` is called independently so its editor-scoped bindings
remain active.

## Save and dirty lifecycle

Manual Save serializes only the active editor and writes UTF-8 bytes. On a
successful write it updates the baseline, disk version, and `dirty` state. On
failure it reports a readable error and leaves `dirty == true`.

Save As uses the native picker, appends `.md` when the selected path has no
Markdown extension, writes the active document, changes the tab's canonical
path/title, refreshes the workspace tree, and moves the new path to the front
of Recent Files.

Save All iterates over dirty tabs and records every failure. It never clears a
tab's dirty state unless that tab's individual write succeeds.

## Autosave

Autosave is enabled by default and uses an 800ms debounce. Every tab has an
independent task generation:

```text
editor notification
  → dirty = true
  → increment TabId generation
  → cancel/drop prior task
  → wait 800ms
  → verify tab exists, generation is current, dirty, and path is unchanged
  → serialize and write
  → clean only on success
```

The generation check prevents a delayed task from saving another tab or a
closed/replaced tab. Tab switching attempts an immediate save for the tab
losing focus. App close attempts Save All when autosave is enabled.

Autosave failures preserve dirty state and set a user-visible notification.
The app never writes once per keystroke.

## Filesystem watcher and external changes

`FileWatcherService` owns the `notify` watcher and translates OS callbacks
into `FileSystemEvent` values sent to the root entity. The watcher never
directly mutates UI state.

Watch scope is:

- the current workspace root recursively, when a workspace exists;
- otherwise, the parent directories of opened files;
- additionally, parent directories of files opened outside the current
  workspace.

The service does not watch the user's home directory.

Each tab stores a `DiskVersion` containing existence, modification metadata,
length, and a content digest. After Mieli writes a file it records the new
version before processing later watcher events. An event matching that version
is treated as the app's own save; a different version is external.

External handling is:

- Clean tab + changed file: reload from disk, reset the baseline, and notify.
- Dirty tab + changed file: set `DiskState::Conflict` and show Keep Mine,
  Reload from Disk, and Cancel.
- Deleted file: set `DiskState::Deleted`, keep the editor contents, mark it
  dirty, and show Keep Open or Close.
- Workspace create/delete: rescan the Markdown tree.

Keep Mine leaves the editor content intact and allows the next save to
overwrite the disk. Reload from Disk discards local edits only after explicit
confirmation. Rename detection is intentionally represented as deletion plus
creation in the first version.

## Recent files

Recent Files are persisted as a small JSON configuration object under the
platform-specific directory returned by `directories::ProjectDirs`. Markdown
documents are never stored there. The list is canonicalized, deduplicated,
bounded to 20 entries, and reordered newest-first after successful opens or
Save As operations.

Selecting a missing recent file displays an error and removes that entry.
Recent folders are not part of the MVP.

## Cross-platform validation

Business logic is shared across macOS, Windows, and Linux. Platform-specific
code is limited to native menus and modifiers, `rfd` dialogs, application
configuration directories, and GPUI window bootstrap. The implementation will
use `cfg` only at those seams. A platform will be described as runtime-tested
only when the application has actually run there; otherwise the report will
identify the available target or CI check.

## Dialogs and error handling

Native file selection uses `rfd` with Markdown filters and explicit extension
validation. Save and open errors are converted into readable messages for
permission denied, missing files, disk-full conditions, and invalid UTF-8.
Filesystem and watcher functions return `Result`; ordinary I/O paths do not
use `unwrap()`.

GPUI modal dialogs are used for dirty-tab close, external conflicts, deleted
files, and failed app shutdown. A canceled choice leaves the corresponding
state unchanged.

## Tests

Pure logic tests cover:

- case-insensitive Markdown extension recognition;
- recursive Markdown-only tree scanning and sorting;
- canonical-path tab deduplication;
- clean → dirty → clean transitions;
- successful and failed save behavior;
- per-tab autosave generation semantics;
- recent-file move-to-front, deduplication, capacity, and missing cleanup;
- clean external update reload;
- dirty external update conflict;
- deleted-file state;
- Bezel Markdown parse/serialize fixed-point behavior and documented
  normalization.

The final verification runs `cargo fmt --check`, `cargo check`, `cargo test`,
and `cargo clippy` when supported. macOS is verified by the local build;
Windows and Linux are verified with available target checks or CI evidence,
without claiming runtime support for a platform that was not actually tested.

## Known limitations

- Bezel's `Editor::source()` intentionally normalizes Markdown; the README
  will document any observed whitespace, list, newline, or fence changes from
  round-trip tests.
- The first version treats rename as delete plus create rather than preserving
  a tab through a rename event.
- The watcher is event-based and may receive duplicate/coalesced OS events;
  disk-version comparison makes processing idempotent, but a transient delay
  is possible.
- Native dialog appearance is provided by `rfd` and follows the host platform.
- Windows and Linux runtime validation depends on available local targets or
  CI; source-level checks alone will be reported as such.

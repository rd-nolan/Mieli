# Mieli Markdown Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a native GPUI Markdown editor that uses Bezel's `editor` and `markdown` crates for multi-tab editing, recursive workspaces, safe autosave, external-change protection, and persistent recent files.

**Architecture:** Keep one GPUI root entity responsible for application state and user-visible transitions. Each open file owns one `editor::Editor` entity plus a saved Markdown baseline and disk version; file I/O, recursive scanning, recent-file persistence, and filesystem events live in focused services. The UI renders state and dispatches centralized actions, while Bezel remains the only editing surface and Markdown document model.

**Tech Stack:** Rust 2024, GPUI from `bezel-gpui`, `gpui_platform`, Bezel `bezel`, `bezel-editor`, and `bezel-markdown` at revision `0bd3940829787bfe7214f79c2ca779f63ac1e332`, Bezel UI/theme crates, `notify`, `rfd`, `directories`, `serde`, and `serde_json`.

**Spec:** `docs/superpowers/specs/2026-08-29-mieli-markdown-editor-design.md`

## Global Constraints

- The disk `.md` or `.markdown` file is the only Markdown document source of truth.
- The editor surface must be Bezel `editor::Editor`; do not add a second Markdown editor or private document format.
- Markdown recognition is case-insensitive for `.md` and `.markdown`; all document reads and writes use UTF-8.
- The default autosave debounce is 800ms and must never write once per keystroke.
- Every delayed autosave must validate `TabId`, generation, dirty state, and unchanged path before writing.
- Dirty content remains dirty after any failed write.
- A clean external update reloads; a dirty external update enters conflict state; a deleted file offers Keep Open or Close.
- The sidebar is hidden by default, fixed at 240px when visible, recursive, Markdown-only, and independently scrollable.
- Recent Files is canonicalized, deduplicated, newest-first, bounded to 20 entries, and persisted as configuration JSON only.
- Ordinary filesystem errors return `Result` and become readable user notifications; production I/O paths do not use `unwrap()`.
- Shared business logic is platform-independent; platform-specific code is limited to GPUI bootstrap/menu bindings, native dialogs, application directories, and window integration.
- Verification must include `cargo fmt --check`, `cargo check`, `cargo test`, and `cargo clippy` when supported; unavailable runtime platforms must be reported as target checks rather than claimed as runtime-tested.

## File map

Create or modify these files:

- `Cargo.toml`: pinned Bezel dependencies and the small native-support dependency set.
- `README.md`: usage, architecture, dependency reasons, round-trip normalization, platform verification, and known limitations.
- `src/main.rs`: native GPUI bootstrap only.
- `src/lib.rs`: library module exports so pure logic can be tested with `cargo test`.
- `src/actions.rs`: application actions and platform key bindings.
- `src/state.rs`: `TabId`, `DiskState`, `DiskVersion`, `FileTreeNode`, `EditorTab`, modal state, and user notifications.
- `src/app.rs`: Task 1's minimal renderable shell, then `Mieli` root entity, action transitions, tab lifecycle, autosave scheduling, watcher polling, and close protection.
- `src/file/mod.rs`: file-service exports and shared file errors.
- `src/file/io.rs`: UTF-8 reads, writes, canonical paths, and disk versions.
- `src/file/scanner.rs`: recursive Markdown-only tree scan.
- `src/file/watcher.rs`: `notify` ownership and `FileSystemEvent` translation.
- `src/config/mod.rs`: platform configuration path lookup and configuration exports.
- `src/config/recent.rs`: JSON Recent Files persistence and ordering.
- `src/autosave.rs`: debounce identity and save eligibility logic.
- `src/ui/mod.rs`: UI module exports and reusable GPUI rendering helpers.
- `src/ui/root.rs`: toolbar, workspace/editor layout, empty state, notifications, and modal placement.
- `src/ui/sidebar.rs`: fixed-width scrollable sidebar.
- `src/ui/file_tree.rs`: recursive directory/file rendering.
- `src/ui/tabs.rs`: tab strip, active/dirty indicators, close buttons, and tab switching.
- `src/ui/dialogs.rs`: close, conflict, delete, and quit confirmation cards.

The exact line ranges will be recorded after each file is created; no existing
application flow is being preserved because the starting binary only prints a
greeting.

### Task 1: Configure the pinned Bezel application shell

**Files:**

- Modify: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/app.rs`
- Modify: `src/main.rs`
- Test: `cargo check`

**Interfaces:**

- Produces a compilable `mieli` library crate with modules available to unit tests.
- Produces a native `main` that follows Bezel's actual bootstrap sequence.

- [ ] **Step 1: Add the exact dependency graph.**

Use the inspected Bezel revision for the facade, editor, Markdown model, and GPUI packages. Keep the package names aligned so the `gpui::` paths generated by `actions!` resolve to Bezel's GPUI fork.

```toml
[dependencies]
bezel = { git = "https://github.com/crabtalk/bezel", rev = "0bd3940829787bfe7214f79c2ca779f63ac1e332" }
editor = { package = "bezel-editor", git = "https://github.com/crabtalk/bezel", rev = "0bd3940829787bfe7214f79c2ca779f63ac1e332" }
markdown = { package = "bezel-markdown", git = "https://github.com/crabtalk/bezel", rev = "0bd3940829787bfe7214f79c2ca779f63ac1e332" }
gpui = { package = "bezel-gpui", version = "0.3.4" }
gpui_platform = { package = "bezel-gpui-platform", version = "0.3.4", features = ["font-kit"] }
directories = "6"
notify = "6"
rfd = "0.15"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Retain only these dependencies after compilation proves they are needed. Do not add a second UI or editor library.

- [ ] **Step 2: Create library module declarations.**

Declare only the bootstrap module that exists in this task; later tasks add their module declarations after creating their files:

```rust
pub mod app;
```

Create a minimal renderable shell so `main.rs` can compile before the feature modules exist:

```rust
pub struct Mieli;

impl Mieli {
    pub fn new(_: &mut gpui::Context<Self>) -> Self { Self }
}

impl gpui::Render for Mieli {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        gpui::div()
    }
}
```

Task 5 replaces this shell with the complete root entity and adds the remaining library module declarations.

- [ ] **Step 3: Replace the greeting with the verified Bezel bootstrap.**

Implement the same sequence used by `apps/hello`: register fonts, initialize system appearance, initialize Bezel editor keys, open a centered window, observe appearance changes, create the root view, and activate the app.

```rust
fn main() {
    gpui_platform::application().run(|cx: &mut gpui::App| {
        if let Err(err) = bezel::ui::register_fonts(cx) {
            eprintln!("FONT REGISTRATION FAILED: {err:?}");
        }
        bezel::theme::appearance::init(
            bezel::theme::appearance::AppearanceMode::System,
            cx,
        );
        editor::init(cx);
        let bounds = gpui::Bounds::centered(None, gpui::size(gpui::px(1100.0), gpui::px(760.0)), cx);
        cx.open_window(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                bezel::theme::appearance::observe_window(window, cx).detach();
                cx.new(app::Mieli::new)
            },
        ).expect("Mieli window should open");
        cx.activate(true);
    });
}
```

The `expect` is limited to the top-level window bootstrap; ordinary file and watcher paths must remain fallible.

- [ ] **Step 4: Run the shell check and commit the shell.**

Run: `cargo check`

Expected: dependency resolution and the empty root entity compile, with no use of a second `gpui` crate.

```bash
git add Cargo.toml Cargo.lock src/main.rs src/lib.rs
git commit -m "feat: bootstrap Mieli with Bezel"
```

### Task 2: Implement file I/O, disk versions, and the recursive Markdown tree

**Files:**

- Create: `src/state.rs`
- Create: `src/file/mod.rs`
- Create: `src/file/io.rs`
- Create: `src/file/scanner.rs`
- Test: `src/file/io.rs` and `src/file/scanner.rs` unit tests

**Interfaces:**

- Produces `FileTreeNode`, `DiskState`, `DiskVersion`, `FileError`, and the file-service functions consumed by app state.
- Consumes only `std::fs`, `std::path`, and the Rust standard library; no GPUI code enters the scanner or I/O modules.

- [ ] **Step 1: Write the failing extension and scanner tests.**

Add tests that create a temporary directory using a unique path under `std::env::temp_dir`, remove that exact path at the end, and assert both the requested positive cases and the excluded branches.

```rust
#[test]
fn markdown_extension_is_case_insensitive() {
    assert!(is_markdown_file(Path::new("README.md")));
    assert!(is_markdown_file(Path::new("README.MD")));
    assert!(is_markdown_file(Path::new("note.markdown")));
    assert!(is_markdown_file(Path::new("note.MARKDOWN")));
    assert!(!is_markdown_file(Path::new("test.txt")));
    assert!(!is_markdown_file(Path::new("image.png")));
}

#[test]
fn scanner_keeps_only_directories_with_markdown_descendants() {
    let tree = scan_markdown_tree(&root_with("README.md", "test.txt", "docs/api.md", "docs/image.png", "assets/image.png")).unwrap();
    assert_eq!(tree_names(&tree), vec!["docs/", "README.md"]);
}
```

The helper must write the named files and create parent directories explicitly so the test does not depend on a repository fixture.

- [ ] **Step 2: Run the focused tests and verify they fail for missing functions.**

Run: `cargo test file::io::tests::markdown_extension_is_case_insensitive file::scanner::tests::scanner_keeps_only_directories_with_markdown_descendants`

Expected: compilation fails because the service functions are not implemented.

- [ ] **Step 3: Implement state types and case-insensitive Markdown detection.**

Use a single enum for disk state and a deterministic version value:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiskState { Synced, ModifiedExternally, Deleted, Conflict }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiskVersion {
    pub exists: bool,
    pub modified: Option<SystemTime>,
    pub len: u64,
    pub digest: u64,
}

pub fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}
```

`digest` must be computed from the bytes with `DefaultHasher`; it is used only for change comparison, not as a security checksum.

- [ ] **Step 4: Implement fallible UTF-8 I/O and canonical paths.**

Expose these signatures:

```rust
pub fn read_markdown(path: &Path) -> Result<String, FileError>;
pub fn write_markdown(path: &Path, source: &str) -> Result<DiskVersion, FileError>;
pub fn canonicalize_path(path: &Path) -> Result<PathBuf, FileError>;
pub fn disk_version(path: &Path) -> Result<DiskVersion, FileError>;
```

`read_markdown` maps `InvalidData` to the exact user-facing meaning “The file is not valid UTF-8.” `write_markdown` creates no parent directories implicitly, writes all bytes with `fs::write`, then computes and returns the version from disk. `canonicalize_path` canonicalizes existing files and, for Save As paths that do not exist, canonicalizes the parent and appends the final component. Map permission denied, not found, and other I/O errors into `FileError` with the original path and operation.

- [ ] **Step 5: Implement recursive scanner ordering and pruning.**

`scan_markdown_tree(root)` must read one directory at a time, recursively scan child directories, omit files that are not Markdown, and omit directories whose returned child list is empty. Sort directory children before Markdown files, then sort each group by lowercase display name and full path tie-breaker. New nodes start `expanded: true` for the first visible workspace scan; later UI toggles own expansion state.

- [ ] **Step 6: Run the focused tests, format, and commit.**

Run: `cargo test file::`

Expected: extension, UTF-8, version, and scanner tests pass.

```bash
cargo fmt
git add src/state.rs src/file
git commit -m "feat: add Markdown file service and tree scanner"
```

### Task 3: Add persistent Recent Files

**Files:**

- Create: `src/config/mod.rs`
- Create: `src/config/recent.rs`
- Modify: `src/state.rs`
- Test: `src/config/recent.rs` unit tests

**Interfaces:**

- Produces `RecentFiles::load`, `record_success`, `remove`, `paths`, and `save` for the root entity.
- Produces platform-independent ordering logic and accepts an injected configuration directory for tests.
- `RecentFiles::in_memory(capacity)` is a test-only constructor whose supplied paths are treated as already canonical; production `record_success` canonicalizes existing filesystem paths before storing them.

- [ ] **Step 1: Write ordering, capacity, and missing-entry tests.**

```rust
#[test]
fn successful_open_moves_existing_path_to_front_without_duplicates() {
    let mut recent = RecentFiles::in_memory(2);
    recent.record_success(path("A.md"));
    recent.record_success(path("B.md"));
    recent.record_success(path("A.md"));
    assert_eq!(recent.paths(), &[path("A.md"), path("B.md")]);
}

#[test]
fn capacity_is_twenty_and_remove_drops_missing_entries() {
    let mut recent = RecentFiles::in_memory(20);
    for index in 0..21 { recent.record_success(path(&format!("{index}.md"))); }
    assert_eq!(recent.paths().len(), 20);
    recent.remove(&path("10.md"));
    assert!(!recent.paths().contains(&path("10.md")));
}
```

- [ ] **Step 2: Run the focused tests and verify the new API fails.**

Run: `cargo test config::recent::tests::successful_open_moves_existing_path_to_front_without_duplicates`

Expected: compilation fails before the implementation exists.

- [ ] **Step 3: Implement injected-path JSON persistence.**

Define a serializable configuration wrapper and keep the Markdown contents out of it:

```rust
#[derive(Serialize, Deserialize, Default)]
struct RecentConfig { recent_files: Vec<PathBuf> }

pub struct RecentFiles {
    paths: Vec<PathBuf>,
    config_path: Option<PathBuf>,
    capacity: usize,
}
```

`load` receives `directories::ProjectDirs::from("com", "Mieli", "Mieli")`, creates the returned config directory only when saving, and treats malformed or unreadable config as an empty list plus a notification-ready error. `record_success` canonicalizes existing paths, removes duplicates, inserts at index zero, truncates to 20, and persists. `in_memory` intentionally skips filesystem canonicalization so ordering tests can use logical paths. `remove` persists after deletion. `open_recent` will check `path.exists()` before opening and remove missing entries.

- [ ] **Step 4: Run tests and commit the persistence layer.**

Run: `cargo test config::recent::tests`

Expected: all ordering, capacity, and persistence round-trip tests pass.

```bash
cargo fmt
git add src/config src/state.rs
git commit -m "feat: persist recent Markdown files"
```

### Task 4: Implement watcher translation and autosave eligibility

**Files:**

- Create: `src/file/watcher.rs`
- Create: `src/autosave.rs`
- Modify: `src/file/mod.rs`
- Modify: `src/state.rs`
- Test: `src/autosave.rs` and `src/file/watcher.rs` unit tests

**Interfaces:**

- Produces `FileSystemEvent`, `FileWatcherService`, `AutosaveKey`, and `autosave_is_current` for `app.rs`.
- Keeps OS callbacks separate from app state mutation.

- [ ] **Step 1: Write watcher and autosave state-machine tests.**

```rust
#[test]
fn stale_generation_cannot_save_a_newer_edit() {
    let key = AutosaveKey { tab_id: TabId(7), generation: 2, path: path("A.md") };
    assert!(autosave_is_current(&key, TabId(7), 2, Path::new("A.md"), true));
    assert!(!autosave_is_current(&key, TabId(7), 1, Path::new("A.md"), true));
    assert!(!autosave_is_current(&key, TabId(7), 2, Path::new("B.md"), true));
    assert!(!autosave_is_current(&key, TabId(7), 2, Path::new("A.md"), false));
}

#[test]
fn notify_events_are_reduced_to_changed_created_removed_or_error() {
    assert_eq!(translate_event(EventKind::Modify(_), path("A.md")), FileSystemEvent::Changed(path("A.md")));
    assert_eq!(translate_event(EventKind::Create(_), path("A.md")), FileSystemEvent::Created(path("A.md")));
    assert_eq!(translate_event(EventKind::Remove(_), path("A.md")), FileSystemEvent::Removed(path("A.md")));
}
```

- [ ] **Step 2: Run the focused tests and verify failure.**

Run: `cargo test autosave::tests::stale_generation_cannot_save_a_newer_edit file::watcher::tests::notify_events_are_reduced_to_changed_created_removed_or_error`

Expected: compilation fails for the missing state-machine functions.

- [ ] **Step 3: Implement the generation key and eligibility check.**

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutosaveKey { pub tab_id: TabId, pub generation: u64, pub path: PathBuf }

pub fn autosave_is_current(
    key: &AutosaveKey,
    tab_id: TabId,
    generation: u64,
    path: &Path,
    dirty: bool,
) -> bool {
    key.tab_id == tab_id && key.generation == generation && key.path == path && dirty
}
```

Do not put a timer in this pure module; GPUI task ownership belongs to `Mieli`.

- [ ] **Step 4: Implement `FileWatcherService`.**

```rust
pub struct FileWatcherService {
    watcher: notify::RecommendedWatcher,
    receiver: std::sync::mpsc::Receiver<FileSystemEvent>,
    watched: BTreeSet<PathBuf>,
}

impl FileWatcherService {
    pub fn new() -> Result<Self, WatchError>;
    pub fn watch_workspace(&mut self, root: &Path) -> Result<(), WatchError>;
    pub fn watch_file_parent(&mut self, path: &Path) -> Result<(), WatchError>;
    pub fn drain(&self) -> Vec<FileSystemEvent>;
}
```

The `notify` callback only converts each event and sends it on the channel. `watch_workspace` uses `RecursiveMode::Recursive`; file-parent watches use `RecursiveMode::NonRecursive`. Deduplicate watched directories with canonical paths. Never watch a home directory root.

- [ ] **Step 5: Run tests and commit watcher/autosave primitives.**

Run: `cargo test autosave:: file::watcher::`

Expected: generation checks and event translation pass without opening a GPUI window.

```bash
cargo fmt
git add src/autosave.rs src/file/watcher.rs src/file/mod.rs src/state.rs
git commit -m "feat: add watcher and autosave primitives"
```

### Task 5: Build the root state and Bezel editor tab lifecycle

**Files:**

- Modify: `src/state.rs`
- Create: `src/app.rs`
- Modify: `src/lib.rs`
- Test: `src/app.rs` pure transition tests

**Interfaces:**

- Produces `Mieli`, `EditorTab`, `Modal`, `Notification`, and root transitions used by the UI and actions.
- Consumes `editor::Editor`, `file::{read_markdown, write_markdown, scan_markdown_tree, disk_version}`, `RecentFiles`, `FileWatcherService`, and `autosave_is_current`.

- [ ] **Step 1: Write pure tab and state transition tests.**

Keep file opening and tab identity testable without GPUI by testing the canonical-path index helper separately from entity construction:

```rust
#[derive(Default)]
struct OpenTabPaths { paths: Vec<PathBuf>, next_id: TabId }

impl OpenTabPaths {
    fn insert(&mut self, path: PathBuf) -> TabId {
        if let Some((index, _)) = self.paths.iter().enumerate().find(|(_, existing)| **existing == path) {
            return TabId(index as u64 + 1);
        }
        self.paths.push(path);
        self.next_id = TabId(self.paths.len() as u64);
        self.next_id
    }

    fn len(&self) -> usize { self.paths.len() }
}

#[derive(Clone)]
struct DirtyTestState { saved: String, current: String, dirty: bool }

impl DirtyTestState {
    fn clean(source: &str) -> Self { Self { saved: source.into(), current: source.into(), dirty: false } }
    fn mark_edited(&mut self, source: &str) { self.current = source.into(); self.dirty = self.current != self.saved; }
    fn save_failed(&mut self) { self.dirty = self.current != self.saved; }
    fn save_succeeded(&mut self) { self.saved = self.current.clone(); self.dirty = false; }
}

#[test]
fn canonical_paths_prevent_duplicate_tabs() {
    let mut paths = OpenTabPaths::default();
    let first = paths.insert(canonical("notes/README.md"));
    let second = paths.insert(canonical("notes/./README.md"));
    assert_eq!(first, second);
    assert_eq!(paths.len(), 1);
}

#[test]
fn failed_save_keeps_dirty_and_success_clears_it() {
    let mut tab = DirtyTestState::clean("# A");
    tab.mark_edited("# B");
    assert!(tab.dirty);
    tab.save_failed();
    assert!(tab.dirty);
    tab.save_succeeded();
    assert!(!tab.dirty);
}
```

- [ ] **Step 2: Run the focused tests and verify they fail.**

Run: `cargo test app::tests::canonical_paths_prevent_duplicate_tabs app::tests::failed_save_keeps_dirty_and_success_clears_it`

Expected: compilation fails because the transition helpers do not exist.

- [ ] **Step 3: Define tab and root state.**

Use one editor entity and one saved baseline per tab:

```rust
pub struct EditorTab {
    pub id: TabId,
    pub path: PathBuf,
    pub title: String,
    pub editor: gpui::Entity<editor::Editor>,
    pub saved_source: String,
    pub disk_version: DiskVersion,
    pub dirty: bool,
    pub disk_state: DiskState,
    pub autosave_generation: u64,
}

pub struct AppState {
    pub workspace_root: Option<PathBuf>,
    pub sidebar_visible: bool,
    pub file_tree: Vec<FileTreeNode>,
    pub tabs: Vec<EditorTab>,
    pub active_tab: Option<TabId>,
    pub recent_files: RecentFiles,
    pub auto_save_enabled: bool,
}
```

`Mieli` additionally owns modal/notification state, the watcher, GPUI autosave tasks keyed by `TabId`, and the next ID counter. `EditorTab` path/title changes only after a successful Save As.

- [ ] **Step 4: Implement `Mieli::new` and editor creation with real Bezel API.**

Opening an existing file performs UTF-8 read, canonicalization, version capture, and entity creation:

```rust
let scroll = gpui::ScrollHandle::new();
let source = read_markdown(&path)?;
let baseline = source.clone();
let editor = cx.new({
    let scroll = scroll.clone();
    move |cx| editor::Editor::new(&source, cx).with_scroll(scroll)
});
cx.observe(&editor, move |view, _, cx| view.editor_changed(tab_id, cx)).detach();
```

`editor_changed` reads `editor.source()` once, compares it with `saved_source`, sets `dirty`, increments the tab generation, and calls `cx.notify()`. Reopening a duplicate canonical path only activates the existing tab.

- [ ] **Step 5: Implement save and Save As transitions.**

`save_tab` must read the current entity source, call `write_markdown`, and update `saved_source`, `disk_version`, `dirty`, and `disk_state` only after the write succeeds. `save_as` uses a selected destination, adds `.md` when the destination has neither Markdown extension, writes first, then changes path/title and refreshes the workspace tree. A failed write sets `notification` and leaves all old tab identity fields untouched.

- [ ] **Step 6: Implement open, close, switch, and workspace transitions.**

Opening a file records Recent Files only after a successful read and tab creation. Switching tabs attempts an immediate save of the tab losing focus. Closing a clean tab removes its autosave task and entity; closing a dirty tab sets `Modal::CloseTab(TabId)` and does not remove content until Save or Don't Save is chosen. Open Folder sets `workspace_root`, scans recursively, refreshes the watcher, and leaves tabs outside the workspace open.

- [ ] **Step 7: Run app-state tests and commit the lifecycle.**

Run: `cargo test app::`

Expected: canonical deduplication, dirty preservation, successful clean-up, and workspace independence pass.

```bash
cargo fmt
git add src/app.rs src/state.rs src/lib.rs
git commit -m "feat: add multi-tab editor state"
```

### Task 6: Add centralized actions, native menus, and keyboard bindings

**Files:**

- Create: `src/actions.rs`
- Modify: `src/main.rs`
- Modify: `src/app.rs`
- Test: `src/actions.rs` action-name and binding table tests where GPUI test support permits

**Interfaces:**

- Produces the application action types and `install(cx)` function.
- Routes menu commands and key bindings to methods on `Mieli`; components do not own independent keyboard handlers.

- [ ] **Step 1: Define unit actions and recent positions.**

Use GPUI's actual unit-action macro, including bounded recent entries so native menu actions remain serializable by GPUI:

```rust
gpui::actions!(mieli, [
    OpenFile, OpenFolder, Save, SaveAs, SaveAll, CloseTab,
    ToggleSidebar, NextTab, PreviousTab, RefreshTree, Quit,
    OpenRecent1, OpenRecent2, OpenRecent3, OpenRecent4, OpenRecent5,
    OpenRecent6, OpenRecent7, OpenRecent8, OpenRecent9, OpenRecent10,
    OpenRecent11, OpenRecent12, OpenRecent13, OpenRecent14, OpenRecent15,
    OpenRecent16, OpenRecent17, OpenRecent18, OpenRecent19, OpenRecent20,
]);
```

- [ ] **Step 2: Implement platform bindings with exact chords.**

`install(cx)` binds Cmd on macOS and Ctrl elsewhere for Save, Save As, Close Tab, Toggle Sidebar, and Quit; it binds Ctrl+Tab and Ctrl+Shift+Tab for tab navigation on every desktop platform. Bind `RefreshTree` to a non-conflicting app shortcut only if compilation confirms the chosen chord is free; the File menu remains the guaranteed path.

- [ ] **Step 3: Install the File menu and dynamic Open Recent entries.**

Create a `File` menu with Open File, Open Folder, Open Recent submenu, Save, Save As, Save All, Close Tab, and Exit. Rebuild the menu after Recent Files changes. The recent action handler maps the unit action type to its zero-based index and calls `open_recent(index)`; a missing path is removed and reported.

- [ ] **Step 4: Route actions through `Mieli`.**

Register root action listeners with `cx.listener` on the root element or root context. Each listener calls one method such as `open_file`, `save_active`, `close_active`, or `toggle_sidebar`; no file I/O is placed in `tabs.rs` or `file_tree.rs`.

- [ ] **Step 5: Run check and commit action wiring.**

Run: `cargo check`

Expected: all action macro expansions use the same `bezel-gpui` crate as Bezel's editor.

```bash
cargo fmt
git add src/actions.rs src/main.rs src/app.rs Cargo.lock
git commit -m "feat: add Mieli actions and menus"
```

### Task 7: Build the native GPUI interface

**Files:**

- Create: `src/ui/mod.rs`
- Create: `src/ui/root.rs`
- Create: `src/ui/sidebar.rs`
- Create: `src/ui/file_tree.rs`
- Create: `src/ui/tabs.rs`
- Create: `src/ui/dialogs.rs`
- Modify: `src/app.rs`
- Test: GPUI compile check and pure tree-row helper tests

**Interfaces:**

- Produces the rendered empty state, toolbar, sidebar, tab strip, editor area, notification, and modal cards.
- Consumes only root actions and public state transitions; it never calls `std::fs` directly.

- [ ] **Step 1: Write pure row-model tests for the file tree.**

```rust
#[test]
fn active_path_is_selected_and_collapsed_children_are_hidden() {
    let tree = sample_tree();
    assert!(visible_rows(&tree, true).iter().any(|row| row.path == path("docs/api.md") && row.selected));
    assert!(!visible_rows(&tree_with_docs_collapsed(), true).iter().any(|row| row.path == path("docs/api.md")));
}
```

- [ ] **Step 2: Render the root shell with no file open.**

Use Bezel `Theme::of(cx)` for colors and typography. The empty state must show exactly the title “Mieli”, the copy “Open a Markdown file or folder to start writing.”, and buttons “Open File” and “Open Folder”. The sidebar starts hidden and the editor area fills the remaining window.

- [ ] **Step 3: Render the fixed sidebar and recursive tree.**

When visible, render a 240px sidebar inside the main horizontal layout and put its contents in `.overflow_y_scroll()`. Directories render before files, carry an expansion toggle, and retain expansion state by path across tree refresh when possible. Markdown rows call `open_path(path)` on click and compare their canonical path with the active tab for selected styling.

- [ ] **Step 4: Render tabs and mount the active Bezel editor entity.**

Each tab renders its basename, a dirty dot when `dirty`, and a close button. Tab selection calls `activate_tab(id)`. The active editor is rendered inside a vertical scroll container using the stored entity; do not convert `Editor::source()` into a separate text widget.

- [ ] **Step 5: Render Bezel modal cards for all safety decisions.**

Use `ui::popover::modal("mieli-modal", window.viewport_size(), card.into_any_element(), on_dismiss)`, with `dialog_card`, `dialog_title`, `dialog_body`, and `Theme::button` inside `card`. Implement exact choices:

- Dirty tab: Save, Don't Save, Cancel.
- External conflict: Keep Mine, Reload from Disk, Cancel.
- Deleted file: Keep Open, Close.
- Failed shutdown: Cancel, Quit Anyway.

Buttons call root transitions and preserve dirty content on Cancel or failed Save.

- [ ] **Step 6: Run compile/tests and commit the UI.**

Run: `cargo test`

Expected: pure row tests pass and the full application compiles with no HTML/CSS/WebView dependency.

```bash
cargo fmt
git add src/ui src/app.rs
git commit -m "feat: add native Mieli workspace UI"
```

### Task 8: Integrate debounced autosave, watcher events, and close protection

**Files:**

- Modify: `src/app.rs`
- Modify: `src/ui/dialogs.rs`
- Modify: `src/ui/root.rs`
- Modify: `src/main.rs`
- Test: `src/app.rs` watcher/close transition tests

**Interfaces:**

- Completes the user-safety flows over the state, file, watcher, autosave, action, and UI layers.

- [ ] **Step 1: Write failing external-change and close tests.**

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
struct ConflictTestState { source: String, saved: String, dirty: bool, disk_state: DiskState }

impl ConflictTestState {
    fn clean(source: &str) -> Self { Self { source: source.into(), saved: source.into(), dirty: false, disk_state: DiskState::Synced } }
    fn apply_external_change(&mut self, disk_source: &str) -> ExternalResolution {
        if self.dirty { self.disk_state = DiskState::Conflict; ExternalResolution::Conflict }
        else { self.source = disk_source.into(); self.saved = self.source.clone(); self.disk_state = DiskState::Synced; ExternalResolution::Reloaded }
    }
    fn mark_dirty(&mut self, source: &str) { self.source = source.into(); self.dirty = true; }
    fn apply_removed_event(&mut self) { self.disk_state = DiskState::Deleted; self.dirty = true; }
    fn keep_deleted_open(&mut self) { self.dirty = true; self.disk_state = DiskState::Deleted; }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExternalResolution { Reloaded, Conflict }

#[test]
fn clean_external_change_reloads_and_dirty_external_change_conflicts() {
    let mut state = ConflictTestState::clean("# A");
    assert_eq!(state.apply_external_change("# B"), ExternalResolution::Reloaded);
    state.mark_dirty("# Local");
    assert_eq!(state.apply_external_change("# Disk"), ExternalResolution::Conflict);
}

#[test]
fn deleted_file_marks_dirty_and_keep_open_preserves_content() {
    let mut state = ConflictTestState::clean("# A");
    state.apply_removed_event();
    assert_eq!(state.disk_state, DiskState::Deleted);
    assert!(state.dirty);
    state.keep_deleted_open();
    assert_eq!(state.source, "# A");
}
```

- [ ] **Step 2: Run focused tests and verify they fail before integration.**

Run: `cargo test app::tests::clean_external_change_reloads_and_dirty_external_change_conflicts app::tests::deleted_file_marks_dirty_and_keep_open_preserves_content`

Expected: the new external-resolution transition methods are not yet implemented.

- [ ] **Step 3: Schedule one GPUI timer per tab generation.**

On an editor notification, drop/replace the previous task for that TabId and store the new `Task<()>`:

```rust
let key = AutosaveKey { tab_id, generation, path: path.clone() };
let task = cx.spawn(move |this, cx| async move {
    cx.background_executor().timer(Duration::from_millis(800)).await;
    this.update(cx, |view, cx| view.run_autosave(key, cx)).ok();
});
self.autosave_tasks.insert(tab_id, task);
```

`run_autosave` re-reads the tab, validates `autosave_is_current`, serializes `editor.source()`, and updates the saved baseline only on a successful `write_markdown`. Removing a tab drops its stored task.

- [ ] **Step 4: Poll and handle watcher events without mutating from callbacks.**

Start a repeating GPUI task that waits 250ms, drains watcher events, handles them on `Mieli`, and schedules the next poll while the entity is alive. For each changed path, compute a fresh `DiskVersion` and ignore an event matching the tab's last saved version. For a different version, read the new UTF-8 source before the clean reload branch:

```rust
let source = read_markdown(&path).map_err(|error| self.notify_file_error(error));
match (tab.dirty, version.exists) {
    (false, true) => self.reload_clean_tab(tab_id, source?, version, cx),
    (true, true) => self.enter_conflict(tab_id, cx),
    (_, false) => self.enter_deleted(tab_id, cx),
}
```

Create/delete events under the workspace trigger a tree rescan. Events for external tab parents are still handled when no workspace is active.

- [ ] **Step 5: Implement conflict, reload, delete, and shutdown decisions.**

Reload replaces the editor entity with `Editor::new` and a fresh scroll handle, resets `saved_source`, `disk_version`, `dirty`, and `DiskState::Synced`, and notifies the root. Keep Mine leaves the editor source intact, marks `DiskState::Conflict`, and lets the next explicit or autosave write overwrite disk. Cancel leaves conflict visible. Keep Open after deletion marks `dirty` and keeps the entity; Close removes the tab without a second discard prompt because the deletion decision already explicitly chose Close.

Register `Window::on_window_should_close`. If autosave is enabled, call Save All synchronously before returning; return `true` only when every dirty tab saved. Otherwise set the quit modal and return `false`. Quit Anyway sets an allow flag and calls `cx.quit()`; the next close callback returns `true`. A canceled quit leaves every dirty tab open.

- [ ] **Step 6: Run integration tests and commit safety flows.**

Run: `cargo test app::`

Expected: clean reload, dirty conflict, deleted-file protection, autosave failure preservation, and close decisions pass.

```bash
cargo fmt
git add src/app.rs src/ui/dialogs.rs src/ui/root.rs src/main.rs
git commit -m "feat: protect edits with autosave and file watching"
```

### Task 9: Document behavior, round-trip normalization, and platform verification

**Files:**

- Create or modify: `README.md`
- Modify: `src/file/io.rs` tests if round-trip coverage reveals a missing case
- Modify: `src/app.rs` tests if final behavior exposes a transition gap

**Interfaces:**

- Produces the final user-facing documentation and a reproducible verification record.

- [ ] **Step 1: Add Markdown round-trip tests using Bezel's actual model.**

Test a corpus containing headings, bold/italic marks, links, unordered and ordered lists, quotes, inline/fenced code, task items, tables, and blank lines:

```rust
#[test]
fn bezel_round_trip_reaches_a_fixed_point() {
    let source = "# Title\n\n- [ ] item\n- [x] done\n\n```rust\nfn main() {}\n```";
    let first = markdown::serialize(&markdown::parse(source));
    let second = markdown::serialize(&markdown::parse(&first));
    assert_eq!(first, second);
}
```

Record any byte-level normalization observed by comparing `source` and `first`; do not claim byte-identical preservation if Bezel normalizes spacing, blank lines, indentation, or fences.

- [ ] **Step 2: Write README usage and safety documentation.**

Document opening files/folders, tabs, shortcuts, autosave timing, conflict choices, Recent Files, supported extensions, invalid UTF-8 behavior, dependency reasons, Bezel data flow, and exact Known Limitations. Include the inspected Bezel revision and state that `rfd`, `notify`, `directories`, and JSON are supporting infrastructure rather than Markdown storage.

- [ ] **Step 3: Run the complete local verification.**

Run in order:

```bash
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: all commands succeed, or any unavailable clippy/platform condition is recorded with its actual error and not hidden.

- [ ] **Step 4: Check available Windows and Linux targets.**

Run `rustup target list --installed` and, for each installed non-host desktop target, run `cargo check --target <installed-target>`. If a target is unavailable, report it as not runtime-tested. Do not install heavyweight toolchains as part of the app implementation.

- [ ] **Step 5: Perform the manual acceptance smoke test.**

Exercise the 15 required flows: empty state; open/edit/save; autosave; Save As; recursive folder tree; multiple tabs and deduplication; sidebar show/hide; dirty close choices; clean external reload; dirty external conflict; external deletion; Recent Files after restart; and autosave failure with dirty preservation.

- [ ] **Step 6: Commit documentation and verification notes.**

```bash
git add README.md src
git commit -m "docs: document Mieli behavior and verification"
```

## Final review checklist

- [ ] `cargo fmt --check` is clean.
- [ ] `cargo check` is clean.
- [ ] `cargo test` is clean, including scanner, Recent Files, dirty, autosave, watcher, conflict, delete, and Bezel round-trip tests.
- [ ] `cargo clippy` result is recorded.
- [ ] No production filesystem path uses `unwrap()`.
- [ ] No duplicate Tab can be created for one canonical path.
- [ ] Save failures leave `dirty == true`.
- [ ] Stale autosave tasks cannot target another TabId or path.
- [ ] Clean external changes reload; dirty external changes never overwrite local content.
- [ ] Recursive tree refreshes only on workspace/file events, Save As, and explicit refresh.
- [ ] README names every dependency and records normalization and platform limitations.

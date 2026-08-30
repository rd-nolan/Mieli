use std::{
    collections::{BTreeSet, HashMap},
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use gpui::AppContext as _;
use gpui::prelude::*;

use crate::{
    actions,
    autosave::{AutosaveKey, autosave_is_current},
    config::recent::RecentFiles,
    file::{
        FileError,
        io::{canonicalize_path, disk_version, is_markdown_file, read_markdown, write_markdown},
        scanner::scan_markdown_tree,
        watcher::{FileSystemEvent, FileWatcherService, WatchError},
    },
    state::{
        AppState, DiskState, DiskVersion, EditorTab, FileTreeNode, Modal, Notification, TabId,
    },
};

#[derive(Default)]
struct OpenTabPaths {
    paths: HashMap<PathBuf, TabId>,
    next_id: u64,
}

impl OpenTabPaths {
    fn insert(&mut self, path: PathBuf) -> TabId {
        if let Some(tab_id) = self.paths.get(&path) {
            return *tab_id;
        }

        let tab_id = self.allocate();
        self.paths.insert(path, tab_id);
        tab_id
    }

    fn allocate(&mut self) -> TabId {
        self.next_id += 1;
        TabId(self.next_id)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.paths.len()
    }

    fn get(&self, path: &Path) -> Option<TabId> {
        self.paths.get(path).copied()
    }

    fn remove(&mut self, path: &Path) {
        self.paths.remove(path);
    }

    fn replace(&mut self, old_path: &Path, new_path: PathBuf, tab_id: TabId) {
        if !old_path.as_os_str().is_empty() {
            self.paths.remove(old_path);
        }
        self.paths.insert(new_path, tab_id);
    }
}

fn dirty_for_source(saved_source: &str, current_source: &str) -> bool {
    current_source != saved_source
}

fn apply_successful_save(saved_source: &mut String, dirty: &mut bool, current_source: String) {
    *saved_source = current_source;
    *dirty = false;
}

fn markdown_destination(path: &Path) -> PathBuf {
    if is_markdown_file(path) {
        return path.to_path_buf();
    }

    let mut destination: OsString = path.as_os_str().to_owned();
    destination.push(".md");
    PathBuf::from(destination)
}

fn apply_save_as_identity(path: &mut PathBuf, title: &mut String, destination: PathBuf) {
    *title = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| destination.display().to_string());
    *path = destination;
}

struct SaveTabState<'a> {
    path: &'a mut PathBuf,
    title: &'a mut String,
    saved_source: &'a mut String,
    disk_version: &'a mut DiskVersion,
    dirty: &'a mut bool,
    disk_state: &'a mut DiskState,
}

impl<'a> SaveTabState<'a> {
    fn from_tab(tab: &'a mut EditorTab) -> Self {
        Self {
            path: &mut tab.path,
            title: &mut tab.title,
            saved_source: &mut tab.saved_source,
            disk_version: &mut tab.disk_version,
            dirty: &mut tab.dirty,
            disk_state: &mut tab.disk_state,
        }
    }

    fn commit_save(&mut self, current_source: String, version: DiskVersion) {
        apply_successful_save(self.saved_source, self.dirty, current_source);
        *self.disk_version = version;
        *self.disk_state = DiskState::Synced;
    }
}

fn save_as_candidate(destination: &Path) -> Result<PathBuf, FileError> {
    match std::fs::symlink_metadata(destination) {
        Ok(_) => {
            return std::fs::canonicalize(destination)
                .map_err(|error| FileError::from_io(destination, "canonicalize", error));
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(FileError::from_io(destination, "canonicalize", error));
        }
        Err(_) => {}
    }

    let file_name = destination
        .file_name()
        .ok_or_else(|| FileError::other(destination, "canonicalize"))?;
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canonical_parent = std::fs::canonicalize(parent)
        .map_err(|error| FileError::from_io(destination, "canonicalize", error))?;
    Ok(canonical_parent.join(file_name))
}

fn save_tab_transition(
    mut tab: SaveTabState<'_>,
    current_source: String,
    writer: impl FnOnce(&Path, &str) -> Result<DiskVersion, FileError>,
) -> Result<(), LifecycleError> {
    let version = writer(tab.path, &current_source)?;
    tab.commit_save(current_source, version);
    Ok(())
}

fn autosave_transition(
    key: &AutosaveKey,
    tab_id: TabId,
    generation: u64,
    tab: SaveTabState<'_>,
    current_source: String,
    writer: impl FnOnce(&Path, &str) -> Result<DiskVersion, FileError>,
) -> Result<bool, LifecycleError> {
    if !autosave_is_current(key, tab_id, generation, tab.path, *tab.dirty) {
        return Ok(false);
    }

    save_tab_transition(tab, current_source, writer)?;
    Ok(true)
}

fn save_as_transition(
    tab_id: TabId,
    mut tab: SaveTabState<'_>,
    open_tab_paths: &mut OpenTabPaths,
    destination: PathBuf,
    current_source: String,
    writer: impl FnOnce(&Path, &str) -> Result<DiskVersion, FileError>,
) -> Result<PathBuf, LifecycleError> {
    let destination = markdown_destination(&destination);
    let candidate = save_as_candidate(&destination)?;
    if let Some(existing) = open_tab_paths.get(&candidate) {
        if existing != tab_id {
            return Err(LifecycleError::PathAlreadyOpen {
                path: candidate,
                tab_id: existing,
            });
        }
    }

    let version = writer(&candidate, &current_source)?;
    let canonical = canonicalize_path(&candidate)?;
    let old_path = tab.path.clone();
    tab.commit_save(current_source, version);
    apply_save_as_identity(tab.path, tab.title, canonical.clone());
    open_tab_paths.replace(&old_path, canonical.clone(), tab_id);
    Ok(canonical)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CloseAction {
    Remove,
    RequestDecision,
}

struct ExternalChangeState<'a> {
    saved_source: &'a mut String,
    disk_version: &'a mut DiskVersion,
    dirty: &'a mut bool,
    disk_state: &'a mut DiskState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ExternalResolution {
    Reloaded(String),
    Conflict,
}

fn apply_external_change(
    state: ExternalChangeState<'_>,
    disk_source: String,
    version: DiskVersion,
) -> ExternalResolution {
    if *state.dirty {
        *state.disk_state = DiskState::Conflict;
        return ExternalResolution::Conflict;
    }

    *state.saved_source = disk_source.clone();
    *state.disk_version = version;
    *state.dirty = false;
    *state.disk_state = DiskState::Synced;
    ExternalResolution::Reloaded(disk_source)
}

fn load_external_change(
    state: ExternalChangeState<'_>,
    path: &Path,
    version: DiskVersion,
    reader: impl FnOnce(&Path) -> Result<String, FileError>,
) -> Result<ExternalResolution, FileError> {
    let disk_source = reader(path)?;
    Ok(apply_external_change(state, disk_source, version))
}

fn apply_removed_event(dirty: &mut bool, disk_state: &mut DiskState) {
    *dirty = true;
    *disk_state = DiskState::Deleted;
}

fn keep_deleted_open(dirty: &mut bool, disk_state: &mut DiskState) {
    *dirty = true;
    *disk_state = DiskState::Deleted;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowCloseAction {
    Allow,
    SaveAll,
    RequestDecision,
}

fn window_close_action(
    allow_quit: bool,
    has_dirty_tabs: bool,
    autosave_enabled: bool,
) -> WindowCloseAction {
    if allow_quit || !has_dirty_tabs {
        WindowCloseAction::Allow
    } else if autosave_enabled {
        WindowCloseAction::SaveAll
    } else {
        WindowCloseAction::RequestDecision
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TabDirection {
    Next,
    Previous,
}

fn adjacent_tab_id(
    tabs: &[TabId],
    active_tab: Option<TabId>,
    direction: TabDirection,
) -> Option<TabId> {
    if tabs.is_empty() {
        return None;
    }

    let current = active_tab.and_then(|active| tabs.iter().position(|tab| *tab == active));
    let index = match (current, direction) {
        (Some(index), TabDirection::Next) => (index + 1) % tabs.len(),
        (Some(0), TabDirection::Previous) | (None, TabDirection::Previous) => tabs.len() - 1,
        (Some(index), TabDirection::Previous) => index - 1,
        (None, TabDirection::Next) => 0,
    };
    Some(tabs[index])
}

fn close_action(dirty: bool) -> CloseAction {
    if dirty {
        CloseAction::RequestDecision
    } else {
        CloseAction::Remove
    }
}

fn apply_workspace_state(
    workspace_root: &mut Option<PathBuf>,
    file_tree: &mut Vec<FileTreeNode>,
    root: PathBuf,
    tree: Vec<FileTreeNode>,
) {
    *workspace_root = Some(root);
    *file_tree = tree;
}

#[derive(Debug)]
pub enum LifecycleError {
    File(FileError),
    Watch(WatchError),
    NoActiveTab,
    MissingTab(TabId),
    SaveAsRequired(TabId),
    PathAlreadyOpen {
        path: PathBuf,
        tab_id: TabId,
    },
    MissingRecentEntry(usize),
    MissingRecentFile {
        path: PathBuf,
        cleanup_error: Option<String>,
    },
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(error) => error.fmt(f),
            Self::Watch(error) => error.fmt(f),
            Self::NoActiveTab => f.write_str("There is no active editor tab."),
            Self::MissingTab(tab_id) => write!(f, "Could not find editor tab {}.", tab_id.0),
            Self::SaveAsRequired(tab_id) => {
                write!(f, "Tab {} must be saved with Save As first.", tab_id.0)
            }
            Self::PathAlreadyOpen { path, tab_id } => {
                write!(f, "{} is already open in tab {}.", path.display(), tab_id.0)
            }
            Self::MissingRecentEntry(index) => {
                write!(f, "Recent file entry {} is no longer available.", index + 1)
            }
            Self::MissingRecentFile {
                path,
                cleanup_error,
            } => {
                write!(
                    f,
                    "Could not open recent file {}: file not found. The entry was removed",
                    path.display()
                )?;
                if let Some(error) = cleanup_error {
                    write!(f, ", but the recent-files list could not be saved: {error}")?;
                }
                f.write_str(".")
            }
        }
    }
}

impl std::error::Error for LifecycleError {}

impl From<FileError> for LifecycleError {
    fn from(error: FileError) -> Self {
        Self::File(error)
    }
}

impl From<WatchError> for LifecycleError {
    fn from(error: WatchError) -> Self {
        Self::Watch(error)
    }
}

pub struct Mieli {
    pub state: AppState,
    pub modal: Option<Modal>,
    pub notification: Option<Notification>,
    watcher: Option<FileWatcherService>,
    _watcher_poll_task: gpui::Task<()>,
    autosave_tasks: HashMap<TabId, gpui::Task<()>>,
    editor_scrolls: HashMap<TabId, gpui::ScrollHandle>,
    open_tab_paths: OpenTabPaths,
    allow_quit: bool,
}

impl Mieli {
    pub fn new(cx: &mut gpui::Context<Self>) -> Self {
        let (recent_files, recent_error) = RecentFiles::load();
        let (watcher, watcher_error) = match FileWatcherService::new() {
            Ok(watcher) => (Some(watcher), None),
            Err(error) => (None, Some(error)),
        };
        let notification = watcher_error
            .map(Notification::error)
            .or_else(|| recent_error.map(Notification::error));

        let watcher_poll_task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if this.update(cx, |view, cx| view.poll_watcher(cx)).is_err() {
                    break;
                }
            }
        });
        let this = Self {
            state: AppState {
                workspace_root: None,
                sidebar_visible: false,
                file_tree: Vec::new(),
                tabs: Vec::new(),
                active_tab: None,
                recent_files,
                auto_save_enabled: true,
            },
            modal: None,
            notification,
            watcher,
            _watcher_poll_task: watcher_poll_task,
            autosave_tasks: HashMap::new(),
            editor_scrolls: HashMap::new(),
            open_tab_paths: OpenTabPaths::default(),
            allow_quit: false,
        };
        actions::set_file_menu(cx, this.state.recent_files.paths());
        this
    }

    pub fn new_tab(&mut self, cx: &mut gpui::Context<Self>) -> TabId {
        let tab_id = self.open_tab_paths.allocate();
        let source = String::new();
        let (editor, scroll) = Self::create_editor(tab_id, source.clone(), cx);
        let title = if self.state.tabs.iter().any(|tab| tab.title == "Untitled") {
            format!("Untitled {}", tab_id.0)
        } else {
            String::from("Untitled")
        };

        self.state.tabs.push(EditorTab {
            id: tab_id,
            path: PathBuf::new(),
            title,
            editor,
            saved_source: source,
            disk_version: Default::default(),
            dirty: false,
            disk_state: DiskState::Synced,
            autosave_generation: 0,
        });
        self.editor_scrolls.insert(tab_id, scroll);
        self.state.active_tab = Some(tab_id);
        cx.notify();
        tab_id
    }

    pub fn open_file(
        &mut self,
        path: PathBuf,
        cx: &mut gpui::Context<Self>,
    ) -> Result<TabId, LifecycleError> {
        let canonical = self.file_result(canonicalize_path(&path))?;
        if let Some(tab_id) = self.open_tab_paths.get(&canonical) {
            self.state.active_tab = Some(tab_id);
            cx.notify();
            return Ok(tab_id);
        }

        let source = self.file_result(read_markdown(&canonical))?;
        let version = self.file_result(disk_version(&canonical))?;
        let tab_id = self.open_tab_paths.insert(canonical.clone());
        let (editor, scroll) = Self::create_editor(tab_id, source.clone(), cx);
        let title = display_title(&canonical);

        self.state.tabs.push(EditorTab {
            id: tab_id,
            path: canonical.clone(),
            title,
            editor,
            saved_source: source,
            disk_version: version,
            dirty: false,
            disk_state: DiskState::Synced,
            autosave_generation: 0,
        });
        self.editor_scrolls.insert(tab_id, scroll);
        self.state.active_tab = Some(tab_id);

        if let Err(error) = self.state.recent_files.record_success(&canonical) {
            self.notification = Some(Notification::error(error));
        }
        actions::set_file_menu(cx, self.state.recent_files.paths());
        self.watch_open_file(&canonical);
        cx.notify();
        Ok(tab_id)
    }

    pub fn editor_changed(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        let current_source = self.state.tabs[index].editor.read(cx).source();
        let tab = &mut self.state.tabs[index];
        tab.dirty = dirty_for_source(&tab.saved_source, &current_source);
        tab.autosave_generation = tab.autosave_generation.saturating_add(1);
        self.schedule_autosave(tab_id, cx);
        cx.notify();
    }

    pub fn save_tab(
        &mut self,
        tab_id: TabId,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), LifecycleError> {
        let index = self
            .tab_index(tab_id)
            .ok_or(LifecycleError::MissingTab(tab_id))?;
        let path = self.state.tabs[index].path.clone();
        if path.as_os_str().is_empty() {
            return self.lifecycle_failure(LifecycleError::SaveAsRequired(tab_id));
        }
        let current_source = self.state.tabs[index].editor.read(cx).source();
        let result = save_tab_transition(
            SaveTabState::from_tab(&mut self.state.tabs[index]),
            current_source,
            write_markdown,
        );
        if let Err(error) = result {
            return self.lifecycle_failure(error);
        };
        self.autosave_tasks.remove(&tab_id);
        self.clear_tab_modal(tab_id);
        cx.notify();
        Ok(())
    }

    pub fn save_as(
        &mut self,
        tab_id: TabId,
        destination: PathBuf,
        cx: &mut gpui::Context<Self>,
    ) -> Result<PathBuf, LifecycleError> {
        let index = self
            .tab_index(tab_id)
            .ok_or(LifecycleError::MissingTab(tab_id))?;
        let current_source = self.state.tabs[index].editor.read(cx).source();
        let canonical = match save_as_transition(
            tab_id,
            SaveTabState::from_tab(&mut self.state.tabs[index]),
            &mut self.open_tab_paths,
            destination,
            current_source,
            write_markdown,
        ) {
            Ok(path) => path,
            Err(error) => return self.lifecycle_failure(error),
        };

        if let Err(error) = self.state.recent_files.record_success(&canonical) {
            self.notification = Some(Notification::error(error));
        }
        actions::set_file_menu(cx, self.state.recent_files.paths());
        self.autosave_tasks.remove(&tab_id);
        self.clear_tab_modal(tab_id);
        self.refresh_workspace_tree();
        self.refresh_watcher();
        cx.notify();
        Ok(canonical)
    }

    pub fn open_folder(
        &mut self,
        root: PathBuf,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), LifecycleError> {
        let canonical = self.file_result(canonicalize_path(&root))?;
        let mut tree = self.file_result(scan_markdown_tree(&canonical))?;
        let watcher = match self.build_watcher(Some(&canonical)) {
            Ok(watcher) => watcher,
            Err(error) => return self.lifecycle_failure(error.into()),
        };

        if self.state.workspace_root.as_ref() == Some(&canonical) {
            crate::ui::file_tree::preserve_expansion(&self.state.file_tree, &mut tree);
        }
        apply_workspace_state(
            &mut self.state.workspace_root,
            &mut self.state.file_tree,
            canonical,
            tree,
        );
        self.watcher = Some(watcher);
        cx.notify();
        Ok(())
    }

    pub fn switch_tab(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) -> bool {
        if self.tab_index(tab_id).is_none() {
            return false;
        }
        if self.state.active_tab == Some(tab_id) {
            return true;
        }

        if let Some(active_id) = self.state.active_tab {
            let should_save = self.state.auto_save_enabled
                && self
                    .tab_index(active_id)
                    .is_some_and(|index| self.state.tabs[index].dirty);
            if should_save {
                let _ = self.save_tab(active_id, cx);
            }
        }

        self.state.active_tab = Some(tab_id);
        cx.notify();
        true
    }

    pub fn close_tab(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) -> bool {
        let Some(index) = self.tab_index(tab_id) else {
            return false;
        };
        match close_action(self.state.tabs[index].dirty) {
            CloseAction::Remove => self.remove_tab(tab_id, cx),
            CloseAction::RequestDecision => {
                self.modal = Some(Modal::CloseTab(tab_id));
                cx.notify();
                false
            }
        }
    }

    pub fn discard_close_tab(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) -> bool {
        if self.modal != Some(Modal::CloseTab(tab_id)) {
            return false;
        }
        self.modal = None;
        self.remove_tab(tab_id, cx)
    }

    pub fn save_and_close_tab(
        &mut self,
        tab_id: TabId,
        cx: &mut gpui::Context<Self>,
    ) -> Result<bool, LifecycleError> {
        self.save_tab(tab_id, cx)?;
        self.modal = None;
        Ok(self.remove_tab(tab_id, cx))
    }

    pub fn save_active(&mut self, cx: &mut gpui::Context<Self>) -> Result<(), LifecycleError> {
        let Some(tab_id) = self.state.active_tab else {
            return self.lifecycle_failure(LifecycleError::NoActiveTab);
        };
        self.save_tab(tab_id, cx)
    }

    pub fn open_file_dialog(&mut self, cx: &mut gpui::Context<Self>) -> Result<(), LifecycleError> {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown"])
            .pick_file()
        else {
            return Ok(());
        };
        self.open_file(path, cx).map(|_| ())
    }

    pub fn open_folder_dialog(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), LifecycleError> {
        let Some(path) = rfd::FileDialog::new().pick_folder() else {
            return Ok(());
        };
        self.open_folder(path, cx)
    }

    pub fn save_active_as(&mut self, cx: &mut gpui::Context<Self>) -> Result<(), LifecycleError> {
        let Some(tab_id) = self.state.active_tab else {
            return self.lifecycle_failure(LifecycleError::NoActiveTab);
        };
        let Some(index) = self.tab_index(tab_id) else {
            return self.lifecycle_failure(LifecycleError::MissingTab(tab_id));
        };
        let title = self.state.tabs[index].title.clone();
        let Some(destination) = rfd::FileDialog::new().set_file_name(title).save_file() else {
            return Ok(());
        };
        self.save_as(tab_id, destination, cx).map(|_| ())
    }

    pub fn save_all(&mut self, cx: &mut gpui::Context<Self>) -> Result<(), LifecycleError> {
        let dirty_tabs = self
            .state
            .tabs
            .iter()
            .filter(|tab| tab.dirty)
            .map(|tab| tab.id)
            .collect::<Vec<_>>();
        for tab_id in dirty_tabs {
            self.save_tab(tab_id, cx)?;
        }
        Ok(())
    }

    pub fn reload_external_file(
        &mut self,
        tab_id: TabId,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), LifecycleError> {
        if self.modal != Some(Modal::ExternalConflict(tab_id)) {
            return Ok(());
        }
        let index = self
            .tab_index(tab_id)
            .ok_or(LifecycleError::MissingTab(tab_id))?;
        let path = self.state.tabs[index].path.clone();
        let version = self.file_result(disk_version(&path))?;
        if !version.exists {
            self.enter_deleted(tab_id, cx);
            return Ok(());
        }
        let source = self.file_result(read_markdown(&path))?;
        self.replace_editor_from_disk(tab_id, source, version, cx);
        self.modal = None;
        cx.notify();
        Ok(())
    }

    pub fn keep_mine(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) -> bool {
        if self.modal != Some(Modal::ExternalConflict(tab_id)) {
            return false;
        }
        let Some(index) = self.tab_index(tab_id) else {
            return false;
        };
        self.state.tabs[index].disk_state = DiskState::Conflict;
        self.modal = None;
        self.schedule_autosave(tab_id, cx);
        cx.notify();
        true
    }

    pub fn keep_deleted_file_open(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) -> bool {
        if self.modal != Some(Modal::DeletedFile(tab_id)) {
            return false;
        }
        let Some(index) = self.tab_index(tab_id) else {
            return false;
        };
        let tab = &mut self.state.tabs[index];
        keep_deleted_open(&mut tab.dirty, &mut tab.disk_state);
        self.modal = None;
        cx.notify();
        true
    }

    pub fn close_deleted_file(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) -> bool {
        if self.modal != Some(Modal::DeletedFile(tab_id)) {
            return false;
        }
        self.modal = None;
        self.remove_tab(tab_id, cx)
    }

    pub fn should_close_window(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        let has_dirty_tabs = self.state.tabs.iter().any(|tab| tab.dirty);
        match window_close_action(
            self.allow_quit,
            has_dirty_tabs,
            self.state.auto_save_enabled,
        ) {
            WindowCloseAction::Allow => true,
            WindowCloseAction::SaveAll => {
                if self.save_all(cx).is_ok() && !self.state.tabs.iter().any(|tab| tab.dirty) {
                    true
                } else {
                    self.modal = Some(Modal::Shutdown);
                    cx.notify();
                    false
                }
            }
            WindowCloseAction::RequestDecision => {
                self.modal = Some(Modal::Shutdown);
                cx.notify();
                false
            }
        }
    }

    pub fn quit_anyway(&mut self, cx: &mut gpui::Context<Self>) {
        self.allow_quit = true;
        self.modal = None;
        cx.quit();
    }

    pub fn close_active(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        self.state
            .active_tab
            .is_some_and(|tab_id| self.close_tab(tab_id, cx))
    }

    pub fn toggle_sidebar(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        self.state.sidebar_visible = !self.state.sidebar_visible;
        cx.notify();
        self.state.sidebar_visible
    }

    pub fn toggle_tree_path(&mut self, path: &Path, cx: &mut gpui::Context<Self>) -> bool {
        let changed = crate::ui::file_tree::toggle_expansion(&mut self.state.file_tree, path);
        if changed {
            cx.notify();
        }
        changed
    }

    pub fn dismiss_modal(&mut self, cx: &mut gpui::Context<Self>) {
        if self.modal.take().is_some() {
            cx.notify();
        }
    }

    pub fn active_editor_surface(
        &self,
    ) -> Option<(gpui::Entity<editor::Editor>, gpui::ScrollHandle)> {
        let tab_id = self.state.active_tab?;
        let tab = self.state.tabs.iter().find(|tab| tab.id == tab_id)?;
        let scroll = self.editor_scrolls.get(&tab_id)?.clone();
        Some((tab.editor.clone(), scroll))
    }

    pub fn next_tab(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        self.navigate_tab(TabDirection::Next, cx)
    }

    pub fn previous_tab(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        self.navigate_tab(TabDirection::Previous, cx)
    }

    pub fn refresh_tree(&mut self, cx: &mut gpui::Context<Self>) {
        self.refresh_workspace_tree();
        cx.notify();
    }

    pub fn open_recent(
        &mut self,
        index: usize,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), LifecycleError> {
        let Some(path) = self.state.recent_files.paths().get(index).cloned() else {
            return self.lifecycle_failure(LifecycleError::MissingRecentEntry(index));
        };

        match self.open_file(path.clone(), cx) {
            Ok(_) => Ok(()),
            Err(LifecycleError::File(FileError::NotFound { .. })) => {
                let cleanup_error = self
                    .state
                    .recent_files
                    .remove(&path)
                    .err()
                    .map(|error| error.to_string());
                actions::set_file_menu(cx, self.state.recent_files.paths());
                cx.notify();
                self.lifecycle_failure(LifecycleError::MissingRecentFile {
                    path,
                    cleanup_error,
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn quit(&mut self, cx: &mut gpui::Context<Self>) {
        cx.quit();
    }

    pub fn autosave_key_is_current(&self, key: &AutosaveKey) -> bool {
        self.tab_index(key.tab_id).is_some_and(|index| {
            let tab = &self.state.tabs[index];
            autosave_is_current(key, tab.id, tab.autosave_generation, &tab.path, tab.dirty)
        })
    }

    fn schedule_autosave(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) {
        let Some(index) = self.tab_index(tab_id) else {
            self.autosave_tasks.remove(&tab_id);
            return;
        };
        let tab = &self.state.tabs[index];
        if !self.state.auto_save_enabled || !tab.dirty || tab.path.as_os_str().is_empty() {
            self.autosave_tasks.remove(&tab_id);
            return;
        }

        let key = AutosaveKey {
            tab_id,
            generation: tab.autosave_generation,
            path: tab.path.clone(),
        };
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(800))
                .await;
            let _ = this.update(cx, |view, cx| view.run_autosave(key, cx));
        });
        self.autosave_tasks.insert(tab_id, task);
    }

    fn run_autosave(&mut self, key: AutosaveKey, cx: &mut gpui::Context<Self>) {
        self.autosave_tasks.remove(&key.tab_id);
        if !self.autosave_key_is_current(&key) {
            return;
        }
        let Some(index) = self.tab_index(key.tab_id) else {
            return;
        };
        let current_source = self.state.tabs[index].editor.read(cx).source();
        let tab_id = self.state.tabs[index].id;
        let generation = self.state.tabs[index].autosave_generation;
        let result = autosave_transition(
            &key,
            tab_id,
            generation,
            SaveTabState::from_tab(&mut self.state.tabs[index]),
            current_source,
            write_markdown,
        );
        match result {
            Ok(true) => self.clear_tab_modal(key.tab_id),
            Ok(false) => return,
            Err(error) => self.notification = Some(Notification::error(error)),
        }
        cx.notify();
    }

    fn clear_tab_modal(&mut self, tab_id: TabId) {
        if matches!(
            self.modal,
            Some(Modal::CloseTab(id) | Modal::ExternalConflict(id) | Modal::DeletedFile(id))
                if id == tab_id
        ) {
            self.modal = None;
        }
    }

    fn navigate_tab(&mut self, direction: TabDirection, cx: &mut gpui::Context<Self>) -> bool {
        let tabs = self.state.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
        adjacent_tab_id(&tabs, self.state.active_tab, direction)
            .is_some_and(|tab_id| self.switch_tab(tab_id, cx))
    }

    fn on_open_file(
        &mut self,
        _: &actions::OpenFile,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.open_file_dialog(cx);
    }

    fn on_open_folder(
        &mut self,
        _: &actions::OpenFolder,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.open_folder_dialog(cx);
    }

    fn on_save(&mut self, _: &actions::Save, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        let _ = self.save_active(cx);
    }

    fn on_save_as(
        &mut self,
        _: &actions::SaveAs,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.save_active_as(cx);
    }

    fn on_save_all(
        &mut self,
        _: &actions::SaveAll,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.save_all(cx);
    }

    fn on_close_tab(
        &mut self,
        _: &actions::CloseTab,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.close_active(cx);
    }

    fn on_toggle_sidebar(
        &mut self,
        _: &actions::ToggleSidebar,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.toggle_sidebar(cx);
    }

    fn on_next_tab(
        &mut self,
        _: &actions::NextTab,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.next_tab(cx);
    }

    fn on_previous_tab(
        &mut self,
        _: &actions::PreviousTab,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.previous_tab(cx);
    }

    fn on_refresh_tree(
        &mut self,
        _: &actions::RefreshTree,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.refresh_tree(cx);
    }

    fn on_quit(&mut self, _: &actions::Quit, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        self.quit(cx);
    }

    fn on_open_recent<A: gpui::Action + actions::RecentAction>(
        &mut self,
        _: &A,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.open_recent(actions::recent_position::<A>(), cx);
    }

    fn create_editor(
        tab_id: TabId,
        source: String,
        cx: &mut gpui::Context<Self>,
    ) -> (gpui::Entity<editor::Editor>, gpui::ScrollHandle) {
        let scroll = gpui::ScrollHandle::new();
        let editor = cx.new({
            let scroll = scroll.clone();
            move |cx| editor::Editor::new(&source, cx).with_scroll(scroll)
        });
        cx.observe(&editor, move |view, _, cx| view.editor_changed(tab_id, cx))
            .detach();
        (editor, scroll)
    }

    fn tab_index(&self, tab_id: TabId) -> Option<usize> {
        self.state.tabs.iter().position(|tab| tab.id == tab_id)
    }

    fn remove_tab(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) -> bool {
        let Some(index) = self.tab_index(tab_id) else {
            return false;
        };
        let removed = self.state.tabs.remove(index);
        if !removed.path.as_os_str().is_empty() {
            self.open_tab_paths.remove(&removed.path);
        }
        self.autosave_tasks.remove(&tab_id);
        self.editor_scrolls.remove(&tab_id);

        if self.state.active_tab == Some(tab_id) {
            self.state.active_tab = if self.state.tabs.is_empty() {
                None
            } else {
                Some(self.state.tabs[index.min(self.state.tabs.len() - 1)].id)
            };
        }
        self.clear_tab_modal(tab_id);
        cx.notify();
        true
    }

    fn poll_watcher(&mut self, cx: &mut gpui::Context<Self>) {
        let events = self
            .watcher
            .as_ref()
            .map(FileWatcherService::drain)
            .unwrap_or_default();
        if events.is_empty() {
            return;
        }

        let mut changed_paths = BTreeSet::new();
        let mut rescan_workspace = false;
        for event in events {
            match event {
                FileSystemEvent::Changed(path) => {
                    changed_paths.insert(path);
                }
                FileSystemEvent::Created(path) | FileSystemEvent::Removed(path) => {
                    if self
                        .state
                        .workspace_root
                        .as_ref()
                        .is_some_and(|root| path.starts_with(root))
                    {
                        rescan_workspace = true;
                    }
                    changed_paths.insert(path);
                }
                FileSystemEvent::Error { path, message } => {
                    let message = path.map_or(message.clone(), |path| {
                        format!("File watcher error for {}: {message}", path.display())
                    });
                    self.notification = Some(Notification::error(message));
                }
            }
        }

        for path in changed_paths {
            self.handle_changed_path(path, cx);
        }
        if rescan_workspace {
            self.refresh_workspace_tree();
        }
        cx.notify();
    }

    fn handle_changed_path(&mut self, path: PathBuf, cx: &mut gpui::Context<Self>) {
        let Some(tab_id) = self.tab_id_for_event_path(&path) else {
            return;
        };
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        let tab_path = self.state.tabs[index].path.clone();
        let version = match disk_version(&tab_path) {
            Ok(version) => version,
            Err(error) => {
                self.notification = Some(Notification::error(error));
                return;
            }
        };
        if self.state.tabs[index].disk_version == version {
            return;
        }
        if !version.exists {
            self.enter_deleted(tab_id, cx);
            return;
        }

        let resolution = {
            let tab = &mut self.state.tabs[index];
            load_external_change(
                ExternalChangeState {
                    saved_source: &mut tab.saved_source,
                    disk_version: &mut tab.disk_version,
                    dirty: &mut tab.dirty,
                    disk_state: &mut tab.disk_state,
                },
                &tab_path,
                version.clone(),
                read_markdown,
            )
        };
        match resolution {
            Ok(ExternalResolution::Reloaded(source)) => {
                self.replace_editor_from_disk(tab_id, source, version, cx)
            }
            Ok(ExternalResolution::Conflict) => self.enter_conflict(tab_id, cx),
            Err(error) => self.notification = Some(Notification::error(error)),
        }
    }

    fn tab_id_for_event_path(&self, path: &Path) -> Option<TabId> {
        self.open_tab_paths.get(path).or_else(|| {
            std::fs::canonicalize(path)
                .ok()
                .and_then(|canonical| self.open_tab_paths.get(&canonical))
        })
    }

    fn replace_editor_from_disk(
        &mut self,
        tab_id: TabId,
        source: String,
        version: DiskVersion,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        let (editor, scroll) = Self::create_editor(tab_id, source.clone(), cx);
        let tab = &mut self.state.tabs[index];
        tab.editor = editor;
        tab.saved_source = source;
        tab.disk_version = version;
        tab.dirty = false;
        tab.disk_state = DiskState::Synced;
        tab.autosave_generation = tab.autosave_generation.saturating_add(1);
        self.autosave_tasks.remove(&tab_id);
        self.editor_scrolls.insert(tab_id, scroll);
        cx.notify();
    }

    fn enter_conflict(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        self.state.tabs[index].disk_state = DiskState::Conflict;
        self.autosave_tasks.remove(&tab_id);
        self.modal = Some(Modal::ExternalConflict(tab_id));
        cx.notify();
    }

    fn enter_deleted(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        let tab = &mut self.state.tabs[index];
        apply_removed_event(&mut tab.dirty, &mut tab.disk_state);
        self.autosave_tasks.remove(&tab_id);
        self.modal = Some(Modal::DeletedFile(tab_id));
        cx.notify();
    }

    fn watch_open_file(&mut self, path: &Path) {
        let is_outside_workspace = self
            .state
            .workspace_root
            .as_ref()
            .is_none_or(|root| !path.starts_with(root));
        if !is_outside_workspace {
            return;
        }

        if let Some(watcher) = self.watcher.as_mut() {
            if let Err(error) = watcher.watch_file_parent(path) {
                self.notification = Some(Notification::error(error));
            }
            return;
        }

        match self.build_watcher(self.state.workspace_root.as_deref()) {
            Ok(watcher) => self.watcher = Some(watcher),
            Err(error) => self.notification = Some(Notification::error(error)),
        }
    }

    fn build_watcher(
        &self,
        workspace_root: Option<&Path>,
    ) -> Result<FileWatcherService, WatchError> {
        let mut watcher = FileWatcherService::new()?;
        if let Some(root) = workspace_root {
            watcher.watch_workspace(root)?;
        }
        for tab in &self.state.tabs {
            if tab.path.as_os_str().is_empty()
                || workspace_root.is_some_and(|root| tab.path.starts_with(root))
            {
                continue;
            }
            watcher.watch_file_parent(&tab.path)?;
        }
        Ok(watcher)
    }

    fn refresh_watcher(&mut self) {
        match self.build_watcher(self.state.workspace_root.as_deref()) {
            Ok(watcher) => self.watcher = Some(watcher),
            Err(error) => self.notification = Some(Notification::error(error)),
        }
    }

    fn refresh_workspace_tree(&mut self) {
        let Some(root) = self.state.workspace_root.clone() else {
            return;
        };
        match scan_markdown_tree(&root) {
            Ok(mut tree) => {
                crate::ui::file_tree::preserve_expansion(&self.state.file_tree, &mut tree);
                self.state.file_tree = tree;
            }
            Err(error) => self.notification = Some(Notification::error(error)),
        }
    }

    fn file_result<T>(&mut self, result: Result<T, FileError>) -> Result<T, LifecycleError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => self.lifecycle_failure(error.into()),
        }
    }

    fn lifecycle_failure<T>(&mut self, error: LifecycleError) -> Result<T, LifecycleError> {
        self.notification = Some(Notification::error(&error));
        Err(error)
    }
}

fn display_title(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl gpui::Render for Mieli {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        crate::ui::root::render(self, window, cx)
            .on_action(cx.listener(Self::on_open_file))
            .on_action(cx.listener(Self::on_open_folder))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_save_as))
            .on_action(cx.listener(Self::on_save_all))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_previous_tab))
            .on_action(cx.listener(Self::on_refresh_tree))
            .on_action(cx.listener(Self::on_quit))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent1>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent2>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent3>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent4>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent5>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent6>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent7>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent8>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent9>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent10>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent11>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent12>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent13>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent14>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent15>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent16>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent17>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent18>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent19>))
            .on_action(cx.listener(Self::on_open_recent::<actions::OpenRecent20>))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        file::{
            FileError,
            io::{canonicalize_path, disk_version, write_markdown},
        },
        state::{DiskState, DiskVersion, FileTreeNode, TabId},
    };

    use super::{
        CloseAction, ExternalChangeState, ExternalResolution, LifecycleError, OpenTabPaths,
        SaveTabState, TabDirection, WindowCloseAction, adjacent_tab_id, apply_external_change,
        apply_removed_event, apply_workspace_state, autosave_transition, close_action,
        keep_deleted_open, load_external_change, markdown_destination, save_as_transition,
        save_tab_transition, window_close_action,
    };

    #[test]
    fn tab_navigation_wraps_and_uses_the_directional_edge_without_an_active_tab() {
        let tabs = [TabId(10), TabId(20), TabId(30)];

        assert_eq!(
            adjacent_tab_id(&tabs, Some(TabId(10)), TabDirection::Next),
            Some(TabId(20))
        );
        assert_eq!(
            adjacent_tab_id(&tabs, Some(TabId(30)), TabDirection::Next),
            Some(TabId(10))
        );
        assert_eq!(
            adjacent_tab_id(&tabs, Some(TabId(10)), TabDirection::Previous),
            Some(TabId(30))
        );
        assert_eq!(
            adjacent_tab_id(&tabs, None, TabDirection::Next),
            Some(TabId(10))
        );
        assert_eq!(
            adjacent_tab_id(&tabs, None, TabDirection::Previous),
            Some(TabId(30))
        );
        assert_eq!(adjacent_tab_id(&[], None, TabDirection::Next), None);
    }

    #[test]
    fn canonical_paths_prevent_duplicate_tabs() {
        let directory = TestDirectory::new();
        let notes = directory.path().join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("README.md"), "# Notes").unwrap();

        let mut paths = OpenTabPaths::default();
        let first = paths.insert(canonicalize_path(&notes.join("README.md")).unwrap());
        let second = paths.insert(canonicalize_path(&notes.join("./README.md")).unwrap());

        assert_eq!(first, second);
        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn save_tab_preserves_complete_state_on_error_and_commits_after_success() {
        let directory = TestDirectory::new();
        let path = directory.path().join("original.md");
        fs::write(&path, "# A").unwrap();
        let mut path = canonicalize_path(&path).unwrap();
        let original_path = path.clone();
        let mut title = String::from("original.md");
        let mut saved_source = String::from("# A");
        let mut version = disk_version(&path).unwrap();
        let original_version = version.clone();
        let mut dirty = true;
        let mut disk_state = DiskState::Conflict;

        let failed = save_tab_transition(
            SaveTabState {
                path: &mut path,
                title: &mut title,
                saved_source: &mut saved_source,
                disk_version: &mut version,
                dirty: &mut dirty,
                disk_state: &mut disk_state,
            },
            String::from("# B"),
            |path, _| Err(FileError::other(path, "write")),
        );

        assert!(matches!(failed, Err(LifecycleError::File(_))));
        assert_eq!(fs::read_to_string(&original_path).unwrap(), "# A");
        assert_eq!(path, original_path);
        assert_eq!(title, "original.md");
        assert_eq!(saved_source, "# A");
        assert_eq!(version, original_version);
        assert!(dirty);
        assert_eq!(disk_state, DiskState::Conflict);

        save_tab_transition(
            SaveTabState {
                path: &mut path,
                title: &mut title,
                saved_source: &mut saved_source,
                disk_version: &mut version,
                dirty: &mut dirty,
                disk_state: &mut disk_state,
            },
            String::from("# B"),
            write_markdown,
        )
        .unwrap();

        assert_eq!(fs::read_to_string(&original_path).unwrap(), "# B");
        assert_eq!(path, original_path);
        assert_eq!(title, "original.md");
        assert_eq!(saved_source, "# B");
        assert_eq!(version, disk_version(&original_path).unwrap());
        assert!(!dirty);
        assert_eq!(disk_state, DiskState::Synced);
    }

    #[test]
    fn save_as_adds_markdown_extension_without_replacing_an_existing_extension() {
        assert_eq!(
            markdown_destination(Path::new("notes/draft")),
            PathBuf::from("notes/draft.md")
        );
        assert_eq!(
            markdown_destination(Path::new("notes/draft.txt")),
            PathBuf::from("notes/draft.txt.md")
        );
        assert_eq!(
            markdown_destination(Path::new("notes/draft.MARKDOWN")),
            PathBuf::from("notes/draft.MARKDOWN")
        );
    }

    #[test]
    fn save_as_creates_new_destination_before_committing_complete_identity() {
        let directory = TestDirectory::new();
        let original = directory.path().join("original.md");
        fs::write(&original, "# A").unwrap();
        let mut path = canonicalize_path(&original).unwrap();
        let old_path = path.clone();
        let mut title = String::from("original.md");
        let mut saved_source = String::from("# A");
        let mut version = disk_version(&path).unwrap();
        let mut dirty = true;
        let mut disk_state = DiskState::Conflict;
        let mut paths = OpenTabPaths::default();
        let tab_id = paths.insert(path.clone());
        let destination = directory.path().join("renamed");
        let expected = canonicalize_path(directory.path())
            .unwrap()
            .join("renamed.md");

        let canonical = save_as_transition(
            tab_id,
            SaveTabState {
                path: &mut path,
                title: &mut title,
                saved_source: &mut saved_source,
                disk_version: &mut version,
                dirty: &mut dirty,
                disk_state: &mut disk_state,
            },
            &mut paths,
            destination,
            String::from("# B"),
            write_markdown,
        )
        .unwrap();

        assert_eq!(canonical, expected);
        assert_eq!(fs::read_to_string(&canonical).unwrap(), "# B");
        assert_eq!(path, canonical);
        assert_eq!(title, "renamed.md");
        assert_eq!(saved_source, "# B");
        assert_eq!(version, disk_version(&canonical).unwrap());
        assert!(!dirty);
        assert_eq!(disk_state, DiskState::Synced);
        assert_eq!(paths.get(&old_path), None);
        assert_eq!(paths.get(&canonical), Some(tab_id));
    }

    #[test]
    fn failed_save_as_write_preserves_complete_old_identity_and_path_index() {
        let directory = TestDirectory::new();
        let original = directory.path().join("original.md");
        fs::write(&original, "# A").unwrap();
        let mut path = canonicalize_path(&original).unwrap();
        let old_path = path.clone();
        let mut title = String::from("original.md");
        let mut saved_source = String::from("# A");
        let mut version = disk_version(&path).unwrap();
        let old_version = version.clone();
        let mut dirty = true;
        let mut disk_state = DiskState::Conflict;
        let mut paths = OpenTabPaths::default();
        let tab_id = paths.insert(path.clone());
        let destination = directory.path().join("blocked");
        let expected_candidate = canonicalize_path(directory.path())
            .unwrap()
            .join("blocked.md");
        let mut attempted_path = None;

        let failed = save_as_transition(
            tab_id,
            SaveTabState {
                path: &mut path,
                title: &mut title,
                saved_source: &mut saved_source,
                disk_version: &mut version,
                dirty: &mut dirty,
                disk_state: &mut disk_state,
            },
            &mut paths,
            destination,
            String::from("# B"),
            |candidate, _| {
                attempted_path = Some(candidate.to_path_buf());
                Err(FileError::other(candidate, "write"))
            },
        );

        assert!(matches!(failed, Err(LifecycleError::File(_))));
        assert_eq!(attempted_path, Some(expected_candidate.clone()));
        assert!(!expected_candidate.exists());
        assert_eq!(path, old_path);
        assert_eq!(title, "original.md");
        assert_eq!(saved_source, "# A");
        assert_eq!(version, old_version);
        assert!(dirty);
        assert_eq!(disk_state, DiskState::Conflict);
        assert_eq!(paths.get(&old_path), Some(tab_id));
        assert_eq!(paths.get(&expected_candidate), None);
    }

    #[cfg(unix)]
    #[test]
    fn save_as_rejects_existing_symlink_to_open_file_before_write() {
        let directory = TestDirectory::new();
        let original = directory.path().join("original.md");
        let target = directory.path().join("open.md");
        let destination = directory.path().join("alias.md");
        fs::write(&original, "# A").unwrap();
        fs::write(&target, "# Protected").unwrap();
        std::os::unix::fs::symlink(&target, &destination).unwrap();

        let mut path = canonicalize_path(&original).unwrap();
        let old_path = path.clone();
        let canonical_target = canonicalize_path(&target).unwrap();
        let mut title = String::from("original.md");
        let mut saved_source = String::from("# A");
        let mut version = disk_version(&path).unwrap();
        let old_version = version.clone();
        let mut dirty = true;
        let mut disk_state = DiskState::Conflict;
        let mut paths = OpenTabPaths::default();
        let tab_id = paths.insert(path.clone());
        let target_tab_id = paths.insert(canonical_target.clone());

        let result = save_as_transition(
            tab_id,
            SaveTabState {
                path: &mut path,
                title: &mut title,
                saved_source: &mut saved_source,
                disk_version: &mut version,
                dirty: &mut dirty,
                disk_state: &mut disk_state,
            },
            &mut paths,
            destination,
            String::from("# B"),
            write_markdown,
        );

        assert!(matches!(
            result,
            Err(LifecycleError::PathAlreadyOpen {
                path: duplicate,
                tab_id: duplicate_tab_id,
            }) if duplicate == canonical_target && duplicate_tab_id == target_tab_id
        ));
        assert_eq!(fs::read_to_string(&old_path).unwrap(), "# A");
        assert_eq!(
            fs::read_to_string(&canonical_target).unwrap(),
            "# Protected"
        );
        assert_eq!(path, old_path);
        assert_eq!(title, "original.md");
        assert_eq!(saved_source, "# A");
        assert_eq!(version, old_version);
        assert!(dirty);
        assert_eq!(disk_state, DiskState::Conflict);
        assert_eq!(paths.get(&old_path), Some(tab_id));
        assert_eq!(paths.get(&canonical_target), Some(target_tab_id));
    }

    #[cfg(unix)]
    #[test]
    fn save_as_rejects_dangling_symlink_before_write_or_path_reassignment() {
        let directory = TestDirectory::new();
        let original = directory.path().join("original.md");
        let target = directory.path().join("open.md");
        let destination = directory.path().join("dangling.md");
        fs::write(&original, "# A").unwrap();
        fs::write(&target, "# Former target").unwrap();
        std::os::unix::fs::symlink(&target, &destination).unwrap();

        let mut path = canonicalize_path(&original).unwrap();
        let old_path = path.clone();
        let canonical_target = canonicalize_path(&target).unwrap();
        fs::remove_file(&target).unwrap();

        let mut title = String::from("original.md");
        let mut saved_source = String::from("# A");
        let mut version = disk_version(&path).unwrap();
        let old_version = version.clone();
        let mut dirty = true;
        let mut disk_state = DiskState::Conflict;
        let mut paths = OpenTabPaths::default();
        let tab_id = paths.insert(path.clone());
        let target_tab_id = paths.insert(canonical_target.clone());

        let result = save_as_transition(
            tab_id,
            SaveTabState {
                path: &mut path,
                title: &mut title,
                saved_source: &mut saved_source,
                disk_version: &mut version,
                dirty: &mut dirty,
                disk_state: &mut disk_state,
            },
            &mut paths,
            destination.clone(),
            String::from("# B"),
            write_markdown,
        );

        assert!(matches!(
            result,
            Err(LifecycleError::File(FileError::NotFound {
                path: error_path,
                operation: "canonicalize",
            })) if error_path == destination
        ));
        assert_eq!(fs::read_to_string(&old_path).unwrap(), "# A");
        assert_eq!(
            fs::symlink_metadata(&canonical_target).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        assert!(
            fs::symlink_metadata(&destination)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(path, old_path);
        assert_eq!(title, "original.md");
        assert_eq!(saved_source, "# A");
        assert_eq!(version, old_version);
        assert!(dirty);
        assert_eq!(disk_state, DiskState::Conflict);
        assert_eq!(paths.get(&old_path), Some(tab_id));
        assert_eq!(paths.get(&canonical_target), Some(target_tab_id));
    }

    #[test]
    fn dirty_close_waits_for_a_decision() {
        assert_eq!(close_action(true), CloseAction::RequestDecision);
        assert_eq!(close_action(false), CloseAction::Remove);
    }

    #[test]
    fn clean_external_change_reloads_and_dirty_external_change_conflicts() {
        let mut saved_source = String::from("# A");
        let mut version = DiskVersion::default();
        let mut dirty = false;
        let mut disk_state = DiskState::Synced;
        let disk_version = DiskVersion {
            exists: true,
            len: 3,
            digest: 11,
            ..Default::default()
        };

        let resolution = apply_external_change(
            ExternalChangeState {
                saved_source: &mut saved_source,
                disk_version: &mut version,
                dirty: &mut dirty,
                disk_state: &mut disk_state,
            },
            String::from("# B"),
            disk_version.clone(),
        );

        assert_eq!(
            resolution,
            ExternalResolution::Reloaded(String::from("# B"))
        );
        assert_eq!(saved_source, "# B");
        assert_eq!(version, disk_version);
        assert!(!dirty);
        assert_eq!(disk_state, DiskState::Synced);

        dirty = true;
        let resolution = apply_external_change(
            ExternalChangeState {
                saved_source: &mut saved_source,
                disk_version: &mut version,
                dirty: &mut dirty,
                disk_state: &mut disk_state,
            },
            String::from("# Disk"),
            DiskVersion {
                exists: true,
                len: 6,
                digest: 22,
                ..Default::default()
            },
        );

        assert_eq!(resolution, ExternalResolution::Conflict);
        assert_eq!(saved_source, "# B");
        assert_eq!(version, disk_version);
        assert!(dirty);
        assert_eq!(disk_state, DiskState::Conflict);
    }

    #[test]
    fn deleted_file_marks_dirty_and_keep_open_preserves_content() {
        let source = String::from("# A");
        let mut dirty = false;
        let mut disk_state = DiskState::Synced;

        apply_removed_event(&mut dirty, &mut disk_state);

        assert_eq!(disk_state, DiskState::Deleted);
        assert!(dirty);
        keep_deleted_open(&mut dirty, &mut disk_state);
        assert_eq!(source, "# A");
        assert!(dirty);
        assert_eq!(disk_state, DiskState::Deleted);
    }

    #[test]
    fn failed_external_read_preserves_the_complete_tab_state() {
        let path = PathBuf::from("notes.md");
        let mut saved_source = String::from("# Saved");
        let mut version = DiskVersion {
            exists: true,
            len: 7,
            digest: 11,
            ..Default::default()
        };
        let old_version = version.clone();
        let mut dirty = true;
        let mut disk_state = DiskState::Synced;

        let result = load_external_change(
            ExternalChangeState {
                saved_source: &mut saved_source,
                disk_version: &mut version,
                dirty: &mut dirty,
                disk_state: &mut disk_state,
            },
            &path,
            DiskVersion {
                exists: true,
                len: 6,
                digest: 22,
                ..Default::default()
            },
            |path| Err(FileError::other(path, "read")),
        );

        assert!(result.is_err());
        assert_eq!(saved_source, "# Saved");
        assert_eq!(version, old_version);
        assert!(dirty);
        assert_eq!(disk_state, DiskState::Synced);
    }

    #[test]
    fn window_close_saves_dirty_tabs_when_possible_and_otherwise_requests_a_decision() {
        assert_eq!(
            window_close_action(false, false, true),
            WindowCloseAction::Allow
        );
        assert_eq!(
            window_close_action(false, true, true),
            WindowCloseAction::SaveAll
        );
        assert_eq!(
            window_close_action(false, true, false),
            WindowCloseAction::RequestDecision
        );
        assert_eq!(
            window_close_action(true, true, false),
            WindowCloseAction::Allow
        );
    }

    #[test]
    fn autosave_rejects_stale_identity_and_preserves_dirty_state_on_write_failure() {
        let path = PathBuf::from("notes.md");
        let mut tab_path = path.clone();
        let mut title = String::from("notes.md");
        let mut saved_source = String::from("# Saved");
        let mut version = DiskVersion {
            exists: true,
            len: 7,
            digest: 11,
            ..Default::default()
        };
        let old_version = version.clone();
        let mut dirty = true;
        let mut disk_state = DiskState::Conflict;
        let key = crate::autosave::AutosaveKey {
            tab_id: TabId(7),
            generation: 3,
            path,
        };
        let mut writes = 0;

        let stale = autosave_transition(
            &key,
            TabId(7),
            4,
            SaveTabState {
                path: &mut tab_path,
                title: &mut title,
                saved_source: &mut saved_source,
                disk_version: &mut version,
                dirty: &mut dirty,
                disk_state: &mut disk_state,
            },
            String::from("# Local"),
            |_, _| {
                writes += 1;
                Ok(DiskVersion::default())
            },
        )
        .unwrap();

        assert!(!stale);
        assert_eq!(writes, 0);

        let error = autosave_transition(
            &key,
            TabId(7),
            3,
            SaveTabState {
                path: &mut tab_path,
                title: &mut title,
                saved_source: &mut saved_source,
                disk_version: &mut version,
                dirty: &mut dirty,
                disk_state: &mut disk_state,
            },
            String::from("# Local"),
            |path, _| Err(FileError::other(path, "write")),
        )
        .unwrap_err();

        assert!(matches!(error, LifecycleError::File(_)));
        assert_eq!(saved_source, "# Saved");
        assert_eq!(version, old_version);
        assert!(dirty);
        assert_eq!(disk_state, DiskState::Conflict);
    }

    #[test]
    fn clean_close_removes_only_the_target_path() {
        let mut paths = OpenTabPaths::default();
        let first_path = PathBuf::from("first.md");
        let second_path = PathBuf::from("second.md");
        let first = paths.insert(first_path.clone());
        let second = paths.insert(second_path.clone());

        paths.remove(&first_path);

        assert_eq!(paths.get(&first_path), None);
        assert_eq!(paths.get(&second_path), Some(second));
        assert_ne!(first, second);
        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn workspace_changes_do_not_remove_open_tabs() {
        let directory = TestDirectory::new();
        let first_path = directory.path().join("inside.md");
        let second_path = directory.path().join("outside.md");
        let mut paths = OpenTabPaths::default();
        let first = paths.insert(first_path.clone());
        let second = paths.insert(second_path.clone());
        let mut workspace_root = None;
        let mut file_tree = Vec::new();
        let expected_tree = vec![FileTreeNode {
            path: first_path.clone(),
            name: String::from("inside.md"),
            is_dir: false,
            expanded: true,
            children: Vec::new(),
        }];

        apply_workspace_state(
            &mut workspace_root,
            &mut file_tree,
            directory.path().to_path_buf(),
            expected_tree.clone(),
        );

        assert_eq!(workspace_root, Some(directory.path().to_path_buf()));
        assert_eq!(file_tree, expected_tree);
        assert_eq!(paths.get(&first_path), Some(first));
        assert_eq!(paths.get(&second_path), Some(second));
        assert_eq!(paths.len(), 2);
    }

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);

            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("mieli-task-5-{unique}-{suffix}"));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

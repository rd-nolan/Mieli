use std::{
    collections::HashMap,
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
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
        watcher::{FileWatcherService, WatchError},
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
    autosave_tasks: HashMap<TabId, gpui::Task<()>>,
    open_tab_paths: OpenTabPaths,
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
            autosave_tasks: HashMap::new(),
            open_tab_paths: OpenTabPaths::default(),
        };
        actions::set_file_menu(cx, this.state.recent_files.paths());
        this
    }

    pub fn new_tab(&mut self, cx: &mut gpui::Context<Self>) -> TabId {
        let tab_id = self.open_tab_paths.allocate();
        let source = String::new();
        let editor = Self::create_editor(tab_id, source.clone(), cx);
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
        let editor = Self::create_editor(tab_id, source.clone(), cx);
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
        let tree = self.file_result(scan_markdown_tree(&canonical))?;
        let watcher = match self.build_watcher(Some(&canonical)) {
            Ok(watcher) => watcher,
            Err(error) => return self.lifecycle_failure(error.into()),
        };

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
            let should_save = self
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
    ) -> gpui::Entity<editor::Editor> {
        let scroll = gpui::ScrollHandle::new();
        let editor = cx.new({
            let scroll = scroll.clone();
            move |cx| editor::Editor::new(&source, cx).with_scroll(scroll)
        });
        cx.observe(&editor, move |view, _, cx| view.editor_changed(tab_id, cx))
            .detach();
        editor
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

        if self.state.active_tab == Some(tab_id) {
            self.state.active_tab = if self.state.tabs.is_empty() {
                None
            } else {
                Some(self.state.tabs[index.min(self.state.tabs.len() - 1)].id)
            };
        }
        if self.modal == Some(Modal::CloseTab(tab_id)) {
            self.modal = None;
        }
        cx.notify();
        true
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
            Ok(tree) => self.state.file_tree = tree,
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
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div()
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
        state::{DiskState, FileTreeNode, TabId},
    };

    use super::{
        CloseAction, LifecycleError, OpenTabPaths, SaveTabState, TabDirection, adjacent_tab_id,
        apply_workspace_state, close_action, markdown_destination, save_as_transition,
        save_tab_transition,
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

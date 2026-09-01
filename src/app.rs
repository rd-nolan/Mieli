use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    ffi::OsString,
    fmt, fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        mpsc::{self, Receiver, TryRecvError},
    },
    time::Duration,
};

use gpui::AppContext as _;
use gpui::prelude::*;

use crate::{
    actions,
    autosave::{AutosaveKey, autosave_is_current},
    config::recent::{RecentFiles, RecentFilesError},
    file::{
        FileError,
        io::{
            canonicalize_path, disk_version, is_markdown_file, read_markdown,
            validate_markdown_path, write_markdown,
        },
        scanner::scan_markdown_tree_progressive,
        watcher::{FileSystemEvent, FileWatcherService, WatchError},
    },
    i18n::{Language, LocalizedMessage, TextKey},
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

fn workspace_root_for_path(path: &Path, current_workspace: Option<&Path>) -> Option<PathBuf> {
    if path.is_dir() {
        Some(path.to_path_buf())
    } else if current_workspace.is_some_and(|root| path.starts_with(root)) {
        None
    } else {
        path.parent().map(Path::to_path_buf)
    }
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
    if let Some(existing) = open_tab_paths.get(&candidate)
        && existing != tab_id
    {
        return Err(LifecycleError::PathAlreadyOpen {
            path: candidate,
            tab_id: existing,
        });
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
    autosave_blocked: &'a mut bool,
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
        *state.autosave_blocked = true;
        return ExternalResolution::Conflict;
    }

    *state.saved_source = disk_source.clone();
    *state.disk_version = version;
    *state.dirty = false;
    *state.disk_state = DiskState::Synced;
    *state.autosave_blocked = false;
    ExternalResolution::Reloaded(disk_source)
}

fn load_external_change(
    state: ExternalChangeState<'_>,
    path: &Path,
    version: DiskVersion,
    reader: impl FnOnce(&Path) -> Result<String, FileError>,
) -> Result<ExternalResolution, FileError> {
    let disk_source = match reader(path) {
        Ok(source) => source,
        Err(error) => {
            *state.disk_state = DiskState::Conflict;
            *state.autosave_blocked = true;
            return Err(error);
        }
    };
    Ok(apply_external_change(state, disk_source, version))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConflictDecision {
    KeepMine,
}

fn apply_conflict_decision(
    disk_state: &mut DiskState,
    autosave_blocked: &mut bool,
    decision: ConflictDecision,
) {
    *disk_state = DiskState::Conflict;
    match decision {
        ConflictDecision::KeepMine => *autosave_blocked = false,
    }
}

fn autosave_is_eligible(
    autosave_enabled: bool,
    dirty: bool,
    autosave_blocked: bool,
    path_is_empty: bool,
) -> bool {
    autosave_enabled && dirty && !autosave_blocked && !path_is_empty
}

fn save_all_tab_is_writable(dirty: bool, autosave_blocked: bool) -> bool {
    dirty && !autosave_blocked
}

fn queue_modal(active: &mut Option<Modal>, pending: &mut VecDeque<Modal>, modal: Modal) -> bool {
    if *active == Some(modal) || pending.contains(&modal) {
        return false;
    }
    if active.is_none() {
        *active = Some(modal);
    } else {
        pending.push_back(modal);
    }
    true
}

fn advance_modal(active: &mut Option<Modal>, pending: &mut VecDeque<Modal>) {
    *active = pending.pop_front();
}

fn clear_shutdown_modal(
    active: &mut Option<Modal>,
    pending: &mut VecDeque<Modal>,
    has_dirty_tabs: bool,
) {
    if has_dirty_tabs {
        return;
    }

    pending.retain(|modal| *modal != Modal::Shutdown);
    if *active == Some(Modal::Shutdown) {
        advance_modal(active, pending);
    }
}

fn clear_tab_modal_state(
    active: &mut Option<Modal>,
    pending: &mut VecDeque<Modal>,
    tab_id: TabId,
    has_dirty_tabs: bool,
) {
    pending.retain(|modal| {
        !matches!(
            modal,
            Modal::CloseTab(id) | Modal::ExternalConflict(id) | Modal::DeletedFile(id)
                if *id == tab_id
        )
    });
    if matches!(
        active,
        Some(Modal::CloseTab(id) | Modal::ExternalConflict(id) | Modal::DeletedFile(id))
            if *id == tab_id
    ) {
        advance_modal(active, pending);
    }
    clear_shutdown_modal(active, pending, has_dirty_tabs);
}

fn mark_deleted(dirty: &mut bool, disk_state: &mut DiskState) {
    *dirty = true;
    *disk_state = DiskState::Deleted;
}

fn dismiss_modal_state(active: &mut Option<Modal>, pending: &mut VecDeque<Modal>) -> bool {
    if matches!(active, Some(Modal::ExternalConflict(_))) {
        return false;
    }
    advance_modal(active, pending);
    true
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowCloseAction {
    Allow,
    SaveAll,
}

fn window_close_action(allow_quit: bool, has_dirty_tabs: bool) -> WindowCloseAction {
    if allow_quit || !has_dirty_tabs {
        WindowCloseAction::Allow
    } else {
        WindowCloseAction::SaveAll
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
    UnresolvedExternalChange(TabId),
    PathAlreadyOpen {
        path: PathBuf,
        tab_id: TabId,
    },
    MissingRecentEntry(usize),
    SaveAll {
        errors: Vec<LifecycleError>,
    },
    MissingRecentFile {
        path: PathBuf,
        cleanup_error: Option<RecentFilesError>,
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
            Self::UnresolvedExternalChange(tab_id) => write!(
                f,
                "Tab {} has an unresolved external file change.",
                tab_id.0
            ),
            Self::PathAlreadyOpen { path, tab_id } => {
                write!(f, "{} is already open in tab {}.", path.display(), tab_id.0)
            }
            Self::MissingRecentEntry(index) => {
                write!(f, "Recent file entry {} is no longer available.", index + 1)
            }
            Self::SaveAll { errors } => {
                write!(
                    f,
                    "Could not save all dirty files ({} failure{}): ",
                    errors.len(),
                    if errors.len() == 1 { "" } else { "s" }
                )?;
                for (index, error) in errors.iter().enumerate() {
                    if index > 0 {
                        f.write_str("; ")?;
                    }
                    error.fmt(f)?;
                }
                Ok(())
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

impl LocalizedMessage for LifecycleError {
    fn localized_message(&self, language: Language) -> String {
        if matches!(language, Language::English) {
            return self.to_string();
        }

        match self {
            Self::File(error) => error.localized_message(language),
            Self::Watch(error) => error.localized_message(language),
            Self::NoActiveTab => "没有活动的编辑器标签页。".to_string(),
            Self::MissingTab(tab_id) => format!("找不到编辑器标签页 {}。", tab_id.0),
            Self::SaveAsRequired(tab_id) => {
                format!("标签页 {} 需要先使用“另存为”。", tab_id.0)
            }
            Self::UnresolvedExternalChange(tab_id) => {
                format!("标签页 {} 有未解决的外部文件更改。", tab_id.0)
            }
            Self::PathAlreadyOpen { path, tab_id } => {
                format!("{} 已在标签页 {} 中打开。", path.display(), tab_id.0)
            }
            Self::MissingRecentEntry(index) => {
                format!("最近打开的第 {} 项已不可用。", index + 1)
            }
            Self::SaveAll { errors } => {
                let messages = errors
                    .iter()
                    .map(|error| error.localized_message(language))
                    .collect::<Vec<_>>()
                    .join("；");
                format!(
                    "无法保存所有未保存文件（{} 个失败）：{messages}",
                    errors.len()
                )
            }
            Self::MissingRecentFile {
                path,
                cleanup_error,
            } => {
                let cleanup = cleanup_error
                    .as_ref()
                    .map(|error| {
                        format!(
                            "；最近打开列表保存失败：{}",
                            error.localized_message(language)
                        )
                    })
                    .unwrap_or_default();
                format!(
                    "无法打开最近文件 {}：找不到文件。已移除该条记录{}。",
                    path.display(),
                    cleanup
                )
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

const WORKSPACE_SCAN_BATCH_SIZE: usize = 64;
const WORKSPACE_SCAN_CHANNEL_CAPACITY: usize = 4;
const MAX_WORKSPACE_SCAN_PATHS_PER_POLL: usize = 256;

enum WorkspaceScanMessage {
    Batch {
        generation: u64,
        paths: Vec<PathBuf>,
    },
    Finished {
        generation: u64,
        result: Result<(), FileError>,
    },
}

struct WorkspaceScan {
    root: PathBuf,
    generation: u64,
    receiver: Receiver<WorkspaceScanMessage>,
    cancel: Arc<AtomicBool>,
    _task: gpui::Task<()>,
    expansion: HashMap<PathBuf, bool>,
    pending_paths: VecDeque<PathBuf>,
    finished: Option<Result<(), FileError>>,
    showing_previous_tree: bool,
}

fn workspace_scan_channel() -> (
    mpsc::SyncSender<WorkspaceScanMessage>,
    Receiver<WorkspaceScanMessage>,
) {
    mpsc::sync_channel(WORKSPACE_SCAN_CHANNEL_CAPACITY)
}

fn workspace_scan_disconnected(root: &Path) -> Result<(), FileError> {
    Err(FileError::other(root, "scan"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceRefreshMode {
    Immediate,
    Coalesce,
}

pub struct Mieli {
    language: Language,
    pub(crate) sidebar_width: gpui::Pixels,
    pub(crate) sidebar_dragging: bool,
    pub state: AppState,
    pub modal: Option<Modal>,
    pub notification: Option<Notification>,
    pending_modals: VecDeque<Modal>,
    watcher: Option<FileWatcherService>,
    _watcher_poll_task: gpui::Task<()>,
    autosave_tasks: HashMap<TabId, gpui::Task<()>>,
    editor_scrolls: HashMap<TabId, gpui::ScrollHandle>,
    open_tab_paths: OpenTabPaths,
    workspace_scan: Option<WorkspaceScan>,
    workspace_scan_generation: u64,
    workspace_refresh_pending: bool,
    workspace_scan_error: Option<FileError>,
    allow_quit: bool,
    security_scopes: Vec<(PathBuf, crate::file::dialog::SecurityScopedResource)>,
}

impl Mieli {
    pub fn new(cx: &mut gpui::Context<Self>) -> Self {
        let language = Language::current();
        let (recent_files, recent_error) = RecentFiles::load();
        let (watcher, watcher_error) = match FileWatcherService::new() {
            Ok(watcher) => (Some(watcher), None),
            Err(error) => (None, Some(error)),
        };
        let notification = watcher_error
            .map(|error| Notification::localized_error(&error, language))
            .or_else(|| recent_error.map(|error| Notification::localized_error(&error, language)));

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
            language,
            sidebar_width: gpui::px(224.0),
            sidebar_dragging: false,
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
            pending_modals: VecDeque::new(),
            watcher,
            _watcher_poll_task: watcher_poll_task,
            autosave_tasks: HashMap::new(),
            editor_scrolls: HashMap::new(),
            open_tab_paths: OpenTabPaths::default(),
            workspace_scan: None,
            workspace_scan_generation: 0,
            workspace_refresh_pending: false,
            workspace_scan_error: None,
            allow_quit: false,
            security_scopes: Vec::new(),
        };
        actions::set_file_menu(cx, this.state.recent_files.paths(), this.language);
        this
    }

    pub(crate) fn language(&self) -> Language {
        self.language
    }

    pub(crate) fn workspace_scan_loading(&self) -> bool {
        self.workspace_scan.is_some()
    }

    pub(crate) fn workspace_scan_showing_previous_tree(&self) -> bool {
        self.workspace_scan
            .as_ref()
            .is_some_and(|scan| scan.showing_previous_tree)
    }

    pub(crate) fn workspace_scan_error(&self) -> Option<&FileError> {
        self.workspace_scan_error.as_ref()
    }

    pub(crate) fn active_tab(&self) -> Option<&EditorTab> {
        let active_id = self.state.active_tab?;
        self.state.tabs.iter().find(|tab| tab.id == active_id)
    }

    pub fn toggle_language(&mut self, cx: &mut gpui::Context<Self>) {
        self.language = self.language.toggle();
        actions::set_file_menu(cx, self.state.recent_files.paths(), self.language);
        cx.notify();
    }

    pub fn new_tab(&mut self, cx: &mut gpui::Context<Self>) -> TabId {
        let tab_id = self.open_tab_paths.allocate();
        let source = String::new();
        let (editor, scroll) = Self::create_editor(tab_id, source.clone(), cx);
        let untitled = self.language.text(TextKey::Untitled);
        let title = if self.state.tabs.iter().any(|tab| tab.title == untitled) {
            format!("{untitled} {}", tab_id.0)
        } else {
            untitled.to_string()
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
            autosave_blocked: false,
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
        self.file_result(validate_markdown_path(&path))?;
        let canonical = self.file_result(canonicalize_path(&path))?;
        if let Some(tab_id) = self.open_tab_paths.get(&canonical) {
            self.state.active_tab = Some(tab_id);
            self.record_recent_open(&canonical, cx);
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
            autosave_blocked: false,
        });
        self.editor_scrolls.insert(tab_id, scroll);
        self.state.active_tab = Some(tab_id);

        self.record_recent_open(&canonical, cx);
        self.watch_open_file(&canonical);
        cx.notify();
        Ok(tab_id)
    }

    pub fn open_path(
        &mut self,
        path: PathBuf,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), LifecycleError> {
        self.open_selected_path(crate::file::dialog::SelectedPath::from_path(path), cx)
    }

    pub fn open_selected_path(
        &mut self,
        selection: crate::file::dialog::SelectedPath,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), LifecycleError> {
        let (path, security_scope) = selection.into_parts();

        let scope_path = path.clone();
        let result = self.open_path_without_security_scope(path, cx);
        if result.is_ok()
            && !self
                .security_scopes
                .iter()
                .any(|(path, _)| path == &scope_path)
        {
            self.security_scopes.push((scope_path, security_scope));
        }
        result
    }

    pub fn open_selected_path_with_permission_fallback(
        &mut self,
        selection: crate::file::dialog::SelectedPath,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), LifecycleError> {
        let result = self.open_selected_path(selection, cx);
        if matches!(
            &result,
            Err(LifecycleError::File(FileError::PermissionDenied { .. }))
        ) {
            let _ = self.open_path_dialog(cx);
        }
        result
    }

    fn open_path_without_security_scope(
        &mut self,
        path: PathBuf,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), LifecycleError> {
        let canonical = self.file_result(canonicalize_path(&path))?;
        if canonical.is_dir() {
            return self.open_folder(canonical, cx);
        }

        if let Some(parent) =
            workspace_root_for_path(&canonical, self.state.workspace_root.as_deref())
        {
            self.open_folder(parent, cx)?;
        }
        self.open_file(canonical, cx).map(|_| ())
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
        if self.state.tabs[index].autosave_blocked {
            return self.lifecycle_failure(LifecycleError::UnresolvedExternalChange(tab_id));
        }
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
            self.notification = Some(Notification::localized_error(&error, self.language));
        }
        actions::set_file_menu(cx, self.state.recent_files.paths(), self.language);
        self.state.tabs[index].autosave_blocked = false;
        self.autosave_tasks.remove(&tab_id);
        self.clear_tab_modal(tab_id);
        self.refresh_workspace_tree(cx, WorkspaceRefreshMode::Immediate);
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
        self.file_result(
            fs::read_dir(&canonical)
                .map(|_| ())
                .map_err(|error| FileError::from_io(&canonical, "scan", error)),
        )?;
        let watcher = match self.build_watcher(Some(&canonical)) {
            Ok(watcher) => watcher,
            Err(error) => return self.lifecycle_failure(error.into()),
        };

        let expansion = if self.state.workspace_root.as_ref() == Some(&canonical) {
            crate::ui::file_tree::capture_expansion(&self.state.file_tree)
        } else {
            HashMap::new()
        };
        self.cancel_workspace_scan();
        self.workspace_refresh_pending = false;
        let previous_tree = std::mem::take(&mut self.state.file_tree);
        apply_workspace_state(
            &mut self.state.workspace_root,
            &mut self.state.file_tree,
            canonical.clone(),
            previous_tree,
        );
        self.state.sidebar_visible = true;
        self.workspace_scan_error = None;
        self.watcher = Some(watcher);
        self.prune_security_scopes();
        self.start_workspace_scan(canonical, expansion, cx);
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
                if queue_modal(
                    &mut self.modal,
                    &mut self.pending_modals,
                    Modal::CloseTab(tab_id),
                ) {
                    cx.notify();
                }
                false
            }
        }
    }

    pub fn discard_close_tab(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) -> bool {
        if self.modal != Some(Modal::CloseTab(tab_id)) {
            return false;
        }
        self.remove_tab(tab_id, cx)
    }

    pub fn save_and_close_tab(
        &mut self,
        tab_id: TabId,
        cx: &mut gpui::Context<Self>,
    ) -> Result<bool, LifecycleError> {
        self.save_tab(tab_id, cx)?;
        Ok(self.remove_tab(tab_id, cx))
    }

    pub fn save_active(&mut self, cx: &mut gpui::Context<Self>) -> Result<(), LifecycleError> {
        let Some(tab_id) = self.state.active_tab else {
            return self.lifecycle_failure(LifecycleError::NoActiveTab);
        };
        self.save_tab(tab_id, cx)
    }

    pub fn open_path_dialog(&mut self, cx: &mut gpui::Context<Self>) -> Result<(), LifecycleError> {
        #[cfg(target_os = "macos")]
        {
            use std::cell::RefCell;
            use std::rc::Rc;

            let view = cx.entity().downgrade();
            let async_cx = Rc::new(RefCell::new(cx.to_async()));
            crate::file::dialog::begin_pick_path(move |selection| {
                let Some(selection) = selection else {
                    return;
                };
                let mut async_cx = async_cx.borrow_mut();
                let _ = view.update(&mut *async_cx, |view, cx| {
                    view.open_selected_path(selection, cx)
                });
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let language = self.language;
            cx.spawn(async move |this, cx| {
                let Some(selection) = crate::file::dialog::pick_path(language) else {
                    return;
                };
                let _ = this.update(cx, |view, cx| view.open_selected_path(selection, cx));
            })
            .detach();
        }

        Ok(())
    }

    pub fn save_active_as(&mut self, cx: &mut gpui::Context<Self>) -> Result<(), LifecycleError> {
        let Some(tab_id) = self.state.active_tab else {
            return self.lifecycle_failure(LifecycleError::NoActiveTab);
        };
        let Some(index) = self.tab_index(tab_id) else {
            return self.lifecycle_failure(LifecycleError::MissingTab(tab_id));
        };
        let title = self.state.tabs[index].title.clone();
        cx.spawn(async move |this, cx| {
            let Some(destination) = rfd::FileDialog::new()
                .add_filter("Markdown", crate::file::dialog::MARKDOWN_FILE_EXTENSIONS)
                .set_file_name(title)
                .save_file()
            else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                view.save_as(tab_id, destination, cx).map(|_| ())
            });
        })
        .detach();
        Ok(())
    }

    pub fn save_all(&mut self, cx: &mut gpui::Context<Self>) -> Result<(), LifecycleError> {
        let dirty_tabs = self
            .state
            .tabs
            .iter()
            .filter(|tab| tab.dirty)
            .map(|tab| tab.id)
            .collect::<Vec<_>>();
        let mut errors = Vec::new();
        for tab_id in dirty_tabs {
            let writable = self.tab_index(tab_id).is_some_and(|index| {
                let tab = &self.state.tabs[index];
                save_all_tab_is_writable(tab.dirty, tab.autosave_blocked)
            });
            let result = if writable {
                self.save_tab(tab_id, cx)
            } else {
                Err(LifecycleError::UnresolvedExternalChange(tab_id))
            };
            if let Err(error) = result {
                errors.push(error);
            }
        }
        if !errors.is_empty() {
            return self.lifecycle_failure(LifecycleError::SaveAll { errors });
        }
        let has_dirty_tabs = self.has_dirty_tabs();
        clear_shutdown_modal(&mut self.modal, &mut self.pending_modals, has_dirty_tabs);
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
        self.clear_tab_modal(tab_id);
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
        let tab = &mut self.state.tabs[index];
        apply_conflict_decision(
            &mut tab.disk_state,
            &mut tab.autosave_blocked,
            ConflictDecision::KeepMine,
        );
        self.clear_tab_modal(tab_id);
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
        mark_deleted(&mut tab.dirty, &mut tab.disk_state);
        tab.autosave_blocked = false;
        self.clear_tab_modal(tab_id);
        cx.notify();
        true
    }

    pub fn close_deleted_file(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) -> bool {
        if self.modal != Some(Modal::DeletedFile(tab_id)) {
            return false;
        }
        self.remove_tab(tab_id, cx)
    }

    pub fn should_close_window(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        let has_dirty_tabs = self.state.tabs.iter().any(|tab| tab.dirty);
        match window_close_action(self.allow_quit, has_dirty_tabs) {
            WindowCloseAction::Allow => true,
            WindowCloseAction::SaveAll => {
                if self.save_all(cx).is_ok() && !self.state.tabs.iter().any(|tab| tab.dirty) {
                    true
                } else {
                    if queue_modal(&mut self.modal, &mut self.pending_modals, Modal::Shutdown) {
                        cx.notify();
                    }
                    false
                }
            }
        }
    }

    pub fn quit_anyway(&mut self, cx: &mut gpui::Context<Self>) {
        self.allow_quit = true;
        self.modal = None;
        self.pending_modals.clear();
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

    pub fn dismiss_notification(&mut self, cx: &mut gpui::Context<Self>) {
        if self.notification.take().is_some() {
            cx.notify();
        }
    }

    pub fn toggle_tree_path(&mut self, path: &Path, cx: &mut gpui::Context<Self>) -> bool {
        let changed = crate::ui::file_tree::toggle_expansion(&mut self.state.file_tree, path);
        if changed {
            cx.notify();
        }
        changed
    }

    pub fn dismiss_modal(&mut self, cx: &mut gpui::Context<Self>) {
        if self.modal.is_none() {
            return;
        }
        if !dismiss_modal_state(&mut self.modal, &mut self.pending_modals) {
            return;
        }
        cx.notify();
    }

    pub fn active_editor_surface(
        &self,
    ) -> Option<(gpui::Entity<editor::Editor>, gpui::ScrollHandle)> {
        let tab = self.active_tab()?;
        let tab_id = tab.id;
        let scroll = self.editor_scrolls.get(&tab_id)?.clone();
        Some((tab.editor.clone(), scroll))
    }

    pub fn active_file_path(&self) -> Option<PathBuf> {
        let tab = self.active_tab()?;
        (!tab.path.as_os_str().is_empty()).then(|| tab.path.clone())
    }

    pub fn copy_active_path(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        let Some(path) = self.active_file_path() else {
            return false;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.display().to_string()));
        let notification = Notification::success(self.language.text(TextKey::PathCopied));
        self.notification = Some(notification.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.notification.as_ref() == Some(&notification) {
                    view.notification = None;
                    cx.notify();
                }
            });
        })
        .detach();
        true
    }

    pub fn next_tab(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        self.navigate_tab(TabDirection::Next, cx)
    }

    pub fn previous_tab(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        self.navigate_tab(TabDirection::Previous, cx)
    }

    pub fn refresh_tree(&mut self, cx: &mut gpui::Context<Self>) {
        self.refresh_workspace_tree(cx, WorkspaceRefreshMode::Immediate);
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
                let cleanup_error = self.state.recent_files.remove(&path).err();
                actions::set_file_menu(cx, self.state.recent_files.paths(), self.language);
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
        if self.should_close_window(cx) {
            cx.quit();
        }
    }

    pub fn autosave_key_is_current(&self, key: &AutosaveKey) -> bool {
        self.tab_index(key.tab_id).is_some_and(|index| {
            let tab = &self.state.tabs[index];
            autosave_is_eligible(
                self.state.auto_save_enabled,
                tab.dirty,
                tab.autosave_blocked,
                tab.path.as_os_str().is_empty(),
            ) && autosave_is_current(key, tab.id, tab.autosave_generation, &tab.path, tab.dirty)
        })
    }

    fn schedule_autosave(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) {
        let Some(index) = self.tab_index(tab_id) else {
            self.autosave_tasks.remove(&tab_id);
            return;
        };
        let tab = &self.state.tabs[index];
        if !autosave_is_eligible(
            self.state.auto_save_enabled,
            tab.dirty,
            tab.autosave_blocked,
            tab.path.as_os_str().is_empty(),
        ) {
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
            Err(error) => {
                self.notification = Some(Notification::localized_error(&error, self.language))
            }
        }
        cx.notify();
    }

    fn clear_tab_modal(&mut self, tab_id: TabId) {
        let has_dirty_tabs = self.has_dirty_tabs();
        clear_tab_modal_state(
            &mut self.modal,
            &mut self.pending_modals,
            tab_id,
            has_dirty_tabs,
        );
    }

    fn has_dirty_tabs(&self) -> bool {
        self.state.tabs.iter().any(|tab| tab.dirty)
    }

    fn navigate_tab(&mut self, direction: TabDirection, cx: &mut gpui::Context<Self>) -> bool {
        let tabs = self.state.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
        adjacent_tab_id(&tabs, self.state.active_tab, direction)
            .is_some_and(|tab_id| self.switch_tab(tab_id, cx))
    }

    fn on_open_path(
        &mut self,
        _: &actions::OpenPath,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.open_path_dialog(cx);
    }

    fn on_new_file(
        &mut self,
        _: &actions::NewFile,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.new_tab(cx);
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

    fn on_external_paths_drop(
        &mut self,
        paths: &gpui::ExternalPaths,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        for path in paths.paths() {
            let result = self.open_selected_path_with_permission_fallback(
                crate::file::dialog::SelectedPath::from_path(path.clone()),
                cx,
            );
            if matches!(
                &result,
                Err(LifecycleError::File(FileError::PermissionDenied { .. }))
            ) {
                break;
            }
        }
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
        self.prune_security_scopes();
        cx.notify();
        true
    }

    fn poll_watcher(&mut self, cx: &mut gpui::Context<Self>) {
        let mut should_notify = self.poll_workspace_scan(cx);
        let events = self
            .watcher
            .as_ref()
            .map(FileWatcherService::drain)
            .unwrap_or_default();
        if events.is_empty() {
            if should_notify {
                cx.notify();
            }
            return;
        }

        let mut changed_paths = BTreeSet::new();
        let mut rescan_workspace = false;
        for event in events {
            let rescan_for_event = workspace_event_requires_rescan(
                self.state.workspace_root.as_deref(),
                &self.state.file_tree,
                &event,
            );
            match event {
                FileSystemEvent::Error { path, message } => {
                    let path = path.map(|path| path.display().to_string());
                    let message = self.language.file_watcher_error(path.as_deref(), &message);
                    self.notification = Some(Notification::error(message));
                }
                event => {
                    if rescan_for_event {
                        rescan_workspace = true;
                    }
                    if let Some(path) = event.path() {
                        changed_paths.insert(path.to_path_buf());
                    }
                }
            }
        }

        for path in changed_paths {
            self.handle_changed_path(path, cx);
        }
        if rescan_workspace {
            self.refresh_workspace_tree(cx, WorkspaceRefreshMode::Coalesce);
        }
        should_notify = true;
        if should_notify {
            cx.notify();
        }
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
                self.notification = Some(Notification::localized_error(&error, self.language));
                self.enter_conflict(tab_id, cx);
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
                    autosave_blocked: &mut tab.autosave_blocked,
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
            Err(error) => {
                self.notification = Some(Notification::localized_error(&error, self.language));
                self.enter_conflict(tab_id, cx);
            }
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
        tab.autosave_blocked = false;
        tab.autosave_generation = tab.autosave_generation.saturating_add(1);
        self.autosave_tasks.remove(&tab_id);
        self.editor_scrolls.insert(tab_id, scroll);
        cx.notify();
    }

    fn enter_conflict(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        let tab = &mut self.state.tabs[index];
        tab.disk_state = DiskState::Conflict;
        tab.autosave_blocked = true;
        tab.autosave_generation = tab.autosave_generation.saturating_add(1);
        self.autosave_tasks.remove(&tab_id);
        if queue_modal(
            &mut self.modal,
            &mut self.pending_modals,
            Modal::ExternalConflict(tab_id),
        ) {
            cx.notify();
        }
    }

    fn enter_deleted(&mut self, tab_id: TabId, cx: &mut gpui::Context<Self>) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        let tab = &mut self.state.tabs[index];
        mark_deleted(&mut tab.dirty, &mut tab.disk_state);
        tab.autosave_blocked = true;
        tab.autosave_generation = tab.autosave_generation.saturating_add(1);
        self.autosave_tasks.remove(&tab_id);
        if queue_modal(
            &mut self.modal,
            &mut self.pending_modals,
            Modal::DeletedFile(tab_id),
        ) {
            cx.notify();
        }
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
                self.notification = Some(Notification::localized_error(&error, self.language));
            }
            return;
        }

        match self.build_watcher(self.state.workspace_root.as_deref()) {
            Ok(watcher) => self.watcher = Some(watcher),
            Err(error) => {
                self.notification = Some(Notification::localized_error(&error, self.language))
            }
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
            Err(error) => {
                self.notification = Some(Notification::localized_error(&error, self.language))
            }
        }
    }

    fn prune_security_scopes(&mut self) {
        let workspace_root = self.state.workspace_root.clone();
        let tab_paths = self
            .state
            .tabs
            .iter()
            .filter(|tab| !tab.path.as_os_str().is_empty())
            .map(|tab| tab.path.clone())
            .collect::<Vec<_>>();

        self.security_scopes.retain(|(scope_path, _)| {
            workspace_root
                .as_ref()
                .is_some_and(|root| paths_overlap(root, scope_path))
                || tab_paths.iter().any(|path| paths_overlap(path, scope_path))
        });
    }

    fn start_workspace_scan(
        &mut self,
        root: PathBuf,
        expansion: HashMap<PathBuf, bool>,
        cx: &mut gpui::Context<Self>,
    ) {
        self.cancel_workspace_scan();
        self.workspace_scan_generation = self.workspace_scan_generation.saturating_add(1);
        let generation = self.workspace_scan_generation;
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (sender, receiver) = workspace_scan_channel();
        let worker_root = root.clone();
        let task = cx.spawn(async move |_this, cx| {
            cx.background_executor()
                .spawn(async move {
                    let mut batch = Vec::with_capacity(WORKSPACE_SCAN_BATCH_SIZE);
                    let mut send_failed = false;
                    let result =
                        scan_markdown_tree_progressive(&worker_root, &worker_cancel, &mut |path| {
                            if send_failed {
                                return;
                            }
                            batch.push(path);
                            if batch.len() == WORKSPACE_SCAN_BATCH_SIZE {
                                let paths = std::mem::take(&mut batch);
                                if sender
                                    .send(WorkspaceScanMessage::Batch { generation, paths })
                                    .is_err()
                                {
                                    worker_cancel.store(true, AtomicOrdering::Relaxed);
                                    send_failed = true;
                                }
                            }
                        });
                    if worker_cancel.load(AtomicOrdering::Relaxed) || send_failed {
                        return;
                    }
                    if !batch.is_empty()
                        && sender
                            .send(WorkspaceScanMessage::Batch {
                                generation,
                                paths: batch,
                            })
                            .is_err()
                    {
                        return;
                    }
                    let _ = sender.send(WorkspaceScanMessage::Finished { generation, result });
                })
                .await;
        });
        self.workspace_scan = Some(WorkspaceScan {
            root,
            generation,
            receiver,
            cancel,
            _task: task,
            expansion,
            pending_paths: VecDeque::new(),
            finished: None,
            showing_previous_tree: true,
        });
    }

    fn cancel_workspace_scan(&mut self) {
        if let Some(scan) = self.workspace_scan.take() {
            scan.cancel.store(true, AtomicOrdering::Relaxed);
        }
    }

    fn refresh_workspace_tree(&mut self, cx: &mut gpui::Context<Self>, mode: WorkspaceRefreshMode) {
        let Some(root) = self.state.workspace_root.clone() else {
            return;
        };
        if matches!(mode, WorkspaceRefreshMode::Coalesce) && self.workspace_scan.is_some() {
            self.workspace_refresh_pending = true;
            return;
        }
        self.workspace_refresh_pending = false;
        self.workspace_scan_error = None;
        let expansion = crate::ui::file_tree::capture_expansion(&self.state.file_tree);
        self.start_workspace_scan(root, expansion, cx);
    }

    fn poll_workspace_scan(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        let Some(generation) = self.workspace_scan.as_ref().map(|scan| scan.generation) else {
            return false;
        };

        let mut receiver_disconnected = false;
        if let Some(scan) = self.workspace_scan.as_mut() {
            loop {
                if scan.pending_paths.len() >= MAX_WORKSPACE_SCAN_PATHS_PER_POLL {
                    break;
                }
                match scan.receiver.try_recv() {
                    Ok(WorkspaceScanMessage::Batch {
                        generation: message_generation,
                        paths,
                    }) if message_generation == generation => {
                        scan.pending_paths.extend(paths);
                    }
                    Ok(WorkspaceScanMessage::Finished {
                        generation: message_generation,
                        result,
                    }) if message_generation == generation => {
                        scan.finished = Some(result);
                    }
                    Ok(_) => {}
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        receiver_disconnected = true;
                        break;
                    }
                }
            }
        }

        if let Some(scan) = self.workspace_scan.as_mut()
            && receiver_disconnected
            && scan.finished.is_none()
        {
            scan.finished = Some(workspace_scan_disconnected(&scan.root));
        }

        let mut changed = false;
        let mut completed_root = None;
        let mut completed_result = None;
        let mut completed = false;
        if let Some(scan) = self.workspace_scan.as_mut() {
            if !scan.pending_paths.is_empty() {
                let mut paths = Vec::with_capacity(MAX_WORKSPACE_SCAN_PATHS_PER_POLL);
                while paths.len() < MAX_WORKSPACE_SCAN_PATHS_PER_POLL {
                    let Some(path) = scan.pending_paths.pop_front() else {
                        break;
                    };
                    paths.push(path);
                }
                if scan.showing_previous_tree {
                    self.state.file_tree.clear();
                    scan.showing_previous_tree = false;
                }
                crate::ui::file_tree::insert_markdown_paths(
                    &mut self.state.file_tree,
                    &scan.root,
                    paths,
                    &scan.expansion,
                );
                changed = true;
            }

            if scan.finished.is_some() && scan.pending_paths.is_empty() {
                if scan.showing_previous_tree {
                    self.state.file_tree.clear();
                    scan.showing_previous_tree = false;
                    changed = true;
                }
                completed_root = Some(scan.root.clone());
                completed_result = scan.finished.take();
                completed = true;
            }
        }

        if !completed {
            return changed;
        }

        self.workspace_scan = None;
        if let Some(result) = completed_result {
            match result {
                Ok(()) => self.workspace_scan_error = None,
                Err(error) => {
                    self.workspace_scan_error = Some(error.clone());
                    self.notification = Some(Notification::localized_error(&error, self.language));
                }
            }
            changed = true;
        }

        if self.workspace_refresh_pending {
            self.workspace_refresh_pending = false;
            if let Some(root) = completed_root {
                let expansion = crate::ui::file_tree::capture_expansion(&self.state.file_tree);
                self.start_workspace_scan(root, expansion, cx);
                changed = true;
            }
        }
        changed
    }

    fn file_result<T>(&mut self, result: Result<T, FileError>) -> Result<T, LifecycleError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => self.lifecycle_failure(error.into()),
        }
    }

    fn record_recent_open(&mut self, path: &Path, cx: &mut gpui::Context<Self>) {
        if let Err(error) = self.state.recent_files.record_success(path) {
            self.notification = Some(Notification::localized_error(&error, self.language));
        }
        actions::set_file_menu(cx, self.state.recent_files.paths(), self.language);
    }

    fn lifecycle_failure<T>(&mut self, error: LifecycleError) -> Result<T, LifecycleError> {
        self.notification = Some(Notification::localized_error(&error, self.language));
        Err(error)
    }
}

pub fn handle_global_quit(view: Option<gpui::WeakEntity<Mieli>>, cx: &mut gpui::App) {
    if let Some(view) = view {
        if view.update(cx, |view, cx| view.quit(cx)).is_err() {
            cx.quit();
        }
    } else {
        cx.quit();
    }
}

fn display_title(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn workspace_event_requires_rescan(
    workspace_root: Option<&Path>,
    file_tree: &[FileTreeNode],
    event: &FileSystemEvent,
) -> bool {
    let Some((path, created)) = event.workspace_mutation() else {
        return false;
    };
    let Some(root) = workspace_root else {
        return false;
    };
    if !path.starts_with(root) {
        return false;
    }
    if is_markdown_file(path) {
        return true;
    }

    !created && file_tree_contains_path(file_tree, path)
}

fn file_tree_contains_path(nodes: &[FileTreeNode], path: &Path) -> bool {
    let mut pending = nodes.iter().collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        if node.path == path || node.path.starts_with(path) {
            return true;
        }
        if node.is_dir {
            pending.extend(node.children.iter());
        }
    }
    false
}

impl gpui::Render for Mieli {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        crate::ui::root::render(self, window, cx)
            .on_action(cx.listener(Self::on_new_file))
            .on_action(cx.listener(Self::on_open_path))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_save_as))
            .on_action(cx.listener(Self::on_save_all))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_previous_tab))
            .on_action(cx.listener(Self::on_refresh_tree))
            .on_drop(cx.listener(Self::on_external_paths_drop))
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
        collections::VecDeque,
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        file::{
            FileError,
            io::{canonicalize_path, disk_version, write_markdown},
            watcher::FileSystemEvent,
        },
        state::{DiskState, DiskVersion, FileTreeNode, Modal, TabId},
    };

    use super::{
        CloseAction, ConflictDecision, ExternalChangeState, ExternalResolution, LifecycleError,
        OpenTabPaths, SaveTabState, TabDirection, WORKSPACE_SCAN_CHANNEL_CAPACITY,
        WindowCloseAction, WorkspaceScanMessage, adjacent_tab_id, advance_modal,
        apply_conflict_decision, apply_external_change, apply_workspace_state,
        autosave_is_eligible, autosave_transition, clear_shutdown_modal, clear_tab_modal_state,
        close_action, dismiss_modal_state, load_external_change, mark_deleted,
        markdown_destination, queue_modal, save_all_tab_is_writable, save_as_transition,
        save_tab_transition, window_close_action, workspace_event_requires_rescan,
        workspace_root_for_path, workspace_scan_channel, workspace_scan_disconnected,
    };

    #[test]
    fn root_render_accepts_external_file_drops() {
        let source = include_str!("app.rs");

        assert!(source.contains("ExternalPaths"));
        assert!(source.contains("on_external_paths_drop"));
        assert!(source.contains("SelectedPath::from_path"));
        assert!(source.contains("PermissionDenied"));
        assert!(source.contains("open_selected_path_with_permission_fallback"));
    }

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
    fn workspace_root_for_path_keeps_directories_and_uses_file_parents() {
        let directory = TestDirectory::new();
        let file = directory.path().join("README.md");
        fs::write(&file, "# Mieli").unwrap();

        let canonical_directory = canonicalize_path(directory.path()).unwrap();
        let canonical_file = canonicalize_path(&file).unwrap();

        assert_eq!(
            workspace_root_for_path(&canonical_directory, None),
            Some(canonical_directory)
        );
        assert_eq!(
            workspace_root_for_path(&canonical_file, None),
            canonical_file.parent().map(Path::to_path_buf)
        );
    }

    #[test]
    fn nested_file_in_current_workspace_does_not_replace_workspace_root() {
        let root = Path::new("/workspace");
        let nested_file = root.join("docs/README.md");
        let outside_file = Path::new("/other/README.md");

        assert_eq!(workspace_root_for_path(&nested_file, Some(root)), None);
        assert_eq!(
            workspace_root_for_path(outside_file, Some(root)),
            Some(PathBuf::from("/other"))
        );
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
        let mut autosave_blocked = false;
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
                autosave_blocked: &mut autosave_blocked,
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
        assert!(!autosave_blocked);

        dirty = true;
        let resolution = apply_external_change(
            ExternalChangeState {
                saved_source: &mut saved_source,
                disk_version: &mut version,
                dirty: &mut dirty,
                disk_state: &mut disk_state,
                autosave_blocked: &mut autosave_blocked,
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
        assert!(autosave_blocked);
    }

    #[test]
    fn deleted_file_marks_dirty_and_keep_open_preserves_content() {
        let source = String::from("# A");
        let mut dirty = false;
        let mut disk_state = DiskState::Synced;

        mark_deleted(&mut dirty, &mut disk_state);

        assert_eq!(disk_state, DiskState::Deleted);
        assert!(dirty);
        mark_deleted(&mut dirty, &mut disk_state);
        assert_eq!(source, "# A");
        assert!(dirty);
        assert_eq!(disk_state, DiskState::Deleted);
    }

    #[test]
    fn failed_external_read_blocks_autosave_without_changing_saved_identity() {
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
        let mut autosave_blocked = false;

        let result = load_external_change(
            ExternalChangeState {
                saved_source: &mut saved_source,
                disk_version: &mut version,
                dirty: &mut dirty,
                disk_state: &mut disk_state,
                autosave_blocked: &mut autosave_blocked,
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
        assert_eq!(disk_state, DiskState::Conflict);
        assert!(autosave_blocked);
        assert!(!save_all_tab_is_writable(dirty, autosave_blocked));
    }

    #[test]
    fn window_close_always_attempts_save_before_showing_failed_shutdown() {
        assert_eq!(window_close_action(false, false), WindowCloseAction::Allow);
        assert_eq!(window_close_action(false, true), WindowCloseAction::SaveAll);
        assert_eq!(window_close_action(true, true), WindowCloseAction::Allow);
    }

    #[test]
    fn save_all_error_reports_every_failed_tab() {
        let error = LifecycleError::SaveAll {
            errors: vec![
                LifecycleError::File(FileError::other(Path::new("first.md"), "write")),
                LifecycleError::UnresolvedExternalChange(TabId(2)),
            ],
        };

        let message = error.to_string();

        assert!(message.contains("2 failures"));
        assert!(message.contains("Could not write first.md"));
        assert!(message.contains("Tab 2 has an unresolved external file change."));
    }

    #[test]
    fn external_conflict_stays_blocked_until_keep_mine() {
        let mut disk_state = DiskState::Conflict;
        let mut autosave_blocked = true;
        assert_eq!(disk_state, DiskState::Conflict);
        assert!(autosave_blocked);
        assert!(!autosave_is_eligible(true, true, autosave_blocked, false));
        assert!(!save_all_tab_is_writable(true, autosave_blocked));

        apply_conflict_decision(
            &mut disk_state,
            &mut autosave_blocked,
            ConflictDecision::KeepMine,
        );
        assert_eq!(disk_state, DiskState::Conflict);
        assert!(!autosave_blocked);
        assert!(autosave_is_eligible(true, true, autosave_blocked, false));
        assert!(save_all_tab_is_writable(true, autosave_blocked));
    }

    #[test]
    fn dismissing_external_conflict_keeps_the_resolution_modal_visible() {
        let mut active = Some(Modal::ExternalConflict(TabId(7)));
        let mut pending = VecDeque::from([Modal::Shutdown]);

        assert!(!dismiss_modal_state(&mut active, &mut pending));
        assert_eq!(active, Some(Modal::ExternalConflict(TabId(7))));
        assert_eq!(pending, VecDeque::from([Modal::Shutdown]));
    }

    #[test]
    fn disconnected_workspace_scan_is_reported_as_an_error() {
        let result = workspace_scan_disconnected(Path::new("/workspace"));

        assert!(matches!(
            result,
            Err(FileError::Io {
                path,
                operation: "scan",
                kind: std::io::ErrorKind::Other,
            }) if path.as_path() == Path::new("/workspace")
        ));
    }

    #[test]
    fn workspace_scan_channel_applies_backpressure() {
        let (sender, _receiver) = workspace_scan_channel();
        for _ in 0..WORKSPACE_SCAN_CHANNEL_CAPACITY {
            sender
                .try_send(WorkspaceScanMessage::Batch {
                    generation: 1,
                    paths: Vec::new(),
                })
                .unwrap();
        }

        assert!(matches!(
            sender.try_send(WorkspaceScanMessage::Batch {
                generation: 1,
                paths: Vec::new(),
            }),
            Err(std::sync::mpsc::TrySendError::Full(_))
        ));
    }

    #[test]
    fn unresolved_external_tabs_are_not_eligible_for_save_all() {
        assert!(!save_all_tab_is_writable(true, true));
        assert!(save_all_tab_is_writable(true, false));
        assert!(!save_all_tab_is_writable(false, false));
    }

    #[test]
    fn watcher_decisions_queue_without_replacing_the_active_modal() {
        let mut active = Some(Modal::CloseTab(TabId(1)));
        let mut pending = VecDeque::new();

        assert!(queue_modal(
            &mut active,
            &mut pending,
            Modal::ExternalConflict(TabId(2))
        ));
        assert!(queue_modal(
            &mut active,
            &mut pending,
            Modal::DeletedFile(TabId(3))
        ));
        assert!(!queue_modal(
            &mut active,
            &mut pending,
            Modal::ExternalConflict(TabId(2))
        ));
        assert_eq!(active, Some(Modal::CloseTab(TabId(1))));
        assert_eq!(
            pending.iter().copied().collect::<Vec<_>>(),
            vec![
                Modal::ExternalConflict(TabId(2)),
                Modal::DeletedFile(TabId(3))
            ]
        );

        advance_modal(&mut active, &mut pending);
        assert_eq!(active, Some(Modal::ExternalConflict(TabId(2))));
        advance_modal(&mut active, &mut pending);
        assert_eq!(active, Some(Modal::DeletedFile(TabId(3))));
    }

    #[test]
    fn clearing_last_dirty_conflict_drops_a_stale_shutdown_modal() {
        let mut active = Some(Modal::ExternalConflict(TabId(2)));
        let mut pending = VecDeque::from([Modal::Shutdown]);

        clear_tab_modal_state(&mut active, &mut pending, TabId(2), false);

        assert_eq!(active, None);
        assert!(pending.is_empty());
    }

    #[test]
    fn clearing_a_dirty_conflict_keeps_shutdown_when_other_dirty_tabs_remain() {
        let mut active = Some(Modal::ExternalConflict(TabId(2)));
        let mut pending = VecDeque::from([Modal::Shutdown]);

        clear_tab_modal_state(&mut active, &mut pending, TabId(2), true);

        assert_eq!(active, Some(Modal::Shutdown));
        assert!(pending.is_empty());
    }

    #[test]
    fn successful_save_all_cleanup_removes_stale_shutdown_state() {
        let mut active = Some(Modal::Shutdown);
        let mut pending = VecDeque::from([Modal::DeletedFile(TabId(4))]);

        clear_shutdown_modal(&mut active, &mut pending, false);

        assert_eq!(active, Some(Modal::DeletedFile(TabId(4))));
        assert!(pending.is_empty());

        let mut active = Some(Modal::CloseTab(TabId(1)));
        let mut pending = VecDeque::from([Modal::Shutdown]);

        clear_shutdown_modal(&mut active, &mut pending, false);

        assert_eq!(active, Some(Modal::CloseTab(TabId(1))));
        assert!(pending.is_empty());
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

    #[test]
    fn workspace_rescan_ignores_non_markdown_created_paths() {
        let root = Path::new("/workspace");
        let event = FileSystemEvent::Created(root.join("target/debug/test-binary"));

        assert!(!workspace_event_requires_rescan(Some(root), &[], &event));
    }

    #[test]
    fn workspace_rescan_includes_markdown_changes_and_known_directory_removals() {
        let root = Path::new("/workspace");
        let docs = root.join("docs");
        let tree = vec![FileTreeNode {
            path: docs.clone(),
            name: String::from("docs/"),
            is_dir: true,
            expanded: true,
            children: vec![FileTreeNode {
                path: docs.join("guide.md"),
                name: String::from("guide.md"),
                is_dir: false,
                expanded: true,
                children: Vec::new(),
            }],
        }];

        assert!(workspace_event_requires_rescan(
            Some(root),
            &tree,
            &FileSystemEvent::Created(root.join("new.md")),
        ));
        assert!(workspace_event_requires_rescan(
            Some(root),
            &tree,
            &FileSystemEvent::Removed(root.join("old.markdown")),
        ));
        assert!(workspace_event_requires_rescan(
            Some(root),
            &tree,
            &FileSystemEvent::Removed(docs),
        ));
    }

    #[test]
    fn workspace_rescan_ignores_unrelated_removed_paths_and_outside_events() {
        let root = Path::new("/workspace");

        assert!(!workspace_event_requires_rescan(
            Some(root),
            &[],
            &FileSystemEvent::Removed(root.join("target/cache")),
        ));
        assert!(!workspace_event_requires_rescan(
            Some(root),
            &[],
            &FileSystemEvent::Created(PathBuf::from("/other/new.md")),
        ));
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

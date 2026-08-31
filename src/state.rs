use std::{path::PathBuf, time::SystemTime};

use crate::{
    config::recent::RecentFiles,
    i18n::{Language, LocalizedMessage},
};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TabId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiskState {
    Synced,
    ModifiedExternally,
    Deleted,
    Conflict,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiskVersion {
    pub exists: bool,
    pub modified: Option<SystemTime>,
    pub len: u64,
    pub digest: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileTreeNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub expanded: bool,
    pub children: Vec<FileTreeNode>,
}

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
    pub autosave_blocked: bool,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Modal {
    CloseTab(TabId),
    ExternalConflict(TabId),
    DeletedFile(TabId),
    Shutdown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NotificationKind {
    Error,
    Success,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notification {
    pub message: String,
    pub kind: NotificationKind,
}

impl Notification {
    pub fn error(error: impl std::fmt::Display) -> Self {
        Self {
            message: error.to_string(),
            kind: NotificationKind::Error,
        }
    }

    pub(crate) fn localized_error(error: &impl LocalizedMessage, language: Language) -> Self {
        Self {
            message: error.localized_message(language),
            kind: NotificationKind::Error,
        }
    }

    pub fn success(message: impl std::fmt::Display) -> Self {
        Self {
            message: message.to_string(),
            kind: NotificationKind::Success,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Notification, NotificationKind};

    #[test]
    fn success_notifications_use_the_success_kind() {
        let notification = Notification::success("Path copied");

        assert_eq!(notification.kind, NotificationKind::Success);
        assert_eq!(notification.message, "Path copied");
    }
}

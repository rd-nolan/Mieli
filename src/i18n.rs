use std::env;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Language {
    English,
    Chinese,
}

pub(crate) trait LocalizedMessage {
    fn localized_message(&self, language: Language) -> String;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextKey {
    FileMenu,
    NewFile,
    Open,
    #[cfg(not(target_os = "macos"))]
    OpenFile,
    #[cfg(not(target_os = "macos"))]
    OpenFolder,
    #[cfg(not(target_os = "macos"))]
    ChooseOpenTarget,
    OpenRecent,
    RefreshFiles,
    Save,
    SaveAs,
    SaveAll,
    CloseTab,
    Quit,
    HideSidebar,
    ShowSidebar,
    Workspace,
    NoMarkdownFiles,
    WelcomeHint,
    CloseTabTooltip,
    NewDocumentTooltip,
    NoDocumentSelected,
    Untitled,
    CopyPath,
    UnsavedChanges,
    DontSave,
    Cancel,
    FileChangedOnDisk,
    ReloadFromDisk,
    KeepMyChanges,
    FileDeletedOnDisk,
    KeepOpen,
    Close,
    SaveFailed,
    QuitAnyway,
    ThisTab,
    ThisFile,
    SwitchLanguage,
}

impl Language {
    pub(crate) fn current() -> Self {
        let override_language = env::var("MIELI_LANGUAGE")
            .ok()
            .and_then(|value| Self::from_identifier(&value));
        let apple_language = env::var("AppleLanguages")
            .ok()
            .and_then(|value| Self::from_identifier(&value));
        let native_language =
            sys_locale::get_locales().find_map(|value| Self::from_identifier(&value));
        let environment_language = ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"]
            .into_iter()
            .filter_map(|key| env::var(key).ok())
            .find_map(|value| Self::from_identifier(&value));

        override_language
            .or(apple_language)
            .or(native_language)
            .or(environment_language)
            .unwrap_or(Self::English)
    }

    fn from_identifier(identifier: &str) -> Option<Self> {
        let normalized = identifier.to_ascii_lowercase();
        let tokens = normalized
            .split(|character: char| !character.is_ascii_alphanumeric())
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();

        for (index, token) in tokens.iter().enumerate() {
            match *token {
                "en" | "english" => return Some(Self::English),
                "zh" | "chinese" => {
                    let next = tokens.get(index + 1).copied();
                    if !matches!(next, Some("hant" | "traditional" | "tw" | "hk" | "mo")) {
                        return Some(Self::Chinese);
                    }
                }
                _ => {}
            }
        }

        None
    }

    pub(crate) const fn text(self, key: TextKey) -> &'static str {
        match self {
            Self::English => match key {
                TextKey::FileMenu => "File",
                TextKey::NewFile => "New File",
                TextKey::Open => "Open",
                #[cfg(not(target_os = "macos"))]
                TextKey::OpenFile => "File",
                #[cfg(not(target_os = "macos"))]
                TextKey::OpenFolder => "Folder",
                #[cfg(not(target_os = "macos"))]
                TextKey::ChooseOpenTarget => "Choose a Markdown file or folder.",
                TextKey::OpenRecent => "Open Recent",
                TextKey::RefreshFiles => "Refresh Tree",
                TextKey::Save => "Save",
                TextKey::SaveAs => "Save As",
                TextKey::SaveAll => "Save All",
                TextKey::CloseTab => "Close Tab",
                TextKey::Quit => "Quit",
                TextKey::HideSidebar => "Hide Sidebar (⌘⇧L)",
                TextKey::ShowSidebar => "Show Sidebar (⌘⇧L)",
                TextKey::Workspace => "Workspace",
                TextKey::NoMarkdownFiles => "No Markdown files yet.",
                TextKey::WelcomeHint => "Open a Markdown file or folder to start writing.",
                TextKey::CloseTabTooltip => "Close tab",
                TextKey::NewDocumentTooltip => "New Document",
                TextKey::NoDocumentSelected => "No document selected.",
                TextKey::Untitled => "Untitled",
                TextKey::CopyPath => "Copy path",
                TextKey::UnsavedChanges => "Unsaved changes",
                TextKey::DontSave => "Don't Save",
                TextKey::Cancel => "Cancel",
                TextKey::FileChangedOnDisk => "File changed on disk",
                TextKey::ReloadFromDisk => "Reload from Disk",
                TextKey::KeepMyChanges => "Keep My Changes",
                TextKey::FileDeletedOnDisk => "File deleted on disk",
                TextKey::KeepOpen => "Keep Open",
                TextKey::Close => "Close",
                TextKey::SaveFailed => "Save failed",
                TextKey::QuitAnyway => "Quit Anyway",
                TextKey::ThisTab => "this tab",
                TextKey::ThisFile => "This file",
                TextKey::SwitchLanguage => "Switch to Chinese",
            },
            Self::Chinese => match key {
                TextKey::FileMenu => "文件",
                TextKey::NewFile => "新建文件",
                TextKey::Open => "打开",
                #[cfg(not(target_os = "macos"))]
                TextKey::OpenFile => "文件",
                #[cfg(not(target_os = "macos"))]
                TextKey::OpenFolder => "文件夹",
                #[cfg(not(target_os = "macos"))]
                TextKey::ChooseOpenTarget => "选择 Markdown 文件或文件夹。",
                TextKey::OpenRecent => "最近打开",
                TextKey::RefreshFiles => "刷新文件树",
                TextKey::Save => "保存",
                TextKey::SaveAs => "另存为",
                TextKey::SaveAll => "全部保存",
                TextKey::CloseTab => "关闭标签页",
                TextKey::Quit => "退出",
                TextKey::HideSidebar => "隐藏侧边栏 (⌘⇧L)",
                TextKey::ShowSidebar => "显示侧边栏 (⌘⇧L)",
                TextKey::Workspace => "工作区",
                TextKey::NoMarkdownFiles => "暂无 Markdown 文件。",
                TextKey::WelcomeHint => "打开 Markdown 文件或文件夹，开始写作。",
                TextKey::CloseTabTooltip => "关闭标签页",
                TextKey::NewDocumentTooltip => "新建文档",
                TextKey::NoDocumentSelected => "未选择文档。",
                TextKey::Untitled => "未命名",
                TextKey::CopyPath => "复制路径",
                TextKey::UnsavedChanges => "未保存的更改",
                TextKey::DontSave => "不保存",
                TextKey::Cancel => "取消",
                TextKey::FileChangedOnDisk => "文件已被外部修改",
                TextKey::ReloadFromDisk => "从磁盘重新载入",
                TextKey::KeepMyChanges => "保留我的更改",
                TextKey::FileDeletedOnDisk => "文件已从磁盘删除",
                TextKey::KeepOpen => "保持打开",
                TextKey::Close => "关闭",
                TextKey::SaveFailed => "保存失败",
                TextKey::QuitAnyway => "仍然退出",
                TextKey::ThisTab => "该标签页",
                TextKey::ThisFile => "此文件",
                TextKey::SwitchLanguage => "切换为英文",
            },
        }
    }

    pub(crate) const fn toggle(self) -> Self {
        match self {
            Self::English => Self::Chinese,
            Self::Chinese => Self::English,
        }
    }

    pub(crate) const fn short_label(self) -> &'static str {
        match self {
            Self::English => "EN",
            Self::Chinese => "中",
        }
    }

    pub(crate) const fn sidebar_toggle(self, visible: bool) -> &'static str {
        if visible {
            self.text(TextKey::HideSidebar)
        } else {
            self.text(TextKey::ShowSidebar)
        }
    }

    pub(crate) fn save_changes_before_closing(self, title: &str) -> String {
        match self {
            Self::English => format!("Save changes to “{title}” before closing?"),
            Self::Chinese => format!("关闭前保存“{title}”的更改吗？"),
        }
    }

    pub(crate) fn external_change_message(self, title: &str) -> String {
        match self {
            Self::English => {
                format!(
                    "“{title}” was changed outside Mieli. Reload the disk version or keep your changes?"
                )
            }
            Self::Chinese => {
                format!("“{title}”已在 Mieli 外部被修改。重新载入磁盘版本，还是保留你的更改？")
            }
        }
    }

    pub(crate) fn deleted_file_message(self, title: &str) -> String {
        match self {
            Self::English => {
                format!("“{title}” was deleted outside Mieli. Keep the editor open or close it?")
            }
            Self::Chinese => {
                format!("“{title}”已在 Mieli 外部被删除。保持编辑器打开，还是关闭？")
            }
        }
    }

    pub(crate) fn file_watcher_error(self, path: Option<&str>, message: &str) -> String {
        match (self, path) {
            (Self::English, Some(path)) => format!("File watcher error for {path}: {message}"),
            (Self::English, None) => format!("File watcher error: {message}"),
            (Self::Chinese, Some(path)) => {
                format!("文件监视器错误：{path}（详情：{message}）")
            }
            (Self::Chinese, None) => format!("文件监视器错误（详情：{message}）"),
        }
    }

    pub(crate) fn operation_label(self, operation: &str) -> String {
        if matches!(self, Self::English) {
            return operation.to_string();
        }

        match operation {
            "open" => "打开".to_string(),
            "read" => "读取".to_string(),
            "write" => "写入".to_string(),
            "canonicalize" => "解析".to_string(),
            "inspect" => "检查".to_string(),
            "scan" => "扫描".to_string(),
            "create watcher" => "创建文件监视器".to_string(),
            "watch" => "监视".to_string(),
            "unwatch" => "停止监视".to_string(),
            _ => operation.to_string(),
        }
    }

    pub(crate) fn io_error_kind(self, kind: std::io::ErrorKind) -> String {
        if matches!(self, Self::English) {
            return kind.to_string();
        }

        match kind {
            std::io::ErrorKind::NotFound => "找不到文件".to_string(),
            std::io::ErrorKind::PermissionDenied => "没有权限".to_string(),
            std::io::ErrorKind::AlreadyExists => "文件已存在".to_string(),
            std::io::ErrorKind::InvalidInput => "输入无效".to_string(),
            std::io::ErrorKind::InvalidData => "数据无效".to_string(),
            _ => format!("系统错误（{kind}）"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Language, TextKey};

    #[test]
    fn language_toggle_updates_labels_and_switch_hint() {
        assert_eq!(Language::English.short_label(), "EN");
        assert_eq!(Language::English.toggle(), Language::Chinese);
        assert_eq!(
            Language::English.toggle().text(TextKey::SwitchLanguage),
            "切换为英文"
        );
        assert_eq!(Language::Chinese.toggle(), Language::English);
        assert_eq!(
            Language::Chinese.toggle().text(TextKey::SwitchLanguage),
            "Switch to Chinese"
        );
    }
}

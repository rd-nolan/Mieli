use std::{fmt, io as std_io, path::PathBuf};

use crate::i18n::{Language, LocalizedMessage};

pub mod dialog;
pub mod io;
pub mod scanner;
pub mod watcher;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileError {
    NotMarkdown {
        path: PathBuf,
    },
    InvalidUtf8 {
        path: PathBuf,
        operation: &'static str,
    },
    NotFound {
        path: PathBuf,
        operation: &'static str,
    },
    PermissionDenied {
        path: PathBuf,
        operation: &'static str,
    },
    Io {
        path: PathBuf,
        operation: &'static str,
        kind: std_io::ErrorKind,
    },
}

impl FileError {
    pub fn from_io(path: &std::path::Path, operation: &'static str, error: std_io::Error) -> Self {
        match error.kind() {
            std_io::ErrorKind::InvalidData => Self::InvalidUtf8 {
                path: path.to_path_buf(),
                operation,
            },
            std_io::ErrorKind::NotFound => Self::NotFound {
                path: path.to_path_buf(),
                operation,
            },
            std_io::ErrorKind::PermissionDenied => Self::PermissionDenied {
                path: path.to_path_buf(),
                operation,
            },
            kind => Self::Io {
                path: path.to_path_buf(),
                operation,
                kind,
            },
        }
    }

    pub fn other(path: &std::path::Path, operation: &'static str) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            operation,
            kind: std_io::ErrorKind::Other,
        }
    }
}

impl LocalizedMessage for FileError {
    fn localized_message(&self, language: Language) -> String {
        if matches!(language, Language::English) {
            return self.to_string();
        }

        match self {
            Self::NotMarkdown { path } => {
                format!(
                    "无法打开 {}：需要 Markdown 文件（.md 或 .markdown）。",
                    path.display()
                )
            }
            Self::InvalidUtf8 { path, .. } => {
                format!("文件 {} 不是有效的 UTF-8 编码。", path.display())
            }
            Self::NotFound { path, operation } => format!(
                "无法{} {}：找不到文件。",
                language.operation_label(operation),
                path.display()
            ),
            Self::PermissionDenied { path, operation } => format!(
                "无法{} {}：没有权限。",
                language.operation_label(operation),
                path.display()
            ),
            Self::Io {
                path,
                operation,
                kind,
            } => format!(
                "无法{} {}：{}。",
                language.operation_label(operation),
                path.display(),
                language.io_error_kind(*kind)
            ),
        }
    }
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotMarkdown { path } => write!(
                f,
                "Could not open {}: expected a Markdown file (.md or .markdown).",
                path.display()
            ),
            Self::InvalidUtf8 { .. } => f.write_str("The file is not valid UTF-8."),
            Self::NotFound { path, operation } => {
                write!(
                    f,
                    "Could not {operation} {}: file not found.",
                    path.display()
                )
            }
            Self::PermissionDenied { path, operation } => write!(
                f,
                "Could not {operation} {}: permission denied.",
                path.display()
            ),
            Self::Io {
                path,
                operation,
                kind,
            } => write!(f, "Could not {operation} {}: {kind}.", path.display()),
        }
    }
}

impl std::error::Error for FileError {}

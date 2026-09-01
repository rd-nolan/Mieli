use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2_foundation::{NSString, NSURL};

pub(crate) const MARKDOWN_FILE_EXTENSIONS: &[&str] = &["md", "markdown"];

/// A path selected through a macOS user gesture, together with the access
/// scope that must stay alive while the path is scanned and watched.
pub struct SelectedPath {
    pub path: PathBuf,
    security_scope: SecurityScopedResource,
}

impl SelectedPath {
    pub fn from_path(path: PathBuf) -> Self {
        let security_scope = SecurityScopedResource::from_path(&path);
        Self {
            path,
            security_scope,
        }
    }

    #[cfg(target_os = "macos")]
    fn from_url(url: Retained<NSURL>) -> Option<Self> {
        let path = url.path()?.to_string();
        Some(Self {
            path: PathBuf::from(path),
            security_scope: SecurityScopedResource::from_url(url),
        })
    }

    pub(crate) fn into_parts(self) -> (PathBuf, SecurityScopedResource) {
        (self.path, self.security_scope)
    }
}

pub struct SecurityScopedResource {
    #[cfg(target_os = "macos")]
    url: Retained<NSURL>,
    #[cfg(target_os = "macos")]
    access_started: bool,
}

impl SecurityScopedResource {
    #[cfg(target_os = "macos")]
    fn from_path(path: &Path) -> Self {
        let path = path.to_string_lossy();
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.as_ref()));
        Self::from_url(url)
    }

    #[cfg(not(target_os = "macos"))]
    fn from_path(_path: &Path) -> Self {
        Self {}
    }

    #[cfg(target_os = "macos")]
    fn from_url(url: Retained<NSURL>) -> Self {
        let access_started = unsafe { url.startAccessingSecurityScopedResource() };
        Self {
            url,
            access_started,
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for SecurityScopedResource {
    fn drop(&mut self) {
        if self.access_started {
            unsafe { self.url.stopAccessingSecurityScopedResource() };
        }
    }
}

/// Convert a file URL string into a selected path and request a security scope.
#[cfg(target_os = "macos")]
pub fn selected_path_from_file_url(value: &str) -> Option<SelectedPath> {
    let url = NSURL::URLWithString(&NSString::from_str(value))?;
    SelectedPath::from_url(url)
}

#[cfg(not(target_os = "macos"))]
pub fn selected_path_from_file_url(value: &str) -> Option<SelectedPath> {
    let url = url::Url::parse(value).ok()?;
    (url.scheme() == "file")
        .then(|| url.to_file_path().ok())
        .flatten()
        .map(SelectedPath::from_path)
}

/// Present a native open panel that accepts either one Markdown file or a
/// directory.
#[cfg(target_os = "macos")]
pub fn begin_pick_path<F>(callback: F)
where
    F: FnOnce(Option<SelectedPath>) + 'static,
{
    use std::cell::RefCell;

    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponse, NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::{NSArray, NSString};

    let Some(marker) = MainThreadMarker::new() else {
        callback(None);
        return;
    };
    let panel = NSOpenPanel::openPanel(marker);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(false);
    let allowed_types =
        NSArray::from_retained_slice(&[NSString::from_str("md"), NSString::from_str("markdown")]);
    #[allow(deprecated)]
    panel.setAllowedFileTypes(Some(&allowed_types));

    let callback = RefCell::new(Some(callback));
    let panel_for_handler = panel.clone();
    let handler = RcBlock::new(move |response: NSModalResponse| {
        let path = if response == NSModalResponseOK {
            panel_for_handler
                .URLs()
                .firstObject()
                .and_then(SelectedPath::from_url)
        } else {
            None
        };
        let callback = {
            let mut callback = callback.borrow_mut();
            callback.take()
        };
        if let Some(callback) = callback {
            callback(path);
        }
    });

    panel.beginWithCompletionHandler(&handler);
}

#[cfg(not(target_os = "macos"))]
pub fn pick_path(language: crate::i18n::Language) -> Option<SelectedPath> {
    use crate::i18n::TextKey;
    use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult};

    let file_label = language.text(TextKey::OpenFile).to_owned();
    let folder_label = language.text(TextKey::OpenFolder).to_owned();
    let choice = MessageDialog::new()
        .set_title(language.text(TextKey::Open))
        .set_description(language.text(TextKey::ChooseOpenTarget))
        .set_buttons(MessageButtons::YesNoCancelCustom(
            file_label.clone(),
            folder_label.clone(),
            language.text(TextKey::Cancel).to_owned(),
        ))
        .show();

    let path = match choice {
        MessageDialogResult::Yes => FileDialog::new()
            .add_filter("Markdown", MARKDOWN_FILE_EXTENSIONS)
            .pick_file(),
        MessageDialogResult::No => FileDialog::new().pick_folder(),
        MessageDialogResult::Custom(label) if label == file_label => FileDialog::new()
            .add_filter("Markdown", MARKDOWN_FILE_EXTENSIONS)
            .pick_file(),
        MessageDialogResult::Custom(label) if label == folder_label => {
            FileDialog::new().pick_folder()
        }
        MessageDialogResult::Ok | MessageDialogResult::Cancel | MessageDialogResult::Custom(_) => {
            None
        }
    };

    path.map(SelectedPath::from_path)
}

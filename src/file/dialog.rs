use std::path::PathBuf;

/// Present a native open panel that accepts either one Markdown file or a
/// directory.
#[cfg(target_os = "macos")]
pub fn begin_pick_path<F>(callback: F)
where
    F: FnOnce(Option<PathBuf>) + 'static,
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
                .and_then(|url| url.path())
                .map(|path| PathBuf::from(path.to_string()))
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
pub fn pick_path(language: crate::i18n::Language) -> Option<PathBuf> {
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

    match choice {
        MessageDialogResult::Yes => FileDialog::new()
            .add_filter("Markdown", &["md", "markdown"])
            .pick_file(),
        MessageDialogResult::No => FileDialog::new().pick_folder(),
        MessageDialogResult::Custom(label) if label == file_label => FileDialog::new()
            .add_filter("Markdown", &["md", "markdown"])
            .pick_file(),
        MessageDialogResult::Custom(label) if label == folder_label => {
            FileDialog::new().pick_folder()
        }
        MessageDialogResult::Ok | MessageDialogResult::Cancel | MessageDialogResult::Custom(_) => {
            None
        }
    }
}

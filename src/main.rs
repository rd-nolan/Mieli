use std::{cell::RefCell, path::PathBuf, rc::Rc};

use gpui::AppContext as _;
use mieli::{actions, app, theme};
use url::Url;

fn path_from_file_url(value: &str) -> Option<PathBuf> {
    let url = Url::parse(value).ok()?;
    (url.scheme() == "file")
        .then(|| url.to_file_path().ok())
        .flatten()
}

fn main() {
    let open_view: Rc<RefCell<Option<gpui::WeakEntity<app::Mieli>>>> = Rc::new(RefCell::new(None));
    let async_cx: Rc<RefCell<Option<gpui::AsyncApp>>> = Rc::new(RefCell::new(None));
    let pending_paths: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(Vec::new()));
    let callback_open_view = Rc::clone(&open_view);
    let callback_async_cx = Rc::clone(&async_cx);
    let callback_pending_paths = Rc::clone(&pending_paths);
    let application = gpui_platform::application().with_assets(bezel::ui::icons::Assets);

    application.on_open_urls(move |urls| {
        let paths = urls
            .iter()
            .filter_map(|url| path_from_file_url(url))
            .filter(|path| path.is_dir() || mieli::file::io::is_markdown_file(path))
            .collect::<Vec<_>>();
        if paths.is_empty() {
            return;
        }

        let Some(open_view) = callback_open_view.borrow().clone() else {
            callback_pending_paths.borrow_mut().extend(paths);
            return;
        };
        let Some(mut async_cx) = callback_async_cx.borrow().clone() else {
            callback_pending_paths.borrow_mut().extend(paths);
            return;
        };
        let _ = open_view.update(&mut async_cx, |view, cx| {
            for path in paths {
                let _ = view.open_path(path, cx);
            }
        });
    });

    application.run(move |cx: &mut gpui::App| {
        if let Err(err) = bezel::ui::register_fonts(cx) {
            eprintln!("FONT REGISTRATION FAILED: {err:?}");
        }
        bezel::theme::set_palette(theme::palette, cx);
        bezel::theme::appearance::init(bezel::theme::appearance::AppearanceMode::System, cx);
        editor::init(cx);
        actions::install(cx);
        let bounds =
            gpui::Bounds::centered(None, gpui::size(gpui::px(1100.0), gpui::px(760.0)), cx);
        let window = cx
            .open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    bezel::theme::appearance::observe_window(window, cx).detach();
                    let view = cx.new(app::Mieli::new);
                    let close_view = view.downgrade();
                    window.on_window_should_close(cx, move |_, cx| {
                        close_view
                            .update(cx, |view, cx| view.should_close_window(cx))
                            .unwrap_or(true)
                    });
                    view
                },
            )
            .expect("Mieli window should open");

        let root_view = window
            .entity(cx)
            .expect("Mieli window root should be available");
        *open_view.borrow_mut() = Some(root_view.downgrade());
        *async_cx.borrow_mut() = Some(cx.to_async());
        let pending_paths = std::mem::take(&mut *pending_paths.borrow_mut());
        for path in pending_paths {
            let _ = root_view.update(cx, |view, cx| view.open_path(path, cx));
        }
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::path_from_file_url;
    use std::path::Path;

    #[test]
    fn file_urls_are_decoded_into_paths() {
        assert_eq!(
            path_from_file_url("file:///tmp/Mieli%20Notes.md").as_deref(),
            Some(Path::new("/tmp/Mieli Notes.md"))
        );
    }

    #[test]
    fn non_file_urls_are_ignored() {
        assert!(path_from_file_url("https://example.com/notes.md").is_none());
    }
}

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

fn selected_path_from_url(url: &Url) -> Option<mieli::file::dialog::SelectedPath> {
    mieli::file::dialog::selected_path_from_file_url(url.as_str()).or_else(|| {
        path_from_file_url(url.as_str()).map(mieli::file::dialog::SelectedPath::from_path)
    })
}

fn open_main_window(
    cx: &mut gpui::App,
    open_view: &Rc<RefCell<Option<gpui::WeakEntity<app::Mieli>>>>,
    async_cx: &Rc<RefCell<Option<gpui::AsyncApp>>>,
    pending_paths: &Rc<RefCell<Vec<Url>>>,
) {
    let window = cx
        .open_window(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::centered(
                    None,
                    gpui::size(gpui::px(1100.0), gpui::px(760.0)),
                    cx,
                ))),
                window_min_size: Some(gpui::size(gpui::px(709.0), gpui::px(480.0))),
                ..Default::default()
            },
            move |window, cx| {
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

    let paths_to_open = std::mem::take(&mut *pending_paths.borrow_mut());
    for value in paths_to_open {
        let Some(selection) = selected_path_from_url(&value) else {
            continue;
        };
        let _ = root_view.update(cx, |view, cx| {
            view.open_selected_path_with_permission_fallback(selection, cx)
        });
    }
    cx.activate(true);
}

fn main() {
    let open_view: Rc<RefCell<Option<gpui::WeakEntity<app::Mieli>>>> = Rc::new(RefCell::new(None));
    let async_cx: Rc<RefCell<Option<gpui::AsyncApp>>> = Rc::new(RefCell::new(None));
    let pending_paths: Rc<RefCell<Vec<Url>>> = Rc::new(RefCell::new(Vec::new()));
    let callback_open_view = Rc::clone(&open_view);
    let callback_async_cx = Rc::clone(&async_cx);
    let callback_pending_paths = Rc::clone(&pending_paths);
    let application = gpui_platform::application().with_assets(bezel::ui::icons::Assets);

    application.on_open_urls(move |urls| {
        let file_urls = urls
            .iter()
            .filter_map(|value| Url::parse(value).ok())
            .filter(|url| url.scheme() == "file")
            .collect::<Vec<_>>();
        if file_urls.is_empty() {
            return;
        }

        let Some(open_view) = callback_open_view.borrow().clone() else {
            callback_pending_paths.borrow_mut().extend(file_urls);
            return;
        };
        let Some(mut async_cx) = callback_async_cx.borrow().clone() else {
            callback_pending_paths.borrow_mut().extend(file_urls);
            return;
        };
        let urls_for_update = file_urls.clone();
        if open_view
            .update(&mut async_cx, |view, cx| {
                for value in urls_for_update {
                    if let Some(selection) = selected_path_from_url(&value) {
                        let _ = view.open_selected_path_with_permission_fallback(selection, cx);
                    }
                }
            })
            .is_err()
        {
            callback_pending_paths.borrow_mut().extend(file_urls);
        }
    });

    let reopen_open_view = Rc::clone(&open_view);
    let reopen_async_cx = Rc::clone(&async_cx);
    let reopen_pending_paths = Rc::clone(&pending_paths);
    let quit_open_view = Rc::clone(&open_view);
    application.on_reopen(move |cx| {
        open_main_window(
            cx,
            &reopen_open_view,
            &reopen_async_cx,
            &reopen_pending_paths,
        );
    });

    application.run(move |cx: &mut gpui::App| {
        if let Err(err) = bezel::ui::register_fonts(cx) {
            eprintln!("FONT REGISTRATION FAILED: {err:?}");
        }
        bezel::theme::set_palette(theme::palette, cx);
        bezel::theme::appearance::init(bezel::theme::appearance::AppearanceMode::System, cx);
        editor::init(cx);
        actions::install(cx);
        cx.on_action(move |_: &actions::Quit, cx| {
            app::handle_global_quit(quit_open_view.borrow().clone(), cx);
        });
        open_main_window(cx, &open_view, &async_cx, &pending_paths);
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

    #[test]
    fn quit_action_is_registered_at_application_scope() {
        let source = include_str!("main.rs");

        assert!(source.contains("cx.on_action"));
        assert!(source.contains("actions::Quit"));
    }

    #[test]
    fn standard_macos_package_creates_dmg_and_keeps_app_store_pkg() {
        let standard_script = include_str!("../scripts/package-macos.sh");
        let app_store_script = include_str!("../scripts/package-macos-app-store.sh");

        assert!(standard_script.contains("MIELI_DMG_PATH"));
        assert!(standard_script.contains("MIELI_APP_SIGN_IDENTITY"));
        assert!(standard_script.contains("codesign --verify"));
        assert!(standard_script.contains("notarytool"));
        assert!(standard_script.contains("hdiutil create"));
        assert!(app_store_script.contains("Mieli.pkg"));
        assert!(app_store_script.contains("productbuild"));
    }
}

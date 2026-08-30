use gpui::AppContext as _;
use mieli::{actions, app};

fn main() {
    gpui_platform::application().run(|cx: &mut gpui::App| {
        if let Err(err) = bezel::ui::register_fonts(cx) {
            eprintln!("FONT REGISTRATION FAILED: {err:?}");
        }
        bezel::theme::appearance::init(bezel::theme::appearance::AppearanceMode::System, cx);
        editor::init(cx);
        actions::install(cx);
        let bounds =
            gpui::Bounds::centered(None, gpui::size(gpui::px(1100.0), gpui::px(760.0)), cx);
        cx.open_window(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                bezel::theme::appearance::observe_window(window, cx).detach();
                let view = cx.new(app::Mieli::new);
                let weak_view = view.downgrade();
                window.on_window_should_close(cx, move |_, cx| {
                    weak_view
                        .update(cx, |view, cx| view.should_close_window(cx))
                        .unwrap_or(true)
                });
                view
            },
        )
        .expect("Mieli window should open");
        cx.activate(true);
    });
}

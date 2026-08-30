use bezel::{
    gpui::{Context, IntoElement, Window, div, prelude::*, px},
    theme::Theme,
    ui::widgets::{ButtonStyle, Buttons},
};

use crate::app::Mieli;

use super::{dialogs, sidebar, tabs};

pub fn render(
    view: &mut Mieli,
    window: &mut Window,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    let theme = Theme::of(cx).clone();
    let toolbar = toolbar(view, &theme, cx);
    let body = if view.state.workspace_root.is_some() || !view.state.tabs.is_empty() {
        let mut content = div().id("mieli-workspace").flex().flex_1().min_h_0();
        if view.state.sidebar_visible {
            content = content.child(sidebar::render(view, &theme, cx));
        }
        content
            .child(tabs::render(view, window, &theme, cx))
            .into_any_element()
    } else {
        empty_state(&theme, cx).into_any_element()
    };

    let mut root = div()
        .id("mieli-root")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.bg)
        .text_color(theme.text)
        .child(toolbar)
        .child(body);

    if let Some(notification) = view.notification.as_ref() {
        root = root.child(
            div()
                .id("mieli-notification")
                .absolute()
                .right(px(16.0))
                .bottom(px(16.0))
                .max_w(px(420.0))
                .p(px(12.0))
                .rounded(px(8.0))
                .bg(theme.danger_muted)
                .text_color(theme.text)
                .child(notification.message.clone()),
        );
    }

    if view.modal.is_some() {
        root = root.child(dialogs::render(view, window, &theme, cx));
    }

    root
}

fn toolbar(
    view: &mut Mieli,
    theme: &Theme,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    div()
        .id("mieli-toolbar")
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(12.0))
        .py(px(8.0))
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(
            div()
                .id("mieli-title")
                .mr(px(8.0))
                .font_weight(bezel::gpui::FontWeight::SEMIBOLD)
                .child("Mieli"),
        )
        .child(button(view, cx, "Open File", |this, cx| {
            let _ = this.open_file_dialog(cx);
        }))
        .child(button(view, cx, "Open Folder", |this, cx| {
            let _ = this.open_folder_dialog(cx);
        }))
        .child(button(view, cx, "Save", |this, cx| {
            let _ = this.save_active(cx);
        }))
        .child(button(view, cx, "Save As", |this, cx| {
            let _ = this.save_active_as(cx);
        }))
        .child(button(view, cx, "Save All", |this, cx| {
            let _ = this.save_all(cx);
        }))
        .child(
            div()
                .id("mieli-sidebar-toggle")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_sidebar(cx);
                }))
                .child(theme.button(
                    if view.state.sidebar_visible {
                        "Hide Sidebar"
                    } else {
                        "Show Sidebar"
                    },
                    ButtonStyle::Ghost,
                    None,
                )),
        )
}

fn button(
    _view: &mut Mieli,
    cx: &mut Context<Mieli>,
    label: &'static str,
    action: impl Fn(&mut Mieli, &mut Context<Mieli>) + 'static,
) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    div()
        .id(bezel::gpui::SharedString::from(format!(
            "mieli-button-{label}"
        )))
        .on_click(cx.listener(move |this, _, _, cx| action(this, cx)))
        .child(Theme::of(cx).button(label, ButtonStyle::Ghost, None))
}

fn empty_state(theme: &Theme, cx: &mut Context<Mieli>) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    div()
        .id("mieli-empty-state")
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .bg(theme.bg)
        .child(
            div()
                .font_weight(bezel::gpui::FontWeight::SEMIBOLD)
                .text_size(px(24.0))
                .child("Mieli"),
        )
        .child(
            div()
                .text_color(theme.text_muted)
                .child("Open a Markdown file or folder to start writing."),
        )
        .child(
            div()
                .flex()
                .gap(px(8.0))
                .mt(px(6.0))
                .child(
                    div()
                        .id("empty-open-file")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let _ = this.open_file_dialog(cx);
                        }))
                        .child(theme.button("Open File", ButtonStyle::Prominent, None)),
                )
                .child(
                    div()
                        .id("empty-open-folder")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let _ = this.open_folder_dialog(cx);
                        }))
                        .child(theme.button("Open Folder", ButtonStyle::Ghost, None)),
                ),
        )
}

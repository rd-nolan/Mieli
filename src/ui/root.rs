use std::sync::Arc;

use bezel::{
    gpui::{
        Context, DragMoveEvent, FontWeight, Image, ImageFormat, MouseButton, Pixels, Window, div,
        img, prelude::*, px, white,
    },
    theme::Theme,
    ui::{
        icons::{self, icon},
        tooltip::Tooltip,
        widgets::{ButtonStyle, SplitDrag},
    },
};

use crate::{app::Mieli, i18n::TextKey, state::NotificationKind};

use super::{dialogs, sidebar, tabs};

pub(crate) const SIDEBAR_MIN_WIDTH: f32 = 200.0;
pub(crate) const EDITOR_MIN_WIDTH: f32 = 500.0;
pub(crate) const SPLIT_HANDLE_WIDTH: f32 = 9.0;
pub(crate) const PANEL_HEADER_HEIGHT: f32 = 33.0;
pub(crate) const WORKSPACE_MIN_WIDTH: f32 =
    SIDEBAR_MIN_WIDTH + SPLIT_HANDLE_WIDTH + EDITOR_MIN_WIDTH;

pub(crate) fn clamp_sidebar_width(requested: Pixels, available_width: Pixels) -> Pixels {
    let maximum = (available_width - px(SPLIT_HANDLE_WIDTH + EDITOR_MIN_WIDTH))
        .as_f32()
        .max(SIDEBAR_MIN_WIDTH);
    px(requested.as_f32().clamp(SIDEBAR_MIN_WIDTH, maximum))
}

pub fn render(
    view: &mut Mieli,
    window: &mut Window,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    let theme = Theme::of(cx).clone();
    let toolbar = toolbar(view, &theme, cx);
    let has_tabs = !view.state.tabs.is_empty();

    let mut body = div()
        .id("mieli-workspace")
        .flex()
        .flex_1()
        .min_w(px(WORKSPACE_MIN_WIDTH))
        .min_h_0()
        .relative()
        .bg(theme.bg);

    if view.state.sidebar_visible {
        let sidebar_width = clamp_sidebar_width(view.sidebar_width, window.viewport_size().width);
        let split_indicator = if view.sidebar_dragging {
            theme.accent.opacity(0.78)
        } else {
            theme.border.opacity(0.88)
        };
        let split_handle_side_width = (SPLIT_HANDLE_WIDTH - 1.0) / 2.0;
        let split_handle = div()
            .id("mieli-sidebar-split-handle")
            .relative()
            .w(px(SPLIT_HANDLE_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .items_center()
            .cursor_col_resize()
            .child(
                div()
                    .h_full()
                    .w(px(split_handle_side_width))
                    .bg(theme.surface),
            )
            .child(div().h_full().w(px(1.0)).bg(split_indicator))
            .child(div().h_full().w(px(split_handle_side_width)).bg(theme.bg))
            .on_drag(SplitDrag, |_, _, _, cx| cx.new(|_| bezel::gpui::Empty));

        body = body
            .on_drag_move(
                cx.listener(|view, event: &DragMoveEvent<SplitDrag>, _, cx| {
                    let requested = event.event.position.x - event.bounds.origin.x;
                    view.sidebar_width = clamp_sidebar_width(requested, event.bounds.size.width);
                    view.sidebar_dragging = true;
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    view.sidebar_dragging = false;
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    view.sidebar_dragging = false;
                    cx.notify();
                }),
            )
            .child(sidebar::render(view, &theme, sidebar_width, cx))
            .child(split_handle);
    }

    if has_tabs {
        body = body.child(tabs::render(view, window, &theme, cx));
    } else {
        body = body.child(empty_state(view, &theme, cx));
    }
    body = body.child(
        div()
            .absolute()
            .left(px(0.0))
            .right(px(0.0))
            .top(px(PANEL_HEADER_HEIGHT - 1.0))
            .h(px(1.0))
            .bg(theme.border),
    );
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
        let is_success = matches!(notification.kind, NotificationKind::Success);
        let status_icon = if is_success {
            icons::CHECK
        } else {
            icons::DANGER_TRIANGLE
        };
        let status_color = if is_success {
            theme.success
        } else {
            theme.warning
        };
        let status_border = if is_success {
            theme.border
        } else {
            theme.warning
        };
        let status_background = if is_success {
            theme.surface_card
        } else {
            theme.warning_muted
        };
        let toast_gap = if is_success { px(6.0) } else { px(7.0) };
        let toast_padding = if is_success { px(7.0) } else { px(9.0) };
        let toast_icon_size = if is_success { px(15.0) } else { px(17.0) };
        let toast_text_size = if is_success { px(12.0) } else { px(13.0) };
        let mut notification_toast = div()
            .id("mieli-notification")
            .absolute()
            .max_w(if is_success { px(240.0) } else { px(380.0) })
            .flex()
            .items_center()
            .gap(toast_gap)
            .p(toast_padding)
            .rounded(px(Theme::control_radius()))
            .border_1()
            .border_color(status_border)
            .bg(status_background)
            .shadow_sm()
            .text_color(theme.text)
            .child(
                icon(status_icon)
                    .size(toast_icon_size)
                    .text_color(status_color),
            )
            .child(
                div()
                    .id("mieli-notification-message")
                    .min_w(px(0.0))
                    .flex_1()
                    .text_size(toast_text_size)
                    .truncate()
                    .child(notification.message.clone()),
            );

        if is_success {
            notification_toast = notification_toast
                .left(px(0.0))
                .right(px(0.0))
                .bottom(px(44.0))
                .mx_auto();
        } else {
            notification_toast = notification_toast.right(px(18.0)).bottom(px(18.0));
        }

        if !is_success {
            notification_toast = notification_toast.child(
                div()
                    .id("mieli-dismiss-notification")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(22.0))
                    .rounded(px(Theme::control_radius()))
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.element_hover))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.dismiss_notification(cx);
                    }))
                    .child(
                        icon(icons::CLOSE)
                            .size(px(13.0))
                            .text_color(theme.text_muted),
                    ),
            );
        }

        root = root.child(notification_toast);
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
    let language = view.language();
    let sidebar_label = language.sidebar_toggle(view.state.sidebar_visible);

    let mut sidebar_button = toolbar_icon_button(
        cx,
        theme,
        "mieli-sidebar-toggle",
        sidebar_label,
        icons::SIDEBAR_MINIMALISTIC_LEFT,
        ButtonStyle::Ghost,
        |this, cx| {
            this.toggle_sidebar(cx);
        },
    );
    if view.state.sidebar_visible {
        sidebar_button = sidebar_button.bg(theme.element_active);
    }

    let bar = div()
        .id("mieli-toolbar")
        .flex()
        .items_center()
        .flex_none()
        .gap(px(2.0))
        .px(px(8.0))
        .h(px(34.0))
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(sidebar_button)
        .child(brand(theme))
        .child(div().flex_1());

    let actions = div()
        .flex()
        .items_center()
        .gap(px(2.0))
        .child(toolbar_icon_button(
            cx,
            theme,
            "mieli-button-open",
            language.text(TextKey::Open),
            icons::FOLDER_WITH_FILES,
            ButtonStyle::Ghost,
            |this, cx| {
                let _ = this.open_path_dialog(cx);
            },
        ))
        .child(language_button(view, theme, cx));

    bar.child(actions)
}

fn language_button(
    view: &mut Mieli,
    theme: &Theme,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    let language = view.language();
    div()
        .id("mieli-language-toggle")
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(30.0))
        .h(px(26.0))
        .px(px(5.0))
        .rounded(px(Theme::control_radius()))
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(11.0))
        .text_color(theme.text_muted)
        .cursor_pointer()
        .tooltip(move |window, cx| {
            Tooltip::text(language.text(TextKey::SwitchLanguage), window, cx)
        })
        .hover(|style| style.bg(theme.element_hover).text_color(theme.text))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_language(cx)))
        .child(language.short_label())
}

fn brand(theme: &Theme) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    div()
        .id("mieli-brand")
        .flex()
        .items_center()
        .gap(px(5.0))
        .px(px(2.0))
        .child(
            div()
                .size(px(20.0))
                .rounded(px(5.0))
                .overflow_hidden()
                .child(app_logo(20.0)),
        )
        .child(
            div()
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child("Mieli"),
        )
}

fn toolbar_icon_button(
    cx: &mut Context<Mieli>,
    theme: &Theme,
    id: &'static str,
    label: &'static str,
    icon_path: &'static str,
    style: ButtonStyle,
    action: impl Fn(&mut Mieli, &mut Context<Mieli>) + 'static,
) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    let (background, foreground) = match style {
        ButtonStyle::Ghost => (None, theme.text_muted),
        ButtonStyle::Prominent => (Some(theme.accent), theme.on_accent),
        ButtonStyle::Destructive => (Some(theme.danger_strong), white()),
    };

    let mut button = div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(26.0))
        .rounded(px(Theme::control_radius()))
        .text_size(px(12.0))
        .text_color(foreground)
        .cursor_pointer()
        .tooltip(move |window, cx| Tooltip::text(label, window, cx))
        .on_click(cx.listener(move |this, _, _, cx| action(this, cx)))
        .child(icon(icon_path).size(px(14.0)).text_color(foreground));

    if let Some(background) = background {
        button = button.bg(background);
    }

    match style {
        ButtonStyle::Ghost => button.hover(|style| style.bg(theme.element_hover)),
        ButtonStyle::Prominent | ButtonStyle::Destructive => {
            button.hover(|style| style.opacity(0.9))
        }
    }
}

fn command_button(
    cx: &mut Context<Mieli>,
    theme: &Theme,
    id: &'static str,
    label: &'static str,
    icon_path: &'static str,
    style: ButtonStyle,
    action: impl Fn(&mut Mieli, &mut Context<Mieli>) + 'static,
) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    let (background, foreground) = match style {
        ButtonStyle::Ghost => (None, theme.text_muted),
        ButtonStyle::Prominent => (Some(theme.accent), theme.on_accent),
        ButtonStyle::Destructive => (Some(theme.danger_strong), white()),
    };

    let mut button = div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(4.0))
        .px(px(7.0))
        .py(px(3.0))
        .rounded(px(Theme::control_radius()))
        .text_size(px(12.0))
        .font_weight(if style == ButtonStyle::Prominent {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        })
        .text_color(foreground)
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| action(this, cx)))
        .child(icon(icon_path).size(px(14.0)).text_color(foreground))
        .child(label);

    if let Some(background) = background {
        button = button.bg(background);
    }

    match style {
        ButtonStyle::Ghost => button.hover(|style| style.bg(theme.element_hover)),
        ButtonStyle::Prominent | ButtonStyle::Destructive => {
            button.hover(|style| style.opacity(0.9))
        }
    }
}

fn empty_state(
    view: &mut Mieli,
    theme: &Theme,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    let language = view.language();
    let workspace_open = view.state.workspace_root.is_some();
    let empty_hint = if view.workspace_scan_loading() {
        TextKey::LoadingWorkspace
    } else if workspace_open {
        if view.state.file_tree.is_empty() {
            TextKey::NoMarkdownFiles
        } else {
            TextKey::NoDocumentSelected
        }
    } else {
        TextKey::WelcomeHint
    };
    let empty_action = if workspace_open {
        command_button(
            cx,
            theme,
            "empty-new-document",
            language.text(TextKey::NewDocument),
            icons::PLUS,
            ButtonStyle::Prominent,
            |this, cx| {
                this.new_tab(cx);
            },
        )
    } else {
        command_button(
            cx,
            theme,
            "empty-open",
            language.text(TextKey::Open),
            icons::FOLDER_WITH_FILES,
            ButtonStyle::Prominent,
            |this, cx| {
                let _ = this.open_path_dialog(cx);
            },
        )
    };
    div()
        .id("mieli-empty-state")
        .flex_1()
        .min_w(px(EDITOR_MIN_WIDTH))
        .flex()
        .items_center()
        .justify_center()
        .p(px(24.0))
        .bg(theme.bg)
        .child(
            div()
                .id("mieli-welcome-card")
                .w_full()
                .max_w(px(360.0))
                .flex()
                .flex_col()
                .items_center()
                .child(
                    icon(icons::DOCUMENT)
                        .size(px(28.0))
                        .text_color(theme.accent),
                )
                .child(
                    div()
                        .mt(px(12.0))
                        .text_size(px(13.0))
                        .text_color(theme.text_muted)
                        .text_center()
                        .child(language.text(empty_hint)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .mt(px(16.0))
                        .child(empty_action),
                ),
        )
}

fn app_logo(size: f32) -> bezel::gpui::Img {
    img(Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../resources/mieli_logo_1024x1024.png").to_vec(),
    )))
    .size(px(size))
}

#[cfg(test)]
mod tests {
    use bezel::gpui::px;

    use super::clamp_sidebar_width;

    #[test]
    fn sidebar_width_clamps_to_minimum() {
        assert_eq!(clamp_sidebar_width(px(100.0), px(1200.0)), px(200.0));
    }

    #[test]
    fn sidebar_width_preserves_editor_minimum() {
        assert_eq!(clamp_sidebar_width(px(800.0), px(1200.0)), px(691.0));
    }

    #[test]
    fn sidebar_width_stays_at_minimum_when_window_is_too_narrow() {
        assert_eq!(clamp_sidebar_width(px(300.0), px(600.0)), px(200.0));
    }
}

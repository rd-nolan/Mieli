use std::sync::Arc;

use bezel::{
    gpui::{Context, FontWeight, Image, ImageFormat, Window, div, img, prelude::*, px, white},
    theme::Theme,
    ui::{
        icons::{self, icon},
        tooltip::Tooltip,
        widgets::ButtonStyle,
    },
};

use crate::{app::Mieli, i18n::TextKey, state::NotificationKind};

use super::{dialogs, sidebar, tabs};

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
        .min_h_0()
        .bg(theme.bg);

    if view.state.sidebar_visible {
        body = body.child(sidebar::render(view, &theme, cx));
    }

    if has_tabs {
        body = body.child(tabs::render(view, window, &theme, cx));
    } else {
        body = body.child(empty_state(view, &theme, cx));
    }

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
        let mut notification_toast = div()
            .id("mieli-notification")
            .absolute()
            .right(px(18.0))
            .bottom(px(18.0))
            .max_w(px(380.0))
            .flex()
            .items_center()
            .gap(px(7.0))
            .p(px(9.0))
            .rounded(px(Theme::control_radius()))
            .border_1()
            .border_color(status_border)
            .bg(status_background)
            .shadow_sm()
            .text_color(theme.text)
            .child(icon(status_icon).size(px(17.0)).text_color(status_color))
            .child(
                div()
                    .id("mieli-notification-message")
                    .min_w(px(0.0))
                    .flex_1()
                    .text_size(px(13.0))
                    .truncate()
                    .child(notification.message.clone()),
            );

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
    div()
        .id("mieli-empty-state")
        .flex_1()
        .min_w_0()
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
                        .child(language.text(TextKey::WelcomeHint)),
                )
                .child(div().flex().items_center().gap(px(4.0)).mt(px(16.0)).child(
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
                    ),
                )),
        )
}

fn app_logo(size: f32) -> bezel::gpui::Img {
    img(Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../resources/mieli_logo_1024x1024.png").to_vec(),
    )))
    .size(px(size))
}

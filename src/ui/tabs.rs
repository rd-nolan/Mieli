use bezel::{
    gpui::{Context, ElementId, SharedString, Window, div, prelude::*, px},
    theme::Theme,
};

use crate::app::Mieli;

pub fn render(
    view: &mut Mieli,
    _window: &mut Window,
    theme: &Theme,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    let tab_strip = tab_strip(view, theme, cx);
    let editor = editor_surface(view, theme);
    div()
        .id("mieli-main-pane")
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .bg(theme.bg)
        .child(tab_strip)
        .child(editor)
}

fn tab_strip(
    view: &mut Mieli,
    theme: &Theme,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    let active_tab = view.state.active_tab;
    let mut strip = div()
        .id("mieli-tabs")
        .flex()
        .items_center()
        .gap(px(2.0))
        .px(px(8.0))
        .py(px(6.0))
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.surface);

    for tab in &view.state.tabs {
        let tab_id = tab.id;
        let close_id = tab.id;
        let label = if tab.dirty {
            format!("• {}", tab.title)
        } else {
            tab.title.clone()
        };
        let selected = active_tab == Some(tab.id);
        let tab_element_id = ElementId::Name(SharedString::from(format!("tab-{}", tab.id.0)));
        let tab_label_element_id =
            ElementId::Name(SharedString::from(format!("tab-label-{}", tab.id.0)));
        let close_element_id =
            ElementId::Name(SharedString::from(format!("tab-close-{}", tab.id.0)));
        let tab_button = div()
            .id(tab_element_id)
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .py(px(5.0))
            .rounded(px(5.0))
            .text_color(if selected {
                theme.text
            } else {
                theme.text_muted
            })
            .when(selected, |tab| tab.bg(theme.element_active))
            .hover(|style| style.bg(theme.element_hover))
            .child(
                div()
                    .id(tab_label_element_id)
                    .flex()
                    .items_center()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.switch_tab(tab_id, cx);
                    }))
                    .child(label),
            )
            .child(
                div()
                    .id(close_element_id)
                    .flex()
                    .items_center()
                    .px(px(2.0))
                    .text_color(theme.text_faint)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.close_tab(close_id, cx);
                    }))
                    .child("×"),
            );
        strip = strip.child(tab_button);
    }
    strip
}

fn editor_surface(view: &mut Mieli, theme: &Theme) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    let Some((editor, scroll)) = view.active_editor_surface() else {
        return div()
            .id("mieli-no-editor")
            .flex_1()
            .items_center()
            .justify_center()
            .text_color(theme.text_muted)
            .child("No document selected.");
    };

    div()
        .id("mieli-editor-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&scroll)
        .p(px(24.0))
        .child(editor)
}

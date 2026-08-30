use bezel::{
    gpui::{
        Context, ElementId, Focusable, MouseButton, SharedString, StyleRefinement, Window, div,
        prelude::*, px,
    },
    theme::Theme,
    ui::{
        icons::{self, icon},
        tooltip::Tooltip,
    },
};

use crate::app::Mieli;

pub fn render(
    view: &mut Mieli,
    _window: &mut Window,
    theme: &Theme,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    let tab_strip = tab_strip(view, theme, cx);
    let editor = editor_surface(view, theme, cx);
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
        .py(px(2.0))
        .bg(theme.bg);

    for tab in &view.state.tabs {
        let tab_id = tab.id;
        let close_id = tab.id;
        let selected = active_tab == Some(tab.id);
        let tab_element_id = ElementId::Name(SharedString::from(format!("tab-{}", tab.id.0)));
        let tab_label_element_id =
            ElementId::Name(SharedString::from(format!("tab-label-{}", tab.id.0)));
        let close_element_id =
            ElementId::Name(SharedString::from(format!("tab-close-{}", tab.id.0)));
        let title = tab.title.clone();
        let dirty = tab.dirty;

        let dirty_indicator = div()
            .size(px(6.0))
            .rounded(px(99.0))
            .bg(theme.warning)
            .when(!dirty, |dot| dot.opacity(0.0));

        let tab_label = div()
            .id(tab_label_element_id)
            .flex()
            .items_center()
            .gap(px(6.0))
            .min_w(px(72.0))
            .max_w(px(180.0))
            .px(px(7.0))
            .py(px(3.0))
            .rounded(px(Theme::control_radius()))
            .text_size(px(12.0))
            .text_color(if selected {
                theme.text
            } else {
                theme.text_muted
            })
            .when(selected, |tab| {
                tab.bg(theme.surface_card)
                    .border_b_1()
                    .border_color(theme.accent)
            })
            .hover(|style| style.bg(theme.element_hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.switch_tab(tab_id, cx);
            }))
            .child(
                icon(icons::DOCUMENT)
                    .size(px(13.0))
                    .text_color(if selected {
                        theme.accent
                    } else {
                        theme.text_faint
                    }),
            )
            .child(dirty_indicator)
            .child(div().min_w(px(0.0)).flex_1().truncate().child(title));

        let tab_button = div()
            .id(tab_element_id)
            .flex()
            .items_center()
            .flex_none()
            .gap(px(2.0))
            .child(tab_label)
            .child(
                div()
                    .id(close_element_id)
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(18.0))
                    .rounded(px(Theme::control_radius()))
                    .cursor_pointer()
                    .tooltip(|window, cx| Tooltip::text("Close tab", window, cx))
                    .text_color(theme.text_faint)
                    .hover(|style| style.bg(theme.element_hover).text_color(theme.text))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.close_tab(close_id, cx);
                    }))
                    .child(icon(icons::CLOSE).size(px(12.0))),
            );
        strip = strip.child(tab_button);
    }

    strip.child(
        div()
            .id("mieli-tab-new")
            .flex()
            .items_center()
            .justify_center()
            .size(px(24.0))
            .rounded(px(Theme::control_radius()))
            .text_color(theme.text_muted)
            .cursor_pointer()
            .tooltip(|window, cx| Tooltip::text("New Document", window, cx))
            .hover(|style| style.bg(theme.element_hover).text_color(theme.text))
            .on_click(cx.listener(|this, _, _, cx| {
                this.new_tab(cx);
            }))
            .child(icon(icons::PLUS).size(px(14.0))),
    )
}

fn editor_surface(
    view: &mut Mieli,
    theme: &Theme,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    let Some((editor, scroll)) = view.active_editor_surface() else {
        return div()
            .id("mieli-no-editor")
            .flex_1()
            .items_center()
            .justify_center()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .text_color(theme.text_muted)
            .child(
                icon(icons::DOCUMENT)
                    .size(px(24.0))
                    .text_color(theme.text_faint),
            )
            .child("No document selected.");
    };

    let empty_document = view
        .state
        .active_tab
        .and_then(|active_id| view.state.tabs.iter().find(|tab| tab.id == active_id))
        .is_some_and(|tab| tab.path.as_os_str().is_empty() && tab.saved_source.is_empty());

    let empty_editor = editor.clone();
    let editor_content = if empty_document {
        div()
            .id("mieli-empty-editor-hit-area")
            .w_full()
            .h(px(600.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _: &bezel::gpui::MouseDownEvent, window, cx| {
                    empty_editor.focus_handle(cx).focus(window, cx);
                }),
            )
            .child(editor.cached(StyleRefinement::default().size_full()))
            .into_any_element()
    } else {
        editor.into_any_element()
    };

    div()
        .id("mieli-editor-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&scroll)
        .px(px(32.0))
        .py(px(28.0))
        .bg(theme.bg)
        .child(
            div()
                .id("mieli-editor-page")
                .w_full()
                .max_w(px(820.0))
                .mx_auto()
                .child(editor_content),
        )
}

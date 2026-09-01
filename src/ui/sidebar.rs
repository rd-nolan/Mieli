use std::{path::Path, time::Duration};

use bezel::{
    gpui::{
        Animation, AnimationExt, Context, ElementId, IntoElement, Pixels, SharedString, div,
        ease_out_quint, prelude::*, px,
    },
    theme::Theme,
    ui::icons::{self, icon},
};

use crate::{app::Mieli, i18n::TextKey};

use super::{file_tree::visible_rows, root::PANEL_HEADER_HEIGHT};

pub fn render(
    view: &mut Mieli,
    theme: &Theme,
    width: Pixels,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::Stateful<bezel::gpui::Div> {
    let active_path = view.state.active_tab.and_then(|active_id| {
        view.state
            .tabs
            .iter()
            .find(|tab| tab.id == active_id)
            .map(|tab| tab.path.as_path())
    });
    let rows = visible_rows(&view.state.file_tree, active_path);
    let has_rows = !rows.is_empty();
    let workspace_name = view.state.workspace_root.as_deref().map(path_display_name);
    let language = view.language();
    let scan_loading = view.workspace_scan_loading();
    let showing_previous_tree = view.workspace_scan_showing_previous_tree();

    let mut tree = div().id("mieli-file-tree").flex().flex_col();
    for row in rows {
        let path = row.path.clone();
        let is_dir = row.is_dir;
        let selected = row.selected;
        let icon_path = if is_dir {
            icons::FOLDER
        } else {
            icons::DOCUMENT
        };
        let disclosure = if is_dir {
            let disclosure_id = ElementId::Name(SharedString::from(format!(
                "tree-disclosure-{}-{}",
                path.display(),
                row.expanded
            )));
            icon(disclosure_icon_path(row.expanded))
                .size(px(11.0))
                .text_color(theme.text_faint)
                .with_animation(
                    disclosure_id,
                    Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()),
                    |icon, delta| icon.opacity(delta),
                )
                .into_any_element()
        } else {
            div().size(px(12.0)).into_any_element()
        };
        let label = row.name.trim_end_matches('/').to_string();
        let id = ElementId::Name(SharedString::from(format!("tree-{}", path.display())));
        let callback_path = path.clone();

        let mut item = div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(5.0))
            .pl(px(6.0 + row.depth as f32 * 14.0))
            .pr(px(6.0))
            .py(px(3.0))
            .rounded(px(Theme::control_radius()))
            .text_size(px(12.0))
            .text_color(if selected {
                theme.text
            } else {
                theme.text_muted
            })
            .when(selected, |item| item.bg(theme.element_active))
            .hover(|style| style.bg(theme.element_hover))
            .child(disclosure)
            .child(icon(icon_path).size(px(13.0)).text_color(if selected {
                theme.accent
            } else {
                theme.text_muted
            }))
            .child(div().min_w(px(0.0)).flex_1().truncate().child(label));

        if !showing_previous_tree {
            item = item.on_click(cx.listener(move |this, _, _, cx| {
                if is_dir {
                    this.toggle_tree_path(&callback_path, cx);
                } else {
                    let _ = this.open_file(callback_path.clone(), cx);
                }
            }));
        }
        if is_dir {
            item = item.font_weight(bezel::gpui::FontWeight::MEDIUM);
        }
        let row_animation_id = ElementId::Name(SharedString::from(format!(
            "tree-row-{}-{}",
            path.display(),
            is_dir && row.expanded
        )));
        tree = tree.child(item.with_animation(
            row_animation_id,
            Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
            |item, delta| item.opacity(delta),
        ));
    }
    if showing_previous_tree {
        tree = tree.opacity(0.45);
    }

    let empty_message = if !scan_loading && view.state.workspace_root.is_some() && !has_rows {
        language.text(TextKey::NoMarkdownFiles)
    } else {
        ""
    };

    let mut header = div()
        .id("mieli-sidebar-header")
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(10.0))
        .h(px(PANEL_HEADER_HEIGHT))
        .flex_none()
        .child(
            icon(icons::FOLDER_WITH_FILES)
                .size(px(14.0))
                .text_color(theme.accent),
        )
        .child(
            div()
                .min_w(px(0.0))
                .flex_1()
                .truncate()
                .text_size(px(12.0))
                .font_weight(bezel::gpui::FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(
                    workspace_name
                        .clone()
                        .unwrap_or_else(|| language.text(TextKey::Workspace).to_string()),
                ),
        );

    if workspace_name.is_none() {
        header = header.child(
            div()
                .id("mieli-sidebar-open")
                .px(px(5.0))
                .py(px(2.0))
                .rounded(px(Theme::control_radius()))
                .text_size(px(11.0))
                .text_color(theme.accent)
                .cursor_pointer()
                .hover(|style| style.bg(theme.element_hover))
                .on_click(cx.listener(|this, _, _, cx| {
                    let _ = this.open_path_dialog(cx);
                }))
                .child(language.text(TextKey::Open)),
        );
    }

    div()
        .id("mieli-sidebar")
        .w(width)
        .min_w(px(200.0))
        .flex_none()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme.surface)
        .child(header)
        .child(
            div()
                .id("mieli-sidebar-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(5.0))
                .child(tree)
                .when(!empty_message.is_empty(), |content| {
                    content.child(
                        div()
                            .p(px(8.0))
                            .text_size(px(11.0))
                            .text_color(theme.text_faint)
                            .child(empty_message),
                    )
                }),
        )
}

fn path_display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

fn disclosure_icon_path(expanded: bool) -> &'static str {
    if expanded {
        icons::ARROW_DOWN
    } else {
        icons::ARROW_RIGHT
    }
}

#[cfg(test)]
mod tests {
    use bezel::ui::icons;

    use super::disclosure_icon_path;

    #[test]
    fn disclosure_icon_changes_with_expansion_state() {
        assert_eq!(disclosure_icon_path(true), icons::ARROW_DOWN);
        assert_eq!(disclosure_icon_path(false), icons::ARROW_RIGHT);
    }
}

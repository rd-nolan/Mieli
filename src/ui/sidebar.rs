use bezel::{
    gpui::{Context, ElementId, SharedString, div, prelude::*, px},
    theme::Theme,
};

use crate::app::Mieli;

use super::file_tree::visible_rows;

pub fn render(
    view: &mut Mieli,
    theme: &Theme,
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

    let mut tree = div().id("mieli-file-tree").flex().flex_col().gap(px(2.0));
    for row in rows {
        let path = row.path.clone();
        let is_dir = row.is_dir;
        let marker = if row.is_dir {
            if row.expanded { "▾" } else { "▸" }
        } else {
            "·"
        };
        let label = row.name;
        let selected = row.selected;
        let id = ElementId::Name(SharedString::from(format!("tree-{}", path.display())));
        let mut item = div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(6.0))
            .pl(px(10.0 + row.depth as f32 * 16.0))
            .pr(px(8.0))
            .py(px(5.0))
            .rounded(px(5.0))
            .text_color(if selected {
                theme.text
            } else {
                theme.text_muted
            })
            .when(selected, |item| item.bg(theme.element_active))
            .hover(|style| style.bg(theme.element_hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                if is_dir {
                    this.toggle_tree_path(&path, cx);
                } else {
                    let _ = this.open_file(path.clone(), cx);
                }
            }))
            .child(div().w(px(14.0)).text_color(theme.text_faint).child(marker))
            .child(label);
        if is_dir {
            item = item.font_weight(bezel::gpui::FontWeight::MEDIUM);
        }
        tree = tree.child(item);
    }

    div()
        .id("mieli-sidebar")
        .w(px(240.0))
        .flex_none()
        .min_h_0()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(
            div()
                .px(px(12.0))
                .py(px(10.0))
                .text_size(px(12.0))
                .font_weight(bezel::gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_faint)
                .child("FILES"),
        )
        .child(
            div()
                .id("mieli-sidebar-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(6.0))
                .child(tree),
        )
        .when(view.state.workspace_root.is_none(), |sidebar| {
            sidebar.child(
                div()
                    .p(px(12.0))
                    .text_color(theme.text_faint)
                    .child("Open a folder to browse Markdown files."),
            )
        })
}

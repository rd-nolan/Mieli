use bezel::{
    gpui::{Context, IntoElement, Window, div, prelude::*, px},
    theme::Theme,
    ui::{
        self,
        widgets::{ButtonStyle, Buttons},
    },
};

use crate::{app::Mieli, state::Modal};

pub fn render(
    view: &mut Mieli,
    window: &mut Window,
    theme: &Theme,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::AnyElement {
    match view.modal {
        Some(Modal::CloseTab(tab_id)) => close_tab_modal(view, tab_id, window, theme, cx),
        Some(Modal::ExternalConflict(tab_id)) => {
            external_conflict_modal(view, tab_id, window, theme, cx)
        }
        Some(Modal::DeletedFile(tab_id)) => deleted_file_modal(view, tab_id, window, theme, cx),
        Some(Modal::Shutdown) => shutdown_modal(window, theme, cx),
        None => div().into_any_element(),
    }
}

fn close_tab_modal(
    view: &mut Mieli,
    tab_id: crate::state::TabId,
    window: &mut Window,
    theme: &Theme,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::AnyElement {
    let title = view
        .state
        .tabs
        .iter()
        .find(|tab| tab.id == tab_id)
        .map(|tab| tab.title.clone())
        .unwrap_or_else(|| String::from("this tab"));
    let card = ui::popover::dialog_card(theme)
        .gap(px(12.0))
        .child(ui::popover::dialog_title(theme, "Unsaved changes"))
        .child(ui::popover::dialog_body(
            theme,
            format!("Save changes to {title} before closing?"),
        ))
        .child(
            div()
                .mt(px(4.0))
                .flex()
                .justify_end()
                .gap(px(8.0))
                .child(
                    div()
                        .id("close-save")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let _ = this.save_and_close_tab(tab_id, cx);
                        }))
                        .child(theme.button("Save", ButtonStyle::Prominent, None)),
                )
                .child(
                    div()
                        .id("close-discard")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.discard_close_tab(tab_id, cx);
                        }))
                        .child(theme.button("Don't Save", ButtonStyle::Destructive, None)),
                )
                .child(
                    div()
                        .id("close-cancel")
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_modal(cx)))
                        .child(theme.button("Cancel", ButtonStyle::Ghost, None)),
                ),
        );

    ui::popover::modal(
        "mieli-modal",
        window.viewport_size(),
        card.into_any_element(),
        cx.listener(|this, _, _, cx| this.dismiss_modal(cx)),
    )
}

fn external_conflict_modal(
    view: &mut Mieli,
    tab_id: crate::state::TabId,
    window: &mut Window,
    theme: &Theme,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::AnyElement {
    let title = tab_title(view, tab_id);
    let card = ui::popover::dialog_card(theme)
        .gap(px(12.0))
        .child(ui::popover::dialog_title(theme, "File changed on disk"))
        .child(ui::popover::dialog_body(
            theme,
            format!("{title} changed outside Mieli. Reload the disk version or keep your edits?"),
        ))
        .child(
            div()
                .mt(px(4.0))
                .flex()
                .justify_end()
                .gap(px(8.0))
                .child(
                    div()
                        .id("conflict-reload")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let _ = this.reload_external_file(tab_id, cx);
                        }))
                        .child(theme.button("Reload from Disk", ButtonStyle::Prominent, None)),
                )
                .child(
                    div()
                        .id("conflict-keep-mine")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.keep_mine(tab_id, cx);
                        }))
                        .child(theme.button("Keep Mine", ButtonStyle::Ghost, None)),
                )
                .child(
                    div()
                        .id("conflict-cancel")
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_modal(cx)))
                        .child(theme.button("Cancel", ButtonStyle::Ghost, None)),
                ),
        );

    ui::popover::modal(
        "mieli-modal",
        window.viewport_size(),
        card.into_any_element(),
        cx.listener(|this, _, _, cx| this.dismiss_modal(cx)),
    )
}

fn deleted_file_modal(
    view: &mut Mieli,
    tab_id: crate::state::TabId,
    window: &mut Window,
    theme: &Theme,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::AnyElement {
    let title = tab_title(view, tab_id);
    let card = ui::popover::dialog_card(theme)
        .gap(px(12.0))
        .child(ui::popover::dialog_title(theme, "File deleted on disk"))
        .child(ui::popover::dialog_body(
            theme,
            format!("{title} was deleted outside Mieli. Keep the editor open or close it?"),
        ))
        .child(
            div()
                .mt(px(4.0))
                .flex()
                .justify_end()
                .gap(px(8.0))
                .child(
                    div()
                        .id("deleted-keep-open")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.keep_deleted_file_open(tab_id, cx);
                        }))
                        .child(theme.button("Keep Open", ButtonStyle::Prominent, None)),
                )
                .child(
                    div()
                        .id("deleted-close")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.close_deleted_file(tab_id, cx);
                        }))
                        .child(theme.button("Close", ButtonStyle::Destructive, None)),
                ),
        );

    ui::popover::modal(
        "mieli-modal",
        window.viewport_size(),
        card.into_any_element(),
        cx.listener(|this, _, _, cx| this.dismiss_modal(cx)),
    )
}

fn shutdown_modal(
    window: &mut Window,
    theme: &Theme,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::AnyElement {
    let card = ui::popover::dialog_card(theme)
        .gap(px(12.0))
        .child(ui::popover::dialog_title(theme, "Save failed"))
        .child(ui::popover::dialog_body(
            theme,
            "Mieli could not save every document. Quit anyway and lose unsaved changes?",
        ))
        .child(
            div()
                .mt(px(4.0))
                .flex()
                .justify_end()
                .gap(px(8.0))
                .child(
                    div()
                        .id("shutdown-quit-anyway")
                        .on_click(cx.listener(|this, _, _, cx| this.quit_anyway(cx)))
                        .child(theme.button("Quit Anyway", ButtonStyle::Destructive, None)),
                )
                .child(
                    div()
                        .id("shutdown-cancel")
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_modal(cx)))
                        .child(theme.button("Cancel", ButtonStyle::Ghost, None)),
                ),
        );

    ui::popover::modal(
        "mieli-modal",
        window.viewport_size(),
        card.into_any_element(),
        cx.listener(|this, _, _, cx| this.dismiss_modal(cx)),
    )
}

fn tab_title(view: &Mieli, tab_id: crate::state::TabId) -> String {
    view.state
        .tabs
        .iter()
        .find(|tab| tab.id == tab_id)
        .map(|tab| tab.title.clone())
        .unwrap_or_else(|| String::from("This file"))
}

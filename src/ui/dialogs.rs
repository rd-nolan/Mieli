use bezel::{
    gpui::{Context, IntoElement, Window, div, prelude::*, px},
    theme::Theme,
    ui::{
        self,
        widgets::{ButtonStyle, Buttons},
    },
};

use crate::{
    app::Mieli,
    i18n::{Language, TextKey},
    state::Modal,
};

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
        Some(Modal::Shutdown) => shutdown_modal(view.language(), window, theme, cx),
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
    let language = view.language();
    let title = view
        .state
        .tabs
        .iter()
        .find(|tab| tab.id == tab_id)
        .map(|tab| tab.title.clone())
        .unwrap_or_else(|| language.text(TextKey::ThisTab).to_string());
    let card = ui::popover::dialog_card(theme)
        .gap(px(12.0))
        .child(ui::popover::dialog_title(
            theme,
            language.text(TextKey::UnsavedChanges),
        ))
        .child(ui::popover::dialog_body(
            theme,
            language.save_changes_before_closing(&title),
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
                        .child(theme.button(
                            language.text(TextKey::Save),
                            ButtonStyle::Prominent,
                            None,
                        )),
                )
                .child(
                    div()
                        .id("close-discard")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.discard_close_tab(tab_id, cx);
                        }))
                        .child(theme.button(
                            language.text(TextKey::DontSave),
                            ButtonStyle::Destructive,
                            None,
                        )),
                )
                .child(
                    div()
                        .id("close-cancel")
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_modal(cx)))
                        .child(theme.button(
                            language.text(TextKey::Cancel),
                            ButtonStyle::Ghost,
                            None,
                        )),
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
    let language = view.language();
    let title = tab_title(view, tab_id);
    let card = ui::popover::dialog_card(theme)
        .gap(px(12.0))
        .child(ui::popover::dialog_title(
            theme,
            language.text(TextKey::FileChangedOnDisk),
        ))
        .child(ui::popover::dialog_body(
            theme,
            language.external_change_message(&title),
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
                        .child(theme.button(
                            language.text(TextKey::ReloadFromDisk),
                            ButtonStyle::Prominent,
                            None,
                        )),
                )
                .child(
                    div()
                        .id("conflict-keep-mine")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.keep_mine(tab_id, cx);
                        }))
                        .child(theme.button(
                            language.text(TextKey::KeepMyChanges),
                            ButtonStyle::Ghost,
                            None,
                        )),
                )
                .child(
                    div()
                        .id("conflict-cancel")
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_modal(cx)))
                        .child(theme.button(
                            language.text(TextKey::Cancel),
                            ButtonStyle::Ghost,
                            None,
                        )),
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
    let language = view.language();
    let title = tab_title(view, tab_id);
    let card = ui::popover::dialog_card(theme)
        .gap(px(12.0))
        .child(ui::popover::dialog_title(
            theme,
            language.text(TextKey::FileDeletedOnDisk),
        ))
        .child(ui::popover::dialog_body(
            theme,
            language.deleted_file_message(&title),
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
                        .child(theme.button(
                            language.text(TextKey::KeepOpen),
                            ButtonStyle::Prominent,
                            None,
                        )),
                )
                .child(
                    div()
                        .id("deleted-close")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.close_deleted_file(tab_id, cx);
                        }))
                        .child(theme.button(
                            language.text(TextKey::Close),
                            ButtonStyle::Destructive,
                            None,
                        )),
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
    language: Language,
    window: &mut Window,
    theme: &Theme,
    cx: &mut Context<Mieli>,
) -> bezel::gpui::AnyElement {
    let card = ui::popover::dialog_card(theme)
        .gap(px(12.0))
        .child(ui::popover::dialog_title(
            theme,
            language.text(TextKey::SaveFailed),
        ))
        .child(ui::popover::dialog_body(
            theme,
            match language {
                Language::English => {
                    "Mieli couldn't save every document. Quit anyway and lose unsaved changes?"
                }
                Language::Chinese => "Mieli 无法保存所有文档。仍然退出并丢弃未保存的更改吗？",
            },
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
                        .child(theme.button(
                            language.text(TextKey::QuitAnyway),
                            ButtonStyle::Destructive,
                            None,
                        )),
                )
                .child(
                    div()
                        .id("shutdown-cancel")
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_modal(cx)))
                        .child(theme.button(
                            language.text(TextKey::Cancel),
                            ButtonStyle::Ghost,
                            None,
                        )),
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
        .unwrap_or_else(|| view.language().text(TextKey::ThisFile).to_string())
}

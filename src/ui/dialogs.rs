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

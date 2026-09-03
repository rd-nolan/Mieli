use std::path::{Path, PathBuf};

use gpui::{App, KeyBinding, Menu, MenuItem};

use crate::i18n::{Language, TextKey};

gpui::actions!(
    mieli,
    [
        NewFile,
        OpenPath,
        Save,
        SaveAs,
        SaveAll,
        CloseTab,
        ToggleSidebar,
        NextTab,
        PreviousTab,
        RefreshTree,
        Quit,
        OpenWindow,
        OpenRecent1,
        OpenRecent2,
        OpenRecent3,
        OpenRecent4,
        OpenRecent5,
        OpenRecent6,
        OpenRecent7,
        OpenRecent8,
        OpenRecent9,
        OpenRecent10,
        OpenRecent11,
        OpenRecent12,
        OpenRecent13,
        OpenRecent14,
        OpenRecent15,
        OpenRecent16,
        OpenRecent17,
        OpenRecent18,
        OpenRecent19,
        OpenRecent20,
    ]
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BindingAction {
    NewFile,
    Save,
    SaveAs,
    CloseTab,
    ToggleSidebar,
    Quit,
    NextTab,
    PreviousTab,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BindingSpec {
    chord: &'static str,
    action: BindingAction,
}

impl BindingSpec {
    const fn new(chord: &'static str, action: BindingAction) -> Self {
        Self { chord, action }
    }

    fn into_key_binding(self) -> KeyBinding {
        match self.action {
            BindingAction::NewFile => KeyBinding::new(self.chord, NewFile, None),
            BindingAction::Save => KeyBinding::new(self.chord, Save, None),
            BindingAction::SaveAs => KeyBinding::new(self.chord, SaveAs, None),
            BindingAction::CloseTab => KeyBinding::new(self.chord, CloseTab, None),
            BindingAction::ToggleSidebar => KeyBinding::new(self.chord, ToggleSidebar, None),
            BindingAction::Quit => KeyBinding::new(self.chord, Quit, None),
            BindingAction::NextTab => KeyBinding::new(self.chord, NextTab, None),
            BindingAction::PreviousTab => KeyBinding::new(self.chord, PreviousTab, None),
        }
    }
}

fn binding_specs(command: &str) -> Vec<BindingSpec> {
    vec![
        BindingSpec::new(
            if command == "cmd" { "cmd-n" } else { "ctrl-n" },
            BindingAction::NewFile,
        ),
        BindingSpec::new(
            if command == "cmd" { "cmd-s" } else { "ctrl-s" },
            BindingAction::Save,
        ),
        BindingSpec::new(
            if command == "cmd" {
                "cmd-shift-s"
            } else {
                "ctrl-shift-s"
            },
            BindingAction::SaveAs,
        ),
        BindingSpec::new(
            if command == "cmd" { "cmd-w" } else { "ctrl-w" },
            BindingAction::CloseTab,
        ),
        BindingSpec::new(
            if command == "cmd" {
                "cmd-shift-l"
            } else {
                "ctrl-shift-l"
            },
            BindingAction::ToggleSidebar,
        ),
        BindingSpec::new(
            if command == "cmd" { "cmd-q" } else { "ctrl-q" },
            BindingAction::Quit,
        ),
        BindingSpec::new("ctrl-tab", BindingAction::NextTab),
        BindingSpec::new("ctrl-shift-tab", BindingAction::PreviousTab),
    ]
}

pub(crate) trait RecentAction {
    const POSITION: usize;
}

pub(crate) const fn recent_position<A: RecentAction>() -> usize {
    A::POSITION
}

macro_rules! recent_action_mappings {
    ($(($action:ty, $position:literal)),+ $(,)?) => {
        $(
            impl RecentAction for $action {
                const POSITION: usize = $position;
            }
        )+

        fn recent_menu_item(index: usize, label: String) -> Option<MenuItem> {
            match index {
                $(
                    $position => Some(MenuItem::action(label, <$action>::default())),
                )+
                _ => None,
            }
        }
    };
}

recent_action_mappings!(
    (OpenRecent1, 0),
    (OpenRecent2, 1),
    (OpenRecent3, 2),
    (OpenRecent4, 3),
    (OpenRecent5, 4),
    (OpenRecent6, 5),
    (OpenRecent7, 6),
    (OpenRecent8, 7),
    (OpenRecent9, 8),
    (OpenRecent10, 9),
    (OpenRecent11, 10),
    (OpenRecent12, 11),
    (OpenRecent13, 12),
    (OpenRecent14, 13),
    (OpenRecent15, 14),
    (OpenRecent16, 15),
    (OpenRecent17, 16),
    (OpenRecent18, 17),
    (OpenRecent19, 18),
    (OpenRecent20, 19),
);

pub fn install(cx: &mut App) {
    #[cfg(target_os = "macos")]
    let command = "cmd";
    #[cfg(not(target_os = "macos"))]
    let command = "ctrl";

    cx.bind_keys(
        binding_specs(command)
            .into_iter()
            .map(BindingSpec::into_key_binding),
    );
    set_file_menu(cx, &[], Language::current());
}

pub(crate) fn set_file_menu(cx: &mut App, recent_paths: &[PathBuf], language: Language) {
    let recent_items = recent_paths
        .iter()
        .take(20)
        .enumerate()
        .filter_map(|(index, path)| recent_menu_item(index, recent_label(path)))
        .collect::<Vec<_>>();
    let recent_is_empty = recent_items.is_empty();

    cx.set_menus([
        Menu::new(language.text(TextKey::FileMenu)).items([
            MenuItem::action(language.text(TextKey::NewFile), NewFile),
            MenuItem::action(language.text(TextKey::Open), OpenPath),
            MenuItem::submenu(Menu::new(language.text(TextKey::OpenRecent)).items(recent_items))
                .disabled(recent_is_empty),
            MenuItem::action(language.text(TextKey::RefreshFiles), RefreshTree),
            MenuItem::separator(),
            MenuItem::action(language.text(TextKey::Save), Save),
            MenuItem::action(language.text(TextKey::SaveAs), SaveAs),
            MenuItem::action(language.text(TextKey::SaveAll), SaveAll),
            MenuItem::separator(),
            MenuItem::action(language.text(TextKey::CloseTab), CloseTab),
            MenuItem::separator(),
            MenuItem::action(language.text(TextKey::Quit), Quit),
        ]),
        Menu::new(language.text(TextKey::WindowMenu)).items([MenuItem::action(
            language.text(TextKey::ReopenWindow),
            OpenWindow,
        )]),
    ]);
}

fn recent_label(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use gpui::Action as _;

    use super::*;

    #[test]
    fn unit_actions_use_the_mieli_namespace() {
        let names = [
            NewFile.name(),
            OpenPath.name(),
            Save.name(),
            SaveAs.name(),
            SaveAll.name(),
            CloseTab.name(),
            ToggleSidebar.name(),
            NextTab.name(),
            PreviousTab.name(),
            RefreshTree.name(),
            Quit.name(),
            OpenWindow.name(),
            OpenRecent1.name(),
            OpenRecent2.name(),
            OpenRecent3.name(),
            OpenRecent4.name(),
            OpenRecent5.name(),
            OpenRecent6.name(),
            OpenRecent7.name(),
            OpenRecent8.name(),
            OpenRecent9.name(),
            OpenRecent10.name(),
            OpenRecent11.name(),
            OpenRecent12.name(),
            OpenRecent13.name(),
            OpenRecent14.name(),
            OpenRecent15.name(),
            OpenRecent16.name(),
            OpenRecent17.name(),
            OpenRecent18.name(),
            OpenRecent19.name(),
            OpenRecent20.name(),
        ];

        assert_eq!(
            names,
            [
                "mieli::NewFile",
                "mieli::OpenPath",
                "mieli::Save",
                "mieli::SaveAs",
                "mieli::SaveAll",
                "mieli::CloseTab",
                "mieli::ToggleSidebar",
                "mieli::NextTab",
                "mieli::PreviousTab",
                "mieli::RefreshTree",
                "mieli::Quit",
                "mieli::OpenWindow",
                "mieli::OpenRecent1",
                "mieli::OpenRecent2",
                "mieli::OpenRecent3",
                "mieli::OpenRecent4",
                "mieli::OpenRecent5",
                "mieli::OpenRecent6",
                "mieli::OpenRecent7",
                "mieli::OpenRecent8",
                "mieli::OpenRecent9",
                "mieli::OpenRecent10",
                "mieli::OpenRecent11",
                "mieli::OpenRecent12",
                "mieli::OpenRecent13",
                "mieli::OpenRecent14",
                "mieli::OpenRecent15",
                "mieli::OpenRecent16",
                "mieli::OpenRecent17",
                "mieli::OpenRecent18",
                "mieli::OpenRecent19",
                "mieli::OpenRecent20",
            ]
        );
    }

    #[test]
    fn window_menu_exposes_a_reopen_action() {
        let source = include_str!("actions.rs");
        let window_menu = ["Window", "Menu"].concat();
        let reopen_window = ["Reopen", "Window"].concat();
        let open_window = ["Open", "Window"].concat();

        assert!(source.contains(&window_menu));
        assert!(source.contains(&reopen_window));
        assert!(source.contains(&open_window));
    }

    #[test]
    fn platform_binding_tables_use_native_commands_and_cross_platform_tab_navigation() {
        assert_eq!(
            binding_specs("cmd"),
            vec![
                BindingSpec::new("cmd-n", BindingAction::NewFile),
                BindingSpec::new("cmd-s", BindingAction::Save),
                BindingSpec::new("cmd-shift-s", BindingAction::SaveAs),
                BindingSpec::new("cmd-w", BindingAction::CloseTab),
                BindingSpec::new("cmd-shift-l", BindingAction::ToggleSidebar),
                BindingSpec::new("cmd-q", BindingAction::Quit),
                BindingSpec::new("ctrl-tab", BindingAction::NextTab),
                BindingSpec::new("ctrl-shift-tab", BindingAction::PreviousTab),
            ]
        );
        assert_eq!(
            binding_specs("ctrl"),
            vec![
                BindingSpec::new("ctrl-n", BindingAction::NewFile),
                BindingSpec::new("ctrl-s", BindingAction::Save),
                BindingSpec::new("ctrl-shift-s", BindingAction::SaveAs),
                BindingSpec::new("ctrl-w", BindingAction::CloseTab),
                BindingSpec::new("ctrl-shift-l", BindingAction::ToggleSidebar),
                BindingSpec::new("ctrl-q", BindingAction::Quit),
                BindingSpec::new("ctrl-tab", BindingAction::NextTab),
                BindingSpec::new("ctrl-shift-tab", BindingAction::PreviousTab),
            ]
        );
    }

    #[test]
    fn recent_actions_map_to_all_twenty_zero_based_positions() {
        let positions = [
            recent_position::<OpenRecent1>(),
            recent_position::<OpenRecent2>(),
            recent_position::<OpenRecent3>(),
            recent_position::<OpenRecent4>(),
            recent_position::<OpenRecent5>(),
            recent_position::<OpenRecent6>(),
            recent_position::<OpenRecent7>(),
            recent_position::<OpenRecent8>(),
            recent_position::<OpenRecent9>(),
            recent_position::<OpenRecent10>(),
            recent_position::<OpenRecent11>(),
            recent_position::<OpenRecent12>(),
            recent_position::<OpenRecent13>(),
            recent_position::<OpenRecent14>(),
            recent_position::<OpenRecent15>(),
            recent_position::<OpenRecent16>(),
            recent_position::<OpenRecent17>(),
            recent_position::<OpenRecent18>(),
            recent_position::<OpenRecent19>(),
            recent_position::<OpenRecent20>(),
        ];

        assert_eq!(positions, core::array::from_fn(|index| index));
    }
}

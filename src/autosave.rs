use std::path::{Path, PathBuf};

use crate::state::TabId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutosaveKey {
    pub tab_id: TabId,
    pub generation: u64,
    pub path: PathBuf,
}

pub fn autosave_is_current(
    key: &AutosaveKey,
    tab_id: TabId,
    generation: u64,
    path: &Path,
    dirty: bool,
) -> bool {
    key.tab_id == tab_id && key.generation == generation && key.path == path && dirty
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::state::TabId;

    use super::{AutosaveKey, autosave_is_current};

    #[test]
    fn stale_generation_cannot_save_a_newer_edit() {
        let key = AutosaveKey {
            tab_id: TabId(7),
            generation: 2,
            path: path("A.md"),
        };

        assert!(autosave_is_current(
            &key,
            TabId(7),
            2,
            Path::new("A.md"),
            true
        ));
        assert!(!autosave_is_current(
            &key,
            TabId(7),
            1,
            Path::new("A.md"),
            true
        ));
        assert!(!autosave_is_current(
            &key,
            TabId(7),
            2,
            Path::new("B.md"),
            true
        ));
        assert!(!autosave_is_current(
            &key,
            TabId(7),
            2,
            Path::new("A.md"),
            false,
        ));
    }

    #[test]
    fn autosave_key_rejects_stale_tab_identity() {
        let key = AutosaveKey {
            tab_id: TabId(7),
            generation: 2,
            path: path("A.md"),
        };

        assert!(!autosave_is_current(
            &key,
            TabId(8),
            2,
            Path::new("A.md"),
            true
        ));
    }

    fn path(value: &str) -> PathBuf {
        PathBuf::from(value)
    }
}

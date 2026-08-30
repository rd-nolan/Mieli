use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::state::FileTreeNode;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeRow {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    pub selected: bool,
}

pub fn visible_rows(nodes: &[FileTreeNode], active_path: Option<&Path>) -> Vec<TreeRow> {
    fn append_rows(
        rows: &mut Vec<TreeRow>,
        nodes: &[FileTreeNode],
        depth: usize,
        active_path: Option<&Path>,
    ) {
        for node in nodes {
            rows.push(TreeRow {
                path: node.path.clone(),
                name: node.name.clone(),
                depth,
                is_dir: node.is_dir,
                expanded: node.expanded,
                selected: !node.is_dir && active_path == Some(node.path.as_path()),
            });
            if node.is_dir && node.expanded {
                append_rows(rows, &node.children, depth + 1, active_path);
            }
        }
    }

    let mut rows = Vec::new();
    append_rows(&mut rows, nodes, 0, active_path);
    rows
}

pub fn preserve_expansion(previous: &[FileTreeNode], refreshed: &mut [FileTreeNode]) {
    fn collect(nodes: &[FileTreeNode], expansion: &mut HashMap<PathBuf, bool>) {
        for node in nodes {
            if node.is_dir {
                expansion.insert(node.path.clone(), node.expanded);
                collect(&node.children, expansion);
            }
        }
    }

    fn apply(nodes: &mut [FileTreeNode], expansion: &HashMap<PathBuf, bool>) {
        for node in nodes {
            if node.is_dir {
                if let Some(expanded) = expansion.get(&node.path) {
                    node.expanded = *expanded;
                }
                apply(&mut node.children, expansion);
            }
        }
    }

    let mut expansion = HashMap::new();
    collect(previous, &mut expansion);
    apply(refreshed, &expansion);
}

pub fn toggle_expansion(nodes: &mut [FileTreeNode], path: &Path) -> bool {
    for node in nodes {
        if node.is_dir && node.path == path {
            node.expanded = !node.expanded;
            return true;
        }
        if toggle_expansion(&mut node.children, path) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::state::FileTreeNode;

    use super::{preserve_expansion, toggle_expansion, visible_rows};

    #[test]
    fn active_path_is_selected_and_collapsed_children_are_hidden() {
        let tree = sample_tree(true);
        let active_path = path("docs/api.md");

        assert!(
            visible_rows(&tree, Some(&active_path))
                .iter()
                .any(|row| row.path == path("docs/api.md") && row.selected)
        );
        assert!(
            !visible_rows(&sample_tree(false), Some(&active_path))
                .iter()
                .any(|row| row.path == path("docs/api.md"))
        );
    }

    #[test]
    fn refreshed_tree_preserves_directory_expansion_by_path() {
        let previous = sample_tree(false);
        let mut refreshed = sample_tree(true);

        preserve_expansion(&previous, &mut refreshed);

        assert!(!refreshed[0].expanded);
    }

    #[test]
    fn toggling_expansion_updates_only_the_matching_directory() {
        let mut tree = sample_tree(false);

        assert!(toggle_expansion(&mut tree, &path("docs")));
        assert!(tree[0].expanded);
        assert!(tree[1].expanded);
    }

    fn sample_tree(docs_expanded: bool) -> Vec<FileTreeNode> {
        vec![
            FileTreeNode {
                path: path("docs"),
                name: String::from("docs/"),
                is_dir: true,
                expanded: docs_expanded,
                children: vec![FileTreeNode {
                    path: path("docs/api.md"),
                    name: String::from("api.md"),
                    is_dir: false,
                    expanded: true,
                    children: Vec::new(),
                }],
            },
            FileTreeNode {
                path: path("README.md"),
                name: String::from("README.md"),
                is_dir: false,
                expanded: true,
                children: Vec::new(),
            },
        ]
    }

    fn path(value: &str) -> PathBuf {
        PathBuf::from(value)
    }
}

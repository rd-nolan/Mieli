use std::{
    cmp::Ordering,
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

    let expansion = capture_expansion(previous);
    apply(refreshed, &expansion);
}

pub fn capture_expansion(nodes: &[FileTreeNode]) -> HashMap<PathBuf, bool> {
    fn collect(nodes: &[FileTreeNode], expansion: &mut HashMap<PathBuf, bool>) {
        for node in nodes {
            if node.is_dir {
                expansion.insert(node.path.clone(), node.expanded);
                collect(&node.children, expansion);
            }
        }
    }

    let mut expansion = HashMap::new();
    collect(nodes, &mut expansion);
    expansion
}

pub fn insert_markdown_paths(
    nodes: &mut Vec<FileTreeNode>,
    root: &Path,
    paths: impl IntoIterator<Item = PathBuf>,
    expansion: &HashMap<PathBuf, bool>,
) {
    for path in paths {
        if !crate::file::io::is_markdown_file(&path) {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let components = relative
            .components()
            .map(|component| component.as_os_str().to_os_string())
            .collect::<Vec<_>>();
        if components.is_empty() {
            continue;
        }
        insert_components(nodes, root, &components, expansion);
    }
    sort_nodes(nodes);
}

fn insert_components(
    nodes: &mut Vec<FileTreeNode>,
    parent: &Path,
    components: &[std::ffi::OsString],
    expansion: &HashMap<PathBuf, bool>,
) {
    let path = parent.join(&components[0]);
    if components.len() == 1 {
        if nodes.iter().any(|node| node.path == path) {
            return;
        }
        nodes.push(FileTreeNode {
            name: components[0].to_string_lossy().into_owned(),
            path,
            is_dir: false,
            expanded: true,
            children: Vec::new(),
        });
        return;
    }

    if let Some(node) = nodes
        .iter_mut()
        .find(|node| node.is_dir && node.path == path)
    {
        insert_components(&mut node.children, &path, &components[1..], expansion);
        return;
    }

    let mut directory = FileTreeNode {
        name: format!("{}/", components[0].to_string_lossy()),
        path: path.clone(),
        is_dir: true,
        expanded: expansion.get(&path).copied().unwrap_or(true),
        children: Vec::new(),
    };
    insert_components(&mut directory.children, &path, &components[1..], expansion);
    nodes.push(directory);
}

fn sort_nodes(nodes: &mut [FileTreeNode]) {
    nodes.sort_by(compare_nodes);
    for node in nodes {
        if node.is_dir {
            sort_nodes(&mut node.children);
        }
    }
}

fn compare_nodes(left: &FileTreeNode, right: &FileTreeNode) -> Ordering {
    let left_rank = if left.is_dir { 0 } else { 1 };
    let right_rank = if right.is_dir { 0 } else { 1 };
    left_rank
        .cmp(&right_rank)
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        .then_with(|| left.path.cmp(&right.path))
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

    use super::{
        capture_expansion, insert_markdown_paths, preserve_expansion, toggle_expansion,
        visible_rows,
    };

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

    #[test]
    fn inserting_markdown_paths_builds_sorted_unique_ancestors_and_preserves_expansion() {
        let root = path("/workspace");
        let previous = vec![FileTreeNode {
            path: path("/workspace/docs"),
            name: String::from("docs/"),
            is_dir: true,
            expanded: false,
            children: Vec::new(),
        }];
        let expansion = capture_expansion(&previous);
        let mut tree = Vec::new();

        insert_markdown_paths(
            &mut tree,
            &root,
            [
                path("/workspace/z.md"),
                path("/workspace/docs/b.md"),
                path("/workspace/docs/a.md"),
                path("/workspace/docs/a.md"),
                path("/workspace/Alpha/reference.md"),
                path("/workspace/assets/image.png"),
            ],
            &expansion,
        );

        assert_eq!(
            tree.iter()
                .map(|node| node.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Alpha/", "docs/", "z.md"]
        );
        assert!(!tree[1].expanded);
        assert_eq!(
            tree[1]
                .children
                .iter()
                .map(|node| node.name.as_str())
                .collect::<Vec<_>>(),
            vec!["a.md", "b.md"]
        );
        assert_eq!(tree[1].children.len(), 2);
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

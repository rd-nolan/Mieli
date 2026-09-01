use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::state::{FileTreeNode, compare_file_tree_nodes};

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
    let mut rows = Vec::new();
    let mut pending = nodes.iter().rev().map(|node| (node, 0)).collect::<Vec<_>>();
    while let Some((node, depth)) = pending.pop() {
        rows.push(TreeRow {
            path: node.path.clone(),
            name: node.name.clone(),
            depth,
            is_dir: node.is_dir,
            expanded: node.expanded,
            selected: !node.is_dir && active_path == Some(node.path.as_path()),
        });
        if node.is_dir && node.expanded {
            pending.extend(node.children.iter().rev().map(|child| (child, depth + 1)));
        }
    }
    rows
}

pub fn preserve_expansion(previous: &[FileTreeNode], refreshed: &mut [FileTreeNode]) {
    let expansion = capture_expansion(previous);
    let mut pending = vec![refreshed];
    while let Some(nodes) = pending.pop() {
        for node in nodes {
            if !node.is_dir {
                continue;
            }
            if let Some(expanded) = expansion.get(&node.path) {
                node.expanded = *expanded;
            }
            pending.push(node.children.as_mut_slice());
        }
    }
}

pub fn capture_expansion(nodes: &[FileTreeNode]) -> HashMap<PathBuf, bool> {
    let mut expansion = HashMap::new();
    let mut pending = nodes.iter().collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        if node.is_dir {
            expansion.insert(node.path.clone(), node.expanded);
            pending.extend(node.children.iter());
        }
    }
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
    let mut current_nodes = nodes;
    let mut parent = parent.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let path = parent.join(component);
        if index + 1 == components.len() {
            if current_nodes.iter().all(|node| node.path != path) {
                current_nodes.push(FileTreeNode {
                    name: component.to_string_lossy().into_owned(),
                    path,
                    is_dir: false,
                    expanded: true,
                    children: Vec::new(),
                });
            }
            return;
        }

        let directory_index = current_nodes
            .iter()
            .position(|node| node.is_dir && node.path == path);
        let directory_index = match directory_index {
            Some(index) => index,
            None => {
                current_nodes.push(FileTreeNode {
                    name: format!("{}/", component.to_string_lossy()),
                    path: path.clone(),
                    is_dir: true,
                    expanded: expansion.get(&path).copied().unwrap_or(true),
                    children: Vec::new(),
                });
                current_nodes.len() - 1
            }
        };
        parent = path;
        current_nodes = &mut current_nodes[directory_index].children;
    }
}

fn sort_nodes(nodes: &mut [FileTreeNode]) {
    let mut pending = vec![nodes];
    while let Some(nodes) = pending.pop() {
        nodes.sort_by(compare_file_tree_nodes);
        for node in nodes.iter_mut() {
            if node.is_dir {
                pending.push(node.children.as_mut_slice());
            }
        }
    }
}

pub fn toggle_expansion(nodes: &mut [FileTreeNode], path: &Path) -> bool {
    let mut pending = vec![nodes];
    while let Some(nodes) = pending.pop() {
        for node in nodes {
            if node.is_dir && node.path == path {
                node.expanded = !node.expanded;
                return true;
            }
            if node.is_dir {
                pending.push(node.children.as_mut_slice());
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::PathBuf};

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

    #[test]
    fn deeply_nested_tree_operations_remain_iterative() {
        let root = path("/workspace");
        let mut file = root.clone();
        let mut collapsed = root.clone();
        for index in 0..1024 {
            let component = format!("level-{index}");
            file.push(&component);
            if index <= 512 {
                collapsed.push(&component);
            }
        }
        file.push("deep.md");

        let mut tree = Vec::new();
        insert_markdown_paths(&mut tree, &root, [file.clone()], &HashMap::new());

        assert_eq!(visible_rows(&tree, None).len(), 1025);
        assert_eq!(capture_expansion(&tree).len(), 1024);
        assert!(toggle_expansion(&mut tree, &collapsed));

        let mut refreshed = Vec::new();
        insert_markdown_paths(&mut refreshed, &root, [file], &HashMap::new());
        preserve_expansion(&tree, &mut refreshed);
        assert_eq!(capture_expansion(&refreshed).get(&collapsed), Some(&false));
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

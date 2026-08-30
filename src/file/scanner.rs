use std::{cmp::Ordering, fs, path::Path};

use crate::{
    file::{FileError, io::is_markdown_file},
    state::FileTreeNode,
};

pub fn scan_markdown_tree(root: &Path) -> Result<Vec<FileTreeNode>, FileError> {
    let mut nodes = Vec::new();

    for entry in fs::read_dir(root).map_err(|error| FileError::from_io(root, "scan", error))? {
        let entry = entry.map_err(|error| FileError::from_io(root, "scan", error))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| FileError::from_io(&path, "scan", error))?;

        if file_type.is_dir() {
            let children = scan_markdown_tree(&path)?;
            if !children.is_empty() {
                nodes.push(FileTreeNode {
                    name: format!("{}/", entry.file_name().to_string_lossy()),
                    path,
                    is_dir: true,
                    expanded: true,
                    children,
                });
            }
        } else if file_type.is_file() && is_markdown_file(&path) {
            nodes.push(FileTreeNode {
                name: entry.file_name().to_string_lossy().into_owned(),
                path,
                is_dir: false,
                expanded: true,
                children: Vec::new(),
            });
        }
    }

    sort_file_tree_nodes(&mut nodes);

    Ok(nodes)
}

fn sort_file_tree_nodes(nodes: &mut [FileTreeNode]) {
    nodes.sort_by(compare_file_tree_nodes);
}

fn compare_file_tree_nodes(left: &FileTreeNode, right: &FileTreeNode) -> Ordering {
    let left_rank = if left.is_dir { 0 } else { 1 };
    let right_rank = if right.is_dir { 0 } else { 1 };
    left_rank
        .cmp(&right_rank)
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        .then_with(|| left.path.cmp(&right.path))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::state::FileTreeNode;

    use super::{scan_markdown_tree, sort_file_tree_nodes};

    #[test]
    fn scanner_keeps_only_directories_with_markdown_descendants() {
        let temp = TempDir::root_with(&[
            "README.md",
            "test.txt",
            "docs/api.md",
            "docs/image.png",
            "assets/image.png",
        ]);

        let tree = scan_markdown_tree(temp.path()).unwrap();

        assert_eq!(tree_names(&tree), vec!["docs/", "README.md"]);
    }

    #[test]
    fn scanner_sorts_directories_before_files_and_uses_case_insensitive_names() {
        let temp =
            TempDir::root_with(&["zeta.md", "Beta.md", "guide/intro.md", "Alpha/reference.md"]);

        let tree = scan_markdown_tree(temp.path()).unwrap();

        assert_eq!(
            tree_names(&tree),
            vec!["Alpha/", "guide/", "Beta.md", "zeta.md"]
        );
        assert!(tree.iter().all(|node| node.expanded));
    }

    #[test]
    fn scanner_uses_full_path_to_break_equal_lowercase_name_collisions() {
        let mut nodes = vec![
            test_node("/tmp/alpha", "alpha/", true),
            test_node("/tmp/ALPHA", "ALPHA/", true),
            test_node("/tmp/beta.MD", "beta.MD", false),
            test_node("/tmp/Beta.md", "Beta.md", false),
        ];

        sort_file_tree_nodes(&mut nodes);

        assert_eq!(
            tree_names(&nodes),
            vec!["ALPHA/", "alpha/", "Beta.md", "beta.MD"]
        );
        assert_eq!(
            nodes
                .iter()
                .map(|node| node.path.clone())
                .collect::<Vec<_>>(),
            vec![
                PathBuf::from("/tmp/ALPHA"),
                PathBuf::from("/tmp/alpha"),
                PathBuf::from("/tmp/Beta.md"),
                PathBuf::from("/tmp/beta.MD"),
            ]
        );
    }

    fn tree_names(nodes: &[FileTreeNode]) -> Vec<String> {
        nodes.iter().map(|node| node.name.clone()).collect()
    }

    fn test_node(path: &str, name: &str, is_dir: bool) -> FileTreeNode {
        FileTreeNode {
            path: PathBuf::from(path),
            name: name.to_string(),
            is_dir,
            expanded: true,
            children: Vec::new(),
        }
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);

            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("mieli-task-2-{unique}-{suffix}"));
            fs::create_dir_all(&path).unwrap();

            Self { path }
        }

        fn root_with(paths: &[&str]) -> Self {
            let temp = Self::new();
            let path = temp.path.clone();

            for relative in paths {
                let file_path = path.join(relative);
                if let Some(parent) = file_path.parent() {
                    fs::create_dir_all(parent).unwrap();
                }
                fs::write(&file_path, relative).unwrap();
            }

            temp
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

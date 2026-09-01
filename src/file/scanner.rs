use std::{
    collections::VecDeque,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering as AtomicOrdering},
};

use crate::{
    file::{FileError, io::is_markdown_file},
    state::{FileTreeNode, compare_file_tree_nodes},
};

pub fn scan_markdown_tree(root: &Path) -> Result<Vec<FileTreeNode>, FileError> {
    let cancel = AtomicBool::new(false);
    let mut paths = Vec::new();
    walk_markdown_paths(root, &cancel, &mut |path| paths.push(path))?;

    let mut nodes = Vec::new();
    for path in paths {
        insert_markdown_path(&mut nodes, root, path);
    }
    sort_file_tree_nodes(&mut nodes);

    Ok(nodes)
}

pub(crate) fn scan_markdown_tree_progressive(
    root: &Path,
    cancel: &AtomicBool,
    on_file: &mut impl FnMut(PathBuf),
) -> Result<(), FileError> {
    walk_markdown_paths(root, cancel, on_file)
}

fn walk_markdown_paths(
    root: &Path,
    cancel: &AtomicBool,
    on_file: &mut impl FnMut(PathBuf),
) -> Result<(), FileError> {
    let mut directories = VecDeque::from([root.to_path_buf()]);

    while let Some(directory) = directories.pop_front() {
        if cancel.load(AtomicOrdering::Relaxed) {
            return Ok(());
        }

        let mut entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if directory != root && is_transient_scan_error(&error) => continue,
            Err(error) => return Err(FileError::from_io(&directory, "scan", error)),
        };
        loop {
            if cancel.load(AtomicOrdering::Relaxed) {
                return Ok(());
            }

            let Some(entry) = entries.next() else {
                break;
            };
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if is_transient_scan_error(&error) => continue,
                Err(error) => return Err(FileError::from_io(&directory, "scan", error)),
            };
            let path = entry.path();
            if cancel.load(AtomicOrdering::Relaxed) {
                return Ok(());
            }

            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) if is_transient_scan_error(&error) => continue,
                Err(error) => return Err(FileError::from_io(&path, "scan", error)),
            };
            if file_type.is_dir() {
                directories.push_back(path);
            } else if file_type.is_file() && is_markdown_file(&path) {
                on_file(path);
            }
        }
    }

    Ok(())
}

fn insert_markdown_path(nodes: &mut Vec<FileTreeNode>, root: &Path, path: PathBuf) {
    let Ok(relative) = path.strip_prefix(root) else {
        return;
    };
    let components = relative
        .components()
        .map(|component| component.as_os_str().to_os_string())
        .collect::<Vec<_>>();
    if components.is_empty() {
        return;
    }

    let mut current_nodes = nodes;
    let mut parent = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let node_path = parent.join(component);
        if index + 1 == components.len() {
            if current_nodes.iter().all(|node| node.path != node_path) {
                current_nodes.push(FileTreeNode {
                    name: component.to_string_lossy().into_owned(),
                    path: node_path,
                    is_dir: false,
                    expanded: true,
                    children: Vec::new(),
                });
            }
            return;
        }

        let directory_index = current_nodes
            .iter()
            .position(|node| node.is_dir && node.path == node_path);
        let directory_index = match directory_index {
            Some(index) => index,
            None => {
                current_nodes.push(FileTreeNode {
                    name: format!("{}/", component.to_string_lossy()),
                    path: node_path.clone(),
                    is_dir: true,
                    expanded: true,
                    children: Vec::new(),
                });
                current_nodes.len() - 1
            }
        };
        parent = node_path;
        current_nodes = &mut current_nodes[directory_index].children;
    }
}

fn is_transient_scan_error(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
}

fn sort_file_tree_nodes(nodes: &mut [FileTreeNode]) {
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::state::FileTreeNode;

    use super::{scan_markdown_tree, scan_markdown_tree_progressive, sort_file_tree_nodes};

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

    #[test]
    fn progressive_scanner_reports_only_markdown_files() {
        let temp = TempDir::root_with(&[
            "README.md",
            "docs/api.md",
            "docs/image.png",
            "assets/image.png",
            "notes.txt",
        ]);
        let cancel = AtomicBool::new(false);
        let mut files = Vec::new();

        scan_markdown_tree_progressive(temp.path(), &cancel, &mut |path| files.push(path)).unwrap();

        files.sort();
        assert_eq!(
            files,
            vec![
                temp.path().join("README.md"),
                temp.path().join("docs/api.md")
            ]
        );
    }

    #[test]
    fn progressive_scanner_stops_after_cancellation() {
        let temp = TempDir::root_with(&["first.md", "second.md", "third.md"]);
        let cancel = AtomicBool::new(false);
        let mut files = Vec::new();

        scan_markdown_tree_progressive(temp.path(), &cancel, &mut |path| {
            files.push(path);
            cancel.store(true, Ordering::Relaxed);
        })
        .unwrap();

        assert_eq!(files.len(), 1);
    }

    #[test]
    fn progressive_scanner_handles_deep_directories_iteratively() {
        let temp = TempDir::new();
        let mut directory = temp.path().to_path_buf();
        for _ in 0..128 {
            directory.push("d");
            fs::create_dir(&directory).unwrap();
        }
        let file = directory.join("deep.md");
        fs::write(&file, "deep").unwrap();
        let cancel = AtomicBool::new(false);
        let mut files = Vec::new();

        scan_markdown_tree_progressive(temp.path(), &cancel, &mut |path| files.push(path)).unwrap();

        assert_eq!(files, vec![file]);
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

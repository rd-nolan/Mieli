use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
};

use directories::BaseDirs;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileSystemEvent {
    Changed(PathBuf),
    Created(PathBuf),
    Removed(PathBuf),
    Error {
        path: Option<PathBuf>,
        message: String,
    },
}

#[derive(Debug)]
pub enum WatchError {
    MissingParent {
        path: PathBuf,
    },
    HomeDirectoryRoot {
        path: PathBuf,
    },
    Io {
        path: PathBuf,
        operation: &'static str,
        kind: std::io::ErrorKind,
    },
    Notify {
        path: PathBuf,
        operation: &'static str,
        message: String,
    },
}

impl WatchError {
    fn from_io(path: &Path, operation: &'static str, error: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            operation,
            kind: error.kind(),
        }
    }

    fn from_notify(path: &Path, operation: &'static str, error: notify::Error) -> Self {
        Self::Notify {
            path: path.to_path_buf(),
            operation,
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for WatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingParent { path } => {
                write!(
                    f,
                    "Could not watch {}: parent directory not found.",
                    path.display()
                )
            }
            Self::HomeDirectoryRoot { path } => write!(
                f,
                "Could not watch {}: refusing to watch the home directory root.",
                path.display()
            ),
            Self::Io {
                path,
                operation,
                kind,
            } => write!(f, "Could not {operation} {}: {kind}.", path.display()),
            Self::Notify {
                path,
                operation,
                message,
            } => write!(f, "Could not {operation} {}: {message}.", path.display()),
        }
    }
}

impl std::error::Error for WatchError {}

pub struct FileWatcherService {
    watcher: RecommendedWatcher,
    receiver: mpsc::Receiver<FileSystemEvent>,
    watched: BTreeMap<PathBuf, RecursiveMode>,
}

impl FileWatcherService {
    pub fn new() -> Result<Self, WatchError> {
        let (sender, receiver) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |result| {
            for event in translate_notify_result(result) {
                let _ = sender.send(event);
            }
        })
        .map_err(|error| WatchError::from_notify(Path::new("."), "create watcher", error))?;

        Ok(Self {
            watcher,
            receiver,
            watched: BTreeMap::new(),
        })
    }

    pub fn watch_workspace(&mut self, root: &Path) -> Result<(), WatchError> {
        ensure_watched(
            &mut self.watched,
            &mut self.watcher,
            root,
            RecursiveMode::Recursive,
        )
    }

    pub fn watch_file_parent(&mut self, path: &Path) -> Result<(), WatchError> {
        let parent = file_parent_directory(path)?;
        ensure_watched(
            &mut self.watched,
            &mut self.watcher,
            parent,
            RecursiveMode::NonRecursive,
        )
    }

    pub fn drain(&self) -> Vec<FileSystemEvent> {
        self.receiver.try_iter().collect()
    }
}

pub fn translate_event(kind: EventKind, path: PathBuf) -> FileSystemEvent {
    match kind {
        EventKind::Create(_) => FileSystemEvent::Created(path),
        EventKind::Remove(_) => FileSystemEvent::Removed(path),
        EventKind::Modify(_) => FileSystemEvent::Changed(path),
        EventKind::Any | EventKind::Access(_) | EventKind::Other => unsupported_event(kind, path),
    }
}

fn translate_notify_result(result: notify::Result<Event>) -> Vec<FileSystemEvent> {
    match result {
        Ok(event) => event
            .paths
            .into_iter()
            .map(|path| translate_event(event.kind, path))
            .collect(),
        Err(error) => translate_notify_error(error),
    }
}

fn translate_notify_error(error: notify::Error) -> Vec<FileSystemEvent> {
    let message = error.to_string();
    if error.paths.is_empty() {
        vec![FileSystemEvent::Error {
            path: None,
            message,
        }]
    } else {
        error
            .paths
            .into_iter()
            .map(|path| FileSystemEvent::Error {
                path: Some(path),
                message: message.clone(),
            })
            .collect()
    }
}

fn ensure_watched<W: Watcher>(
    watched: &mut BTreeMap<PathBuf, RecursiveMode>,
    watcher: &mut W,
    path: &Path,
    recursive_mode: RecursiveMode,
) -> Result<(), WatchError> {
    let canonical = canonical_watch_directory(path)?;
    match watched.get(&canonical).copied() {
        Some(current_mode) if current_mode == recursive_mode => return Ok(()),
        Some(RecursiveMode::Recursive) => return Ok(()),
        Some(RecursiveMode::NonRecursive) if recursive_mode == RecursiveMode::Recursive => {
            watcher
                .unwatch(&canonical)
                .map_err(|error| WatchError::from_notify(&canonical, "unwatch", error))?;
        }
        Some(RecursiveMode::NonRecursive) => return Ok(()),
        None => {}
    }

    watcher
        .watch(&canonical, recursive_mode)
        .map_err(|error| WatchError::from_notify(&canonical, "watch", error))?;
    watched.insert(canonical, recursive_mode);
    Ok(())
}

fn unsupported_event(kind: EventKind, path: PathBuf) -> FileSystemEvent {
    FileSystemEvent::Error {
        path: Some(path),
        message: format!("Unsupported filesystem event kind: {kind:?}"),
    }
}

fn canonical_watch_directory(path: &Path) -> Result<PathBuf, WatchError> {
    let canonical =
        fs::canonicalize(path).map_err(|error| WatchError::from_io(path, "canonicalize", error))?;
    if is_home_directory_root(&canonical) {
        return Err(WatchError::HomeDirectoryRoot { path: canonical });
    }
    Ok(canonical)
}

fn file_parent_directory(path: &Path) -> Result<&Path, WatchError> {
    match path.parent() {
        Some(parent) if parent.as_os_str().is_empty() => Ok(Path::new(".")),
        Some(parent) => Ok(parent),
        None => Err(WatchError::MissingParent {
            path: path.to_path_buf(),
        }),
    }
}

fn is_home_directory_root(path: &Path) -> bool {
    BaseDirs::new()
        .and_then(|base_dirs| fs::canonicalize(base_dirs.home_dir()).ok())
        .is_some_and(|home| home == path)
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use notify::{
        Config, EventKind, RecursiveMode, Watcher,
        event::{CreateKind, ModifyKind, RemoveKind},
    };

    use super::{
        FileSystemEvent, WatchError, canonical_watch_directory, ensure_watched,
        file_parent_directory, translate_event, translate_notify_error,
    };

    #[test]
    fn notify_events_are_reduced_to_changed_created_removed_or_error() {
        assert_eq!(
            translate_event(EventKind::Modify(ModifyKind::Any), path("A.md")),
            FileSystemEvent::Changed(path("A.md"))
        );
        assert_eq!(
            translate_event(EventKind::Create(CreateKind::Any), path("A.md")),
            FileSystemEvent::Created(path("A.md"))
        );
        assert_eq!(
            translate_event(EventKind::Remove(RemoveKind::Any), path("A.md")),
            FileSystemEvent::Removed(path("A.md"))
        );
    }

    #[test]
    fn watcher_paths_are_canonicalized_and_deduplicated() {
        let temp = TempDir::new();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let requested = temp.path().join("workspace").join("..").join("workspace");
        let canonical = fs::canonicalize(&workspace).unwrap();
        let calls = RefCell::new(Vec::new());
        let mut watcher = FakeWatcher::new(&calls);
        let mut watched = std::collections::BTreeMap::new();

        ensure_watched(
            &mut watched,
            &mut watcher,
            &workspace,
            RecursiveMode::Recursive,
        )
        .unwrap();
        ensure_watched(
            &mut watched,
            &mut watcher,
            &requested,
            RecursiveMode::Recursive,
        )
        .unwrap();

        assert_eq!(
            watched.into_iter().collect::<Vec<_>>(),
            vec![(canonical.clone(), RecursiveMode::Recursive)]
        );
        assert_eq!(
            calls.into_inner(),
            vec![WatchOperation::Watch(canonical, RecursiveMode::Recursive)]
        );
    }

    #[test]
    fn recursive_watch_upgrades_an_existing_non_recursive_watch() {
        let temp = TempDir::new();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let canonical = fs::canonicalize(&workspace).unwrap();
        let calls = RefCell::new(Vec::new());
        let mut watcher = FakeWatcher::new(&calls);
        let mut watched = std::collections::BTreeMap::new();

        ensure_watched(
            &mut watched,
            &mut watcher,
            &workspace,
            RecursiveMode::NonRecursive,
        )
        .unwrap();
        ensure_watched(
            &mut watched,
            &mut watcher,
            &workspace,
            RecursiveMode::Recursive,
        )
        .unwrap();

        assert_eq!(
            calls.into_inner(),
            vec![
                WatchOperation::Watch(canonical.clone(), RecursiveMode::NonRecursive),
                WatchOperation::Unwatch(canonical.clone()),
                WatchOperation::Watch(canonical, RecursiveMode::Recursive),
            ]
        );
    }

    #[test]
    fn file_parent_watches_are_non_recursive_and_reject_home_root() {
        let relative_file = PathBuf::from("note.md");
        assert_eq!(
            file_parent_directory(&relative_file).unwrap(),
            Path::new(".")
        );

        let temp = TempDir::new();
        let parent = temp.path().join("folder");
        fs::create_dir_all(&parent).unwrap();
        let file_path = parent.join("note.md");
        let canonical_parent = fs::canonicalize(&parent).unwrap();
        let calls = RefCell::new(Vec::new());
        let mut watcher = FakeWatcher::new(&calls);
        let mut watched = std::collections::BTreeMap::new();

        ensure_watched(
            &mut watched,
            &mut watcher,
            file_parent_directory(&file_path).unwrap(),
            RecursiveMode::NonRecursive,
        )
        .unwrap();

        assert_eq!(
            calls.into_inner(),
            vec![WatchOperation::Watch(
                canonical_parent,
                RecursiveMode::NonRecursive
            )]
        );

        let home = directories::BaseDirs::new()
            .unwrap()
            .home_dir()
            .to_path_buf();
        let error = canonical_watch_directory(&home).unwrap_err();
        assert!(
            matches!(error, WatchError::HomeDirectoryRoot { path } if path == fs::canonicalize(home).unwrap())
        );
    }

    #[test]
    fn notify_errors_keep_paths_and_message_for_callbacks() {
        let error = notify::Error::generic("watch failed").add_path(path("A.md"));

        assert_eq!(
            translate_notify_error(error),
            vec![FileSystemEvent::Error {
                path: Some(path("A.md")),
                message: "watch failed about [\"A.md\"]".to_string(),
            }]
        );
    }

    #[test]
    fn unknown_notify_kinds_are_reduced_to_error_events() {
        assert_eq!(
            translate_event(EventKind::Any, path("A.md")),
            FileSystemEvent::Error {
                path: Some(path("A.md")),
                message: "Unsupported filesystem event kind: Any".to_string(),
            }
        );
        assert_eq!(
            translate_event(EventKind::Other, path("B.md")),
            FileSystemEvent::Error {
                path: Some(path("B.md")),
                message: "Unsupported filesystem event kind: Other".to_string(),
            }
        );
        assert_eq!(
            translate_event(
                EventKind::Access(notify::event::AccessKind::Any),
                path("C.md")
            ),
            FileSystemEvent::Error {
                path: Some(path("C.md")),
                message: "Unsupported filesystem event kind: Access(Any)".to_string(),
            }
        );
    }

    fn path(value: &str) -> PathBuf {
        PathBuf::from(value)
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum WatchOperation {
        Watch(PathBuf, RecursiveMode),
        Unwatch(PathBuf),
    }

    struct FakeWatcher<'a> {
        calls: &'a RefCell<Vec<WatchOperation>>,
    }

    impl<'a> FakeWatcher<'a> {
        fn new(calls: &'a RefCell<Vec<WatchOperation>>) -> Self {
            Self { calls }
        }
    }

    impl Watcher for FakeWatcher<'_> {
        fn new<F: notify::EventHandler>(_: F, _: Config) -> notify::Result<Self>
        where
            Self: Sized,
        {
            unimplemented!("tests construct FakeWatcher directly")
        }

        fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> notify::Result<()> {
            self.calls
                .borrow_mut()
                .push(WatchOperation::Watch(path.to_path_buf(), recursive_mode));
            Ok(())
        }

        fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
            self.calls
                .borrow_mut()
                .push(WatchOperation::Unwatch(path.to_path_buf()));
            Ok(())
        }

        fn kind() -> notify::WatcherKind
        where
            Self: Sized,
        {
            notify::WatcherKind::PollWatcher
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
            let path = std::env::temp_dir().join(format!("mieli-task-4-{unique}-{suffix}"));
            fs::create_dir_all(&path).unwrap();

            Self { path }
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

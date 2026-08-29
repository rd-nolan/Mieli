use std::{
    fs,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::file::io::canonicalize_path;

use super::recent_files_path;

const DEFAULT_CAPACITY: usize = 20;

#[derive(Serialize, Deserialize, Default)]
struct RecentConfig {
    recent_files: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentFiles {
    paths: Vec<PathBuf>,
    config_path: Option<PathBuf>,
    capacity: usize,
    canonicalize_on_record: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecentFilesError {
    ConfigUnavailable,
    Read { path: PathBuf },
    Parse { path: PathBuf },
    CreateDir { path: PathBuf },
    Write { path: PathBuf },
    Canonicalize { path: PathBuf },
    Serialize { path: PathBuf },
}

impl RecentFiles {
    pub fn load() -> (Self, Option<RecentFilesError>) {
        let Some(config_path) = recent_files_path(ProjectDirs::from("com", "Mieli", "Mieli"))
        else {
            return (
                Self::in_memory_with_paths(DEFAULT_CAPACITY, Vec::new()),
                Some(RecentFilesError::ConfigUnavailable),
            );
        };

        Self::load_from_path_with_notification(config_path, DEFAULT_CAPACITY)
    }

    pub fn load_from_path(config_path: PathBuf, capacity: usize) -> Result<Self, RecentFilesError> {
        let config = read_config(&config_path)?;
        Ok(Self::new(
            config.recent_files,
            Some(config_path),
            capacity,
            true,
        ))
    }

    pub fn load_from_path_with_notification(
        config_path: PathBuf,
        capacity: usize,
    ) -> (Self, Option<RecentFilesError>) {
        match Self::load_from_path(config_path.clone(), capacity) {
            Ok(recent) => (recent, None),
            Err(error) => (
                Self::new(Vec::new(), Some(config_path), capacity, true),
                Some(error),
            ),
        }
    }

    #[cfg(test)]
    pub fn in_memory(capacity: usize) -> Self {
        Self::in_memory_with_paths(capacity, Vec::new())
    }

    pub fn record_success<P: AsRef<Path>>(&mut self, path: P) -> Result<(), RecentFilesError> {
        let path = self.prepare_stored_path(path.as_ref())?;
        self.insert_path(path);
        self.save()
    }

    pub fn remove(&mut self, path: &Path) -> Result<(), RecentFilesError> {
        let path = self.prepare_comparison_path(path)?;
        self.paths.retain(|candidate| candidate != &path);
        self.save()
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn save(&self) -> Result<(), RecentFilesError> {
        let Some(config_path) = &self.config_path else {
            return Ok(());
        };

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).map_err(|_| RecentFilesError::CreateDir {
                path: parent.to_path_buf(),
            })?;
        }

        let payload = serde_json::to_vec_pretty(&RecentConfig {
            recent_files: self.paths.clone(),
        })
        .map_err(|_| RecentFilesError::Serialize {
            path: config_path.clone(),
        })?;

        fs::write(config_path, payload).map_err(|_| RecentFilesError::Write {
            path: config_path.clone(),
        })
    }

    fn new(
        paths: Vec<PathBuf>,
        config_path: Option<PathBuf>,
        capacity: usize,
        canonicalize_on_record: bool,
    ) -> Self {
        let capacity = capacity.min(DEFAULT_CAPACITY);
        if capacity == 0 {
            return Self {
                paths: Vec::new(),
                config_path,
                capacity,
                canonicalize_on_record,
            };
        }

        let mut normalized_paths = Vec::new();

        for path in paths {
            if normalized_paths.iter().any(|candidate| candidate == &path) {
                continue;
            }

            normalized_paths.push(path);

            if normalized_paths.len() == capacity {
                break;
            }
        }

        Self {
            paths: normalized_paths,
            config_path,
            capacity,
            canonicalize_on_record,
        }
    }

    fn in_memory_with_paths(capacity: usize, paths: Vec<PathBuf>) -> Self {
        Self::new(paths, None, capacity, false)
    }

    fn prepare_stored_path(&self, path: &Path) -> Result<PathBuf, RecentFilesError> {
        if self.canonicalize_on_record {
            canonicalize_path(path).map_err(|_| RecentFilesError::Canonicalize {
                path: path.to_path_buf(),
            })
        } else {
            Ok(path.to_path_buf())
        }
    }

    fn prepare_comparison_path(&self, path: &Path) -> Result<PathBuf, RecentFilesError> {
        if !self.canonicalize_on_record {
            return Ok(path.to_path_buf());
        }

        match fs::canonicalize(path) {
            Ok(canonical) => Ok(canonical),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let file_name = path
                    .file_name()
                    .ok_or_else(|| RecentFilesError::Canonicalize {
                        path: path.to_path_buf(),
                    })?;
                let parent = path.parent().unwrap_or_else(|| Path::new("."));
                let canonical_parent =
                    fs::canonicalize(parent).map_err(|_| RecentFilesError::Canonicalize {
                        path: path.to_path_buf(),
                    })?;
                Ok(canonical_parent.join(file_name))
            }
            Err(_) => Err(RecentFilesError::Canonicalize {
                path: path.to_path_buf(),
            }),
        }
    }

    fn insert_path(&mut self, path: PathBuf) {
        self.paths.retain(|candidate| candidate != &path);
        self.paths.insert(0, path);
        self.paths.truncate(self.capacity);
    }
}

impl std::fmt::Display for RecentFilesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigUnavailable => {
                f.write_str("Could not resolve the Recent Files configuration directory.")
            }
            Self::Read { path } => write!(
                f,
                "Could not read the Recent Files configuration at {}.",
                path.display()
            ),
            Self::Parse { path } => write!(
                f,
                "Could not parse the Recent Files configuration at {}.",
                path.display()
            ),
            Self::CreateDir { path } => write!(
                f,
                "Could not create the Recent Files configuration directory at {}.",
                path.display()
            ),
            Self::Write { path } => write!(
                f,
                "Could not write the Recent Files configuration at {}.",
                path.display()
            ),
            Self::Canonicalize { path } => write!(
                f,
                "Could not resolve the Recent Files path {}.",
                path.display()
            ),
            Self::Serialize { path } => write!(
                f,
                "Could not serialize the Recent Files configuration for {}.",
                path.display()
            ),
        }
    }
}

impl std::error::Error for RecentFilesError {}

fn read_config(config_path: &Path) -> Result<RecentConfig, RecentFilesError> {
    match fs::read_to_string(config_path) {
        Ok(contents) => serde_json::from_str(&contents).map_err(|_| RecentFilesError::Parse {
            path: config_path.to_path_buf(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RecentConfig::default()),
        Err(_) => Err(RecentFilesError::Read {
            path: config_path.to_path_buf(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::RecentFiles;

    #[test]
    fn successful_open_moves_existing_path_to_front_without_duplicates() {
        let mut recent = RecentFiles::in_memory(2);
        recent.record_success(path("A.md")).unwrap();
        recent.record_success(path("B.md")).unwrap();
        recent.record_success(path("A.md")).unwrap();

        assert_eq!(recent.paths(), &[path("A.md"), path("B.md")]);
    }

    #[test]
    fn capacity_is_twenty_and_remove_drops_missing_entries() {
        let mut recent = RecentFiles::in_memory(20);

        for index in 0..21 {
            recent.record_success(path(&format!("{index}.md"))).unwrap();
        }

        assert_eq!(recent.paths().len(), 20);

        recent.remove(&path("10.md")).unwrap();

        assert!(!recent.paths().contains(&path("10.md")));
    }

    #[test]
    fn load_returns_empty_recent_files_when_config_is_missing() {
        let temp = TempDir::new();

        let recent = RecentFiles::load_from_path(temp.path().join("recent.json"), 20).unwrap();

        assert!(recent.paths().is_empty());
    }

    #[test]
    fn save_and_load_round_trip_recent_paths() {
        let temp = TempDir::new();
        let config_path = temp.path().join("recent.json");
        let first = temp.write_markdown("first.md");
        let second = temp.write_markdown("second.md");
        let mut recent = RecentFiles::load_from_path(config_path.clone(), 20).unwrap();

        recent.record_success(first.clone()).unwrap();
        recent.record_success(second.clone()).unwrap();
        recent.save().unwrap();

        let loaded = RecentFiles::load_from_path(config_path, 20).unwrap();

        assert_eq!(
            loaded.paths(),
            &[
                second.canonicalize().unwrap(),
                first.canonicalize().unwrap()
            ]
        );
    }

    #[test]
    fn remove_drops_deleted_production_path() {
        let temp = TempDir::new();
        let config_path = temp.path().join("recent.json");
        let markdown_path = temp.write_markdown("stale.md");
        let stored_path = markdown_path.canonicalize().unwrap();
        let mut recent = RecentFiles::load_from_path(config_path.clone(), 20).unwrap();

        recent.record_success(&markdown_path).unwrap();
        fs::remove_file(&markdown_path).unwrap();

        recent.remove(&stored_path).unwrap();

        assert!(!recent.paths().contains(&stored_path));
        let loaded = RecentFiles::load_from_path(config_path, 20).unwrap();
        assert!(loaded.paths().is_empty());
    }

    #[test]
    fn malformed_config_returns_empty_recent_files_and_an_error_through_injected_loader() {
        let temp = TempDir::new();
        let config_path = temp.path().join("recent.json");
        fs::write(&config_path, "{not valid json").unwrap();

        let (recent, error) = RecentFiles::load_from_path_with_notification(config_path, 20);

        assert!(recent.paths().is_empty());
        assert!(error.is_some());
    }

    fn path(value: &str) -> PathBuf {
        PathBuf::from(value)
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
            let path = std::env::temp_dir().join(format!("mieli-task-3-{unique}-{suffix}"));
            fs::create_dir_all(&path).unwrap();

            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn write_markdown(&self, relative: &str) -> PathBuf {
            let path = self.path.join(relative);
            fs::write(&path, relative).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

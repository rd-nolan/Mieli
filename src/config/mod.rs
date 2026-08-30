use std::path::{Path, PathBuf};

use directories::ProjectDirs;

pub mod recent;

const RECENT_FILES_CONFIG_NAME: &str = "recent-files.json";

pub(crate) fn recent_files_path(project_dirs: Option<ProjectDirs>) -> Option<PathBuf> {
    project_dirs.map(|project_dirs| recent_files_path_in_dir(project_dirs.config_dir()))
}

pub(crate) fn recent_files_path_in_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(RECENT_FILES_CONFIG_NAME)
}

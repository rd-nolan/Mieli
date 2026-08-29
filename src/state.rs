use std::{path::PathBuf, time::SystemTime};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TabId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiskState {
    Synced,
    ModifiedExternally,
    Deleted,
    Conflict,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiskVersion {
    pub exists: bool,
    pub modified: Option<SystemTime>,
    pub len: u64,
    pub digest: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileTreeNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub expanded: bool,
    pub children: Vec<FileTreeNode>,
}

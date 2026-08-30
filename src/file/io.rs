use std::{
    ffi::OsStr,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
};

use crate::{file::FileError, state::DiskVersion};

pub fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}

pub fn read_markdown(path: &Path) -> Result<String, FileError> {
    fs::read_to_string(path).map_err(|error| FileError::from_io(path, "read", error))
}

pub fn write_markdown(path: &Path, source: &str) -> Result<DiskVersion, FileError> {
    fs::write(path, source.as_bytes()).map_err(|error| FileError::from_io(path, "write", error))?;
    disk_version(path)
}

pub fn canonicalize_path(path: &Path) -> Result<PathBuf, FileError> {
    match fs::canonicalize(path) {
        Ok(canonical) => return Ok(canonical),
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(FileError::from_io(path, "canonicalize", error));
        }
        Err(_) => {}
    }

    let file_name = path
        .file_name()
        .ok_or_else(|| FileError::other(path, "canonicalize"))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|error| FileError::from_io(path, "canonicalize", error))?;
    Ok(canonical_parent.join(file_name))
}

pub fn disk_version(path: &Path) -> Result<DiskVersion, FileError> {
    match fs::metadata(path) {
        Ok(metadata) => {
            let bytes = fs::read(path).map_err(|error| FileError::from_io(path, "read", error))?;
            Ok(DiskVersion {
                exists: true,
                modified: metadata.modified().ok(),
                len: metadata.len(),
                digest: digest_bytes(&bytes),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DiskVersion {
            exists: false,
            modified: None,
            len: 0,
            digest: 0,
        }),
        Err(error) => Err(FileError::from_io(path, "inspect", error)),
    }
}

fn digest_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::state::DiskVersion;

    use super::{
        canonicalize_path, digest_bytes, disk_version, is_markdown_file, read_markdown,
        write_markdown,
    };

    #[test]
    fn markdown_extension_is_case_insensitive() {
        assert!(is_markdown_file(Path::new("README.md")));
        assert!(is_markdown_file(Path::new("README.MD")));
        assert!(is_markdown_file(Path::new("note.markdown")));
        assert!(is_markdown_file(Path::new("note.MARKDOWN")));
        assert!(!is_markdown_file(Path::new("test.txt")));
        assert!(!is_markdown_file(Path::new("image.png")));
    }

    #[test]
    fn read_markdown_rejects_invalid_utf8_with_exact_message() {
        let temp = TempDir::new();
        let path = temp.path().join("broken.md");
        fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();

        let error = read_markdown(&path).unwrap_err();

        assert_eq!(error.to_string(), "The file is not valid UTF-8.");
    }

    #[test]
    fn write_markdown_returns_the_version_from_disk() {
        let temp = TempDir::new();
        let path = temp.path().join("note.md");

        let version = write_markdown(&path, "# Hello\n").unwrap();

        assert!(version.exists);
        assert_eq!(version.len, 8);
        assert_eq!(version.digest, digest_bytes(b"# Hello\n"));
        assert_eq!(read_markdown(&path).unwrap(), "# Hello\n");
    }

    #[test]
    fn write_markdown_does_not_create_parent_directories() {
        let temp = TempDir::new();
        let path = temp.path().join("nested").join("note.md");

        let error = write_markdown(&path, "text").unwrap_err();

        assert!(matches!(error, crate::file::FileError::NotFound { .. }));
    }

    #[test]
    fn canonicalize_path_uses_the_canonical_parent_for_missing_save_as_paths() {
        let temp = TempDir::new();
        let existing_parent = temp.path().join("folder");
        fs::create_dir_all(&existing_parent).unwrap();
        let destination = existing_parent.join("draft.md");

        let canonical = canonicalize_path(&destination).unwrap();

        assert_eq!(
            canonical,
            fs::canonicalize(&existing_parent).unwrap().join("draft.md")
        );
    }

    #[cfg(unix)]
    #[test]
    fn canonicalize_path_returns_permission_denied_for_inaccessible_existing_paths() {
        use std::os::unix::{fs::PermissionsExt, fs::symlink};

        let temp = TempDir::new();
        let hidden_dir = temp.path().join("hidden");
        fs::create_dir_all(&hidden_dir).unwrap();
        let hidden_file = hidden_dir.join("secret.md");
        fs::write(&hidden_file, "secret").unwrap();

        let original_permissions = fs::metadata(&hidden_dir).unwrap().permissions();
        let symlink_path = temp.path().join("secret-link.md");
        symlink(&hidden_file, &symlink_path).unwrap();

        fs::set_permissions(&hidden_dir, PermissionsExt::from_mode(0o000)).unwrap();

        let result = canonicalize_path(&symlink_path);

        fs::set_permissions(&hidden_dir, original_permissions).unwrap();

        assert!(
            matches!(result, Err(crate::file::FileError::PermissionDenied { path, operation }) if path == symlink_path && operation == "canonicalize")
        );
    }

    #[test]
    fn disk_version_reports_missing_files_without_error() {
        let temp = TempDir::new();
        let missing = temp.path().join("missing.md");

        let version = disk_version(&missing).unwrap();

        assert_eq!(
            version,
            DiskVersion {
                exists: false,
                modified: None,
                len: 0,
                digest: 0,
            }
        );
    }

    #[test]
    fn bezel_round_trip_reaches_a_fixed_point() {
        let source = "# Title\n\nParagraph with **bold**, *italic*, and [a link](https://example.com).\n\n- bullet\n- [ ] item\n- [x] done\n\n1. first\n2. second\n\n> quote\n\nUse `inline` code.\n\n```rust\nfn main() {}\n```\n\n| Col | Value |\n| --- | --- |\n| A | 1 |\n";

        let first = markdown::serialize(&markdown::parse(source));
        let second = markdown::serialize(&markdown::parse(&first));

        assert_eq!(first, second);
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

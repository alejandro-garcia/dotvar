//! Filesystem reads for the desktop app. Call these on GPUI's background executor.

use crate::i18n::tr;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use env_core::Document;

/// The path chosen by the user and the original, unmodified file contents.
pub struct LoadedDocument {
    pub path: PathBuf,
    pub document: Document,
}

/// Matches the supported file names, without touching the filesystem.
pub fn is_env_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == ".env" || name.starts_with(".env.") || name.ends_with(".env"))
}

/// Discovers only files immediately inside the selected folder.
///
/// Symlinks to regular files are included under their chosen path. Dangling
/// symlinks are ignored; other metadata errors remain visible to the caller.
/// This does not read the contents of any discovered file.
pub fn discover(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if !is_env_file(&path) {
            continue;
        }
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => paths.push(path),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    paths.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.file_name().cmp(&b.file_name()))
    });
    Ok(paths)
}

fn rank(path: &Path) -> u8 {
    match path.file_name().and_then(|name| name.to_str()) {
        Some(".env") => 0,
        Some(".env.local") => 1,
        _ => 2,
    }
}

/// Reads raw bytes; unsupported encodings stay intact in a read-only Document.
/// Errors identify the file without including any file contents.
pub fn read_document(path: PathBuf) -> Result<LoadedDocument, String> {
    let bytes = fs::read(&path).map_err(|error| {
        tr!(
            "file.open_failed",
            path = path.display(),
            error = crate::storage::StorageError::Io(error),
        )
        .into_owned()
    })?;
    Ok(LoadedDocument {
        path,
        document: Document::from_bytes(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            // create_dir is exclusive, including across parallel test processes.
            loop {
                let number = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir()
                    .join(format!("dotvar-model-test-{}-{number}", std::process::id()));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("Could not create test directory: {error}"),
                }
            }
        }

        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, bytes).expect("write synthetic fixture");
            path
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn matches_supported_names_only() {
        for name in [
            ".env",
            ".env.local",
            ".env.production",
            "app.env",
            "sub/.env",
        ] {
            assert!(is_env_file(Path::new(name)), "{name}");
        }
        for name in ["env", ".envrc", ".env-example", "app.env.bak", ".ENV", ""] {
            assert!(!is_env_file(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn discovery_is_shallow_and_prioritizes_default_files() {
        let directory = TempDirectory::new();
        directory.file("z.env", b"Z=synthetic\n");
        directory.file(".env.production", b"PRODUCTION=synthetic\n");
        directory.file(".env.local", b"LOCAL=synthetic\n");
        directory.file(".env", b"DEFAULT=synthetic\n");
        directory.file(".envrc", b"UNRELATED=synthetic\n");
        let nested = directory.0.join("nested.env");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join(".env"), b"NESTED=synthetic\n").unwrap();

        let discovered = discover(&directory.0).unwrap();
        assert_eq!(
            discovered,
            [".env", ".env.local", ".env.production", "z.env"].map(|name| directory.0.join(name))
        );
    }

    #[cfg(unix)]
    #[test]
    fn discovery_keeps_file_symlinks_and_ignores_dangling_links() {
        use std::os::unix::fs::symlink;

        let directory = TempDirectory::new();
        let target = directory.file("source.txt", b"LINK=synthetic\n");
        let link = directory.0.join("linked.env");
        symlink(&target, &link).unwrap();
        symlink(directory.0.join("missing"), directory.0.join("missing.env")).unwrap();
        symlink(&directory.0, directory.0.join("folder.env")).unwrap();

        assert_eq!(discover(&directory.0).unwrap(), vec![link.clone()]);
        let loaded = read_document(link.clone()).unwrap();
        assert_eq!(loaded.path, link);
        assert_eq!(loaded.document.bytes(), b"LINK=synthetic\n");
    }

    #[test]
    fn reads_exact_bytes_and_keeps_unsupported_encoding() {
        let directory = TempDirectory::new();
        let bytes = b"\xef\xbb\xbf# synthetic\r\nVALUE='prueba'\nEMPTY=";
        let path = directory.file(".env", bytes);
        let loaded = read_document(path.clone()).unwrap();
        assert_eq!(loaded.path, path);
        assert_eq!(loaded.document.bytes(), bytes);
        assert!(!loaded.document.is_read_only());

        let invalid = directory.file("invalid.env", b"VALUE=\xff\r\n");
        let loaded = read_document(invalid).unwrap();
        assert!(loaded.document.is_read_only());
        assert_eq!(loaded.document.bytes(), b"VALUE=\xff\r\n");
    }

    #[test]
    fn io_failures_are_errors_and_never_create_a_file() {
        let directory = TempDirectory::new();
        let missing = directory.0.join("missing.env");
        let error = read_document(missing.clone()).err().unwrap();
        assert!(error.contains("missing.env"));
        assert!(!missing.exists());
        assert!(discover(&missing).is_err());
        assert!(read_document(directory.0.clone()).is_err());
    }
}

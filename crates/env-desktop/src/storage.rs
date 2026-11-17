//! Filesystem storage for documents. Run these blocking operations off the UI thread.
//!
//! The chosen path and its resolved destination are revalidated before replacement.
//! This detects ordinary external edits and link changes, but is not a filesystem
//! compare-and-swap or a lock against arbitrary concurrent writers.

use crate::i18n::tr;
use std::{
    fmt,
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

use tempfile::{Builder, NamedTempFile};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

/// Physical identity for sharing a single in-memory document across aliases.
/// Other desktop platforms still require their own real identity adapter/QA.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(not(unix))]
    resolved_path: PathBuf,
}

/// Reference read from disk, independent from the editable document in memory.
#[derive(Clone)]
pub struct FileBaseline {
    pub chosen_path: PathBuf,
    pub resolved_path: PathBuf,
    pub bytes: Vec<u8>,
    chosen_stamp: Stamp,
    file_stamp: Stamp,
    access_metadata_supported: bool,
}

impl fmt::Debug for FileBaseline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileBaseline")
            .field("chosen_path", &self.chosen_path)
            .field("resolved_path", &self.resolved_path)
            .field("byte_count", &self.bytes.len())
            .field("hardlinked", &self.is_hardlinked())
            .field("access_metadata_supported", &self.access_metadata_supported)
            .finish()
    }
}

impl FileBaseline {
    pub fn identity(&self) -> FileIdentity {
        FileIdentity {
            #[cfg(unix)]
            device: self.file_stamp.dev,
            #[cfg(unix)]
            inode: self.file_stamp.ino,
            #[cfg(not(unix))]
            resolved_path: self.resolved_path.clone(),
        }
    }

    /// Includes both the chosen alias and resolved target, metadata and bytes.
    /// This is a snapshot comparison, not a filesystem lock.
    pub fn same_reference(&self, other: &Self) -> bool {
        self.chosen_path == other.chosen_path && reference_matches(self, other)
    }

    pub fn is_hardlinked(&self) -> bool {
        self.file_stamp.links > 1
    }

    /// Unsupported ACLs or extended attributes never get silently stripped.
    pub fn has_unsupported_access_metadata(&self) -> bool {
        !self.access_metadata_supported
    }
}

/// Errors intentionally contain no document bytes or value tokens.
#[derive(Debug)]
pub enum StorageError {
    Io(io::Error),
    Conflict,
    Hardlink,
    UnsupportedAccessMetadata,
    AlreadyExists,
    InvalidPath,
    /// Replacement/creation happened, but durability or read-back was not verified.
    Uncertain,
    /// The directory entry moved, but durability or read-back was not verified.
    RenameUncertain,
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Io(error) => match error.kind() {
                io::ErrorKind::PermissionDenied => tr!("storage.permission_denied"),
                io::ErrorKind::NotFound => tr!("storage.not_found"),
                _ => match error.raw_os_error() {
                    Some(code) => tr!("storage.io_with_code", code = code),
                    None => tr!("storage.io"),
                },
            },
            Self::Conflict => tr!("storage.conflict"),
            Self::Hardlink => tr!("storage.hardlink"),
            Self::UnsupportedAccessMetadata => tr!("storage.unsupported_access"),
            Self::AlreadyExists => tr!("storage.already_exists"),
            Self::InvalidPath => tr!("storage.invalid_path"),
            Self::Uncertain => tr!("storage.uncertain"),
            Self::RenameUncertain => tr!("storage.rename_uncertain"),
        };
        f.write_str(&message)
    }
}

impl StorageError {
    pub fn is_conflict(&self) -> bool {
        matches!(self, Self::Conflict)
    }

    pub fn is_uncertain(&self) -> bool {
        matches!(self, Self::Uncertain | Self::RenameUncertain)
    }

    pub fn requires_save_as(&self) -> bool {
        matches!(self, Self::Hardlink | Self::UnsupportedAccessMetadata)
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for StorageError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: Option<SystemTime>,
    is_symlink: bool,
    links: u64,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(unix)]
    mode: u32,
    #[cfg(unix)]
    uid: u32,
    #[cfg(unix)]
    gid: u32,
    #[cfg(unix)]
    ctime: (i64, i64),
}

impl Stamp {
    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            size: metadata.len(),
            modified: metadata.modified().ok(),
            is_symlink: metadata.file_type().is_symlink(),
            #[cfg(unix)]
            links: metadata.nlink(),
            #[cfg(not(unix))]
            links: 1,
            #[cfg(unix)]
            dev: metadata.dev(),
            #[cfg(unix)]
            ino: metadata.ino(),
            #[cfg(unix)]
            mode: metadata.mode(),
            #[cfg(unix)]
            uid: metadata.uid(),
            #[cfg(unix)]
            gid: metadata.gid(),
            #[cfg(unix)]
            ctime: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }

    fn same_file(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            self.dev == other.dev && self.ino == other.ino
        }
        #[cfg(not(unix))]
        {
            self == other
        }
    }

    fn same_access(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            self.uid == other.uid
                && self.gid == other.gid
                && self.mode & 0o7777 == other.mode & 0o7777
        }
        #[cfg(not(unix))]
        {
            self == other
        }
    }
}

fn absolute(path: PathBuf) -> Result<PathBuf, StorageError> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

/// Read a stable reference, retaining the user's symlink path separately.
pub fn open(path: PathBuf) -> Result<FileBaseline, StorageError> {
    let chosen_path = absolute(path)?;
    let resolved_path = fs::canonicalize(&chosen_path)?;
    let chosen_stamp = Stamp::from_metadata(&fs::symlink_metadata(&chosen_path)?);
    let mut file = File::open(&resolved_path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(StorageError::InvalidPath);
    }
    let file_stamp = Stamp::from_metadata(&metadata);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let after = Stamp::from_metadata(&file.metadata()?);
    let access_metadata_supported = access_metadata_supported(&resolved_path);
    if file_stamp != after
        || Stamp::from_metadata(&fs::metadata(&resolved_path)?) != file_stamp
        || fs::canonicalize(&chosen_path)? != resolved_path
        || Stamp::from_metadata(&fs::symlink_metadata(&chosen_path)?) != chosen_stamp
    {
        return Err(StorageError::Conflict);
    }
    Ok(FileBaseline {
        chosen_path,
        resolved_path,
        bytes,
        chosen_stamp,
        file_stamp,
        access_metadata_supported,
    })
}

fn reference_matches(expected: &FileBaseline, actual: &FileBaseline) -> bool {
    expected.resolved_path == actual.resolved_path
        && expected.chosen_stamp == actual.chosen_stamp
        && expected.file_stamp == actual.file_stamp
        && expected.bytes == actual.bytes
        && expected.access_metadata_supported == actual.access_metadata_supported
}

fn validate(expected: &FileBaseline) -> Result<FileBaseline, StorageError> {
    let actual = open(expected.chosen_path.clone())?;
    if !reference_matches(expected, &actual) {
        return Err(StorageError::Conflict);
    }
    Ok(actual)
}

/// Revalidate all secondary paths referencing a shared document before saving.
/// The primary chosen path is checked by `save` itself. A retargeted/deleted
/// alias must not cause a save through that alias to silently edit another file.
/// Like the main pre-save check, this reduces races without locking the paths.
pub fn validate_aliases(baseline: &FileBaseline, paths: &[PathBuf]) -> Result<(), StorageError> {
    let identity = baseline.identity();
    for path in paths {
        let path = absolute(path.clone())?;
        if path == baseline.chosen_path {
            continue;
        }
        let actual = open(path)?;
        if actual.identity() != identity || actual.bytes != baseline.bytes {
            return Err(StorageError::Conflict);
        }
    }
    Ok(())
}

pub(crate) fn file_destination(directory: &Path, name: &str) -> Option<PathBuf> {
    if name.contains(['/', '\0']) || !crate::model::is_env_file(Path::new(name)) {
        return None;
    }
    Some(directory.join(name))
}

/// Rename the chosen directory entry, including a symlink itself when selected.
/// The returned baseline keeps the existing document tied to the new name even
/// if the move succeeded but directory sync or read-back could not be verified.
pub fn rename_file(
    baseline: &FileBaseline,
    source: &Path,
    name: &str,
) -> Result<(FileBaseline, Option<StorageError>), StorageError> {
    let parent = source.parent().ok_or(StorageError::InvalidPath)?;
    let destination = file_destination(parent, name).ok_or(StorageError::InvalidPath)?;
    let mut current = if source == baseline.chosen_path {
        validate(baseline)?
    } else {
        let alias = open(source.to_path_buf())?;
        if alias.identity() != baseline.identity() || alias.bytes != baseline.bytes {
            return Err(StorageError::Conflict);
        }
        alias
    };
    if destination == current.chosen_path {
        return Ok((current, None));
    }
    let directory = File::open(parent)?;
    rename_exclusive(
        &directory,
        source.file_name().ok_or(StorageError::InvalidPath)?,
        name,
    )?;
    current.chosen_path = destination;
    if !current.chosen_stamp.is_symlink {
        current.resolved_path.set_file_name(name);
    }
    let warning = directory
        .sync_all()
        .err()
        .map(|_| StorageError::RenameUncertain);
    if let Ok(actual) = open(current.chosen_path.clone())
        && actual.chosen_stamp.same_file(&current.chosen_stamp)
        && actual.file_stamp.same_file(&current.file_stamp)
        && actual.file_stamp.same_access(&current.file_stamp)
        && actual.bytes == current.bytes
    {
        return Ok((actual, warning));
    }
    Ok((current, Some(StorageError::RenameUncertain)))
}

#[cfg(any(target_vendor = "apple", target_os = "linux"))]
fn rename_exclusive(
    directory: &File,
    from: &std::ffi::OsStr,
    name: &str,
) -> Result<(), StorageError> {
    use rustix::fs::{RenameFlags, renameat_with};

    renameat_with(directory, from, directory, name, RenameFlags::NOREPLACE).map_err(|error| {
        let error = io::Error::from(error);
        if error.kind() == io::ErrorKind::AlreadyExists {
            StorageError::AlreadyExists
        } else {
            StorageError::Io(error)
        }
    })
}

#[cfg(not(any(target_vendor = "apple", target_os = "linux")))]
fn rename_exclusive(_: &File, _: &std::ffi::OsStr, _: &str) -> Result<(), StorageError> {
    Err(io::Error::from(io::ErrorKind::Unsupported).into())
}

// std does not expose macOS ACL/xattr preservation. Detect their presence without
// reading xattr values, and reject replacement instead of weakening access.
// The OS-managed writer-provenance marker is allowed to be regenerated by macOS;
// it is not an access grant. All other attributes are conservatively unsupported.
#[cfg(target_os = "macos")]
fn access_metadata_supported(path: &Path) -> bool {
    use std::process::Command;

    let Ok(acl) = Command::new("/bin/ls")
        .arg("-lde")
        .arg("--")
        .arg(path)
        .output()
    else {
        return false;
    };
    if !acl.status.success() {
        return false;
    }
    let mode = acl
        .stdout
        .split(|byte| byte.is_ascii_whitespace())
        .next()
        .unwrap_or_default();
    // When xattrs are present, ls can print '@' instead of '+' in the mode.
    // ACL entries still occupy additional lines with -e. Ambiguous filenames
    // containing newlines are rejected conservatively by the same check.
    if mode.first() != Some(&b'-')
        || mode.len() < 10
        || mode.get(10) == Some(&b'+')
        || acl.stdout.iter().filter(|&&byte| byte == b'\n').count() != 1
    {
        return false;
    }
    // xattr without -p or -l returns attribute names only, never their values.
    let Ok(attributes) = Command::new("/usr/bin/xattr").arg(path).output() else {
        return false;
    };
    attributes.status.success()
        && (attributes.stdout.is_empty()
            || attributes.stdout.as_slice() == b"com.apple.provenance\n")
}

#[cfg(not(target_os = "macos"))]
fn access_metadata_supported(_path: &Path) -> bool {
    // Other platforms have not yet received the real metadata adapter/QA.
    false
}

fn temporary(parent: &Path) -> Result<NamedTempFile, StorageError> {
    let mut builder = Builder::new();
    builder.prefix(".dotvar-");
    #[cfg(unix)]
    builder.permissions(fs::Permissions::from_mode(0o600));
    let temporary = builder.tempfile_in(parent)?;
    if !access_metadata_supported(temporary.path()) {
        return Err(StorageError::UnsupportedAccessMetadata);
    }
    Ok(temporary)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Write,
    Sync,
    Metadata,
    Revalidate,
    Replace,
    DirectorySync,
    ReadBack,
}

fn preserve_metadata(
    temporary: &NamedTempFile,
    baseline: &FileBaseline,
) -> Result<(), StorageError> {
    #[cfg(unix)]
    {
        let stamp = &baseline.file_stamp;
        let metadata = temporary.as_file().metadata()?;
        if metadata.uid() != stamp.uid || metadata.gid() != stamp.gid {
            std::os::unix::fs::chown(temporary.path(), Some(stamp.uid), Some(stamp.gid))?;
        }
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(stamp.mode & 0o7777))?;
        let after = temporary.as_file().metadata()?;
        if after.uid() != stamp.uid
            || after.gid() != stamp.gid
            || after.mode() & 0o7777 != stamp.mode & 0o7777
        {
            return Err(StorageError::UnsupportedAccessMetadata);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (temporary, baseline);
        return Err(StorageError::UnsupportedAccessMetadata);
    }
    Ok(())
}

/// Save an immutable snapshot. The caller only advances its saved reference to
/// the returned baseline; newer in-memory edits remain separate and dirty.
pub fn save(baseline: &FileBaseline, bytes: &[u8]) -> Result<FileBaseline, StorageError> {
    save_with_hook(baseline, bytes, &mut |_, _| Ok(()))
}

fn save_with_hook(
    baseline: &FileBaseline,
    bytes: &[u8],
    hook: &mut impl FnMut(Stage, &Path) -> io::Result<()>,
) -> Result<FileBaseline, StorageError> {
    let current = validate(baseline)?;
    if current.is_hardlinked() {
        return Err(StorageError::Hardlink);
    }
    if current.bytes == bytes {
        return Ok(current);
    }
    if !current.access_metadata_supported {
        return Err(StorageError::UnsupportedAccessMetadata);
    }
    #[cfg(unix)]
    if current.file_stamp.mode & 0o222 == 0 {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied).into());
    }
    // Atomic rename must not silently bypass a read-only original's permissions.
    let _writable = OpenOptions::new()
        .write(true)
        .open(&current.resolved_path)?;
    let parent = current
        .resolved_path
        .parent()
        .ok_or(StorageError::InvalidPath)?;
    let mut temporary = temporary(parent)?;
    hook(Stage::Write, temporary.path())?;
    temporary.write_all(bytes)?;
    hook(Stage::Sync, temporary.path())?;
    temporary.as_file().sync_all()?;
    hook(Stage::Metadata, temporary.path())?;
    preserve_metadata(&temporary, &current)?;
    temporary.as_file().sync_all()?;
    hook(Stage::Revalidate, temporary.path())?;
    validate(baseline)?;
    let temporary_stamp = Stamp::from_metadata(&temporary.as_file().metadata()?);
    if !temporary_stamp.same_file(&Stamp::from_metadata(&fs::metadata(temporary.path())?)) {
        return Err(StorageError::Conflict);
    }
    hook(Stage::Replace, temporary.path())?;
    let persisted = temporary
        .persist(&current.resolved_path)
        .map_err(|error| StorageError::Io(error.error))?;
    let persisted_stamp =
        Stamp::from_metadata(&persisted.metadata().map_err(|_| StorageError::Uncertain)?);
    hook(Stage::DirectorySync, parent).map_err(|_| StorageError::Uncertain)?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| StorageError::Uncertain)?;
    hook(Stage::ReadBack, &current.resolved_path).map_err(|_| StorageError::Uncertain)?;
    let actual = open(current.chosen_path).map_err(|_| StorageError::Uncertain)?;
    if actual.bytes != bytes
        || !actual.file_stamp.same_file(&persisted_stamp)
        || actual.is_hardlinked()
        || !actual.file_stamp.same_access(&current.file_stamp)
        || !actual.access_metadata_supported
    {
        return Err(StorageError::Uncertain);
    }
    Ok(actual)
}

/// Create a new file with user-only access, never overwriting a file or symlink.
pub fn save_as(path: PathBuf, bytes: &[u8]) -> Result<FileBaseline, StorageError> {
    save_as_with_hook(path, bytes, &mut |_, _| Ok(()))
}

fn save_as_with_hook(
    path: PathBuf,
    bytes: &[u8],
    hook: &mut impl FnMut(Stage, &Path) -> io::Result<()>,
) -> Result<FileBaseline, StorageError> {
    let chosen = absolute(path)?;
    let name = chosen.file_name().ok_or(StorageError::InvalidPath)?;
    let parent_chosen = chosen.parent().ok_or(StorageError::InvalidPath)?;
    let parent = fs::canonicalize(parent_chosen)?;
    if !parent.is_dir() {
        return Err(StorageError::InvalidPath);
    }
    let resolved = parent.join(name);
    match fs::symlink_metadata(&resolved) {
        Ok(_) => return Err(StorageError::AlreadyExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut temporary = temporary(&parent)?;
    let private_stamp = Stamp::from_metadata(&temporary.as_file().metadata()?);
    #[cfg(unix)]
    if private_stamp.mode & 0o7777 != 0o600 {
        return Err(StorageError::UnsupportedAccessMetadata);
    }
    hook(Stage::Write, temporary.path())?;
    temporary.write_all(bytes)?;
    hook(Stage::Sync, temporary.path())?;
    temporary.as_file().sync_all()?;
    hook(Stage::Revalidate, temporary.path())?;
    if fs::canonicalize(parent_chosen)? != parent {
        return Err(StorageError::Conflict);
    }
    hook(Stage::Replace, temporary.path())?;
    let before_persist = Stamp::from_metadata(&temporary.as_file().metadata()?);
    if !before_persist.same_access(&private_stamp)
        || !before_persist.same_file(&private_stamp)
        || !before_persist.same_file(&Stamp::from_metadata(&fs::metadata(temporary.path())?))
        || !access_metadata_supported(temporary.path())
    {
        return Err(StorageError::UnsupportedAccessMetadata);
    }
    let persisted = temporary.persist_noclobber(&resolved).map_err(|error| {
        if error.error.kind() == io::ErrorKind::AlreadyExists {
            StorageError::AlreadyExists
        } else {
            StorageError::Io(error.error)
        }
    })?;
    let persisted_stamp =
        Stamp::from_metadata(&persisted.metadata().map_err(|_| StorageError::Uncertain)?);
    hook(Stage::DirectorySync, &parent).map_err(|_| StorageError::Uncertain)?;
    File::open(&parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| StorageError::Uncertain)?;
    hook(Stage::ReadBack, &resolved).map_err(|_| StorageError::Uncertain)?;
    let actual = open(chosen).map_err(|_| StorageError::Uncertain)?;
    if actual.bytes != bytes
        || !actual.file_stamp.same_file(&persisted_stamp)
        || actual.is_hardlinked()
        || !actual.file_stamp.same_access(&private_stamp)
        || !actual.access_metadata_supported
    {
        return Err(StorageError::Uncertain);
    }
    Ok(actual)
}

pub fn create_empty(path: PathBuf) -> Result<FileBaseline, StorageError> {
    save_as(path, &[])
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::process::Command;
    use tempfile::{TempDir, tempdir};

    fn fixture(directory: &TempDir) -> FileBaseline {
        let path = directory.path().join("test.env");
        fs::write(&path, b"# synthetic\r\nVALUE=before\r\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        open(path).unwrap()
    }

    #[test]
    fn rename_preserves_identity_and_contents_and_saves_at_the_new_path() {
        let directory = TempDir::new().unwrap();
        let baseline = fixture(&directory);
        let source = baseline.chosen_path.clone();
        let (renamed, warning) = rename_file(&baseline, &source, ".env.Staging").unwrap();
        assert!(warning.is_none());
        assert!(!source.exists());
        assert_eq!(renamed.chosen_path, directory.path().join(".env.Staging"));
        assert_eq!(renamed.identity(), baseline.identity());
        assert_eq!(renamed.bytes, baseline.bytes);
        assert!(renamed.file_stamp.same_access(&baseline.file_stamp));

        let (renamed, warning) =
            rename_file(&renamed, &renamed.chosen_path, ".env.staging").unwrap();
        assert!(warning.is_none());
        assert_eq!(renamed.chosen_path, directory.path().join(".env.staging"));
        assert_eq!(
            fs::read_dir(directory.path())
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .file_name(),
            ".env.staging"
        );
        save(&renamed, b"K=updated\n").unwrap();
        assert_eq!(fs::read(renamed.chosen_path).unwrap(), b"K=updated\n");
        assert!(!source.exists());
    }

    #[test]
    fn rename_rejects_conflicts_and_paths_without_moving_the_source() {
        let directory = TempDir::new().unwrap();
        let baseline = fixture(&directory);
        let source = &baseline.chosen_path;
        let taken = directory.path().join(".env.taken");
        std::os::unix::fs::symlink("missing", &taken).unwrap();
        assert!(matches!(
            rename_file(&baseline, source, ".env.taken"),
            Err(StorageError::AlreadyExists)
        ));
        for name in ["", "config", "../.env", ".env/", ".env.\0"] {
            assert!(matches!(
                rename_file(&baseline, source, name),
                Err(StorageError::InvalidPath)
            ));
        }
        assert_eq!(fs::read_link(taken).unwrap(), Path::new("missing"));
        assert_eq!(fs::read(source).unwrap(), baseline.bytes);
        fs::write(source, b"K=external").unwrap();
        assert!(matches!(
            rename_file(&baseline, source, "new.env"),
            Err(StorageError::Conflict)
        ));
        assert_eq!(fs::read(source).unwrap(), b"K=external");
    }

    #[test]
    fn renaming_a_secondary_symlink_moves_the_link_in_place() {
        let directory = TempDir::new().unwrap();
        let baseline = fixture(&directory);
        let link = directory.path().join("alias.env");
        let target = baseline.chosen_path.file_name().unwrap();
        std::os::unix::fs::symlink(target, &link).unwrap();
        let (renamed, warning) = rename_file(&baseline, &link, "renamed.env").unwrap();
        assert!(warning.is_none());
        assert!(!link.exists());
        assert_eq!(
            fs::read_link(renamed.chosen_path).unwrap(),
            Path::new(target)
        );
        assert_eq!(renamed.resolved_path, baseline.resolved_path);
        assert_eq!(fs::read(baseline.chosen_path).unwrap(), baseline.bytes);
    }

    fn assert_no_temporaries(directory: &Path) {
        assert!(fs::read_dir(directory).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".dotvar-")
        }));
    }

    #[test]
    fn save_reads_back_snapshot_and_preserves_unix_access() {
        let directory = tempdir().unwrap();
        let initial = fixture(&directory);
        let before = fs::metadata(&initial.resolved_path).unwrap();
        let expected = b"# synthetic\r\nVALUE=after\r\n";
        let saved = save(&initial, expected).unwrap();
        assert_eq!(saved.bytes, expected);
        assert_eq!(fs::read(&initial.chosen_path).unwrap(), expected);
        let after = fs::metadata(&initial.resolved_path).unwrap();
        assert_eq!(after.mode() & 0o7777, before.mode() & 0o7777);
        assert_eq!((after.uid(), after.gid()), (before.uid(), before.gid()));
        assert!(!saved.is_hardlinked());
        assert_no_temporaries(directory.path());
    }

    #[test]
    fn unchanged_save_does_not_replace_or_touch_mtime() {
        let directory = tempdir().unwrap();
        let initial = fixture(&directory);
        let saved = save(&initial, &initial.bytes).unwrap();
        assert_eq!(saved.file_stamp, initial.file_stamp);
        assert_no_temporaries(directory.path());
    }

    #[test]
    fn external_content_change_and_atomic_replacement_conflict() {
        for replace_inode in [false, true] {
            let directory = tempdir().unwrap();
            let initial = fixture(&directory);
            if replace_inode {
                let other = directory.path().join("replacement");
                fs::write(&other, &initial.bytes).unwrap();
                fs::rename(other, &initial.resolved_path).unwrap();
            } else {
                fs::write(&initial.resolved_path, b"EXTERNAL=synthetic\n").unwrap();
            }
            let actual = fs::read(&initial.resolved_path).unwrap();
            assert!(matches!(
                save(&initial, b"VALUE=ours\n"),
                Err(StorageError::Conflict)
            ));
            assert_eq!(fs::read(&initial.resolved_path).unwrap(), actual);
            assert_no_temporaries(directory.path());
        }
    }

    #[test]
    fn removed_destination_is_not_recreated_even_for_noop() {
        let directory = tempdir().unwrap();
        let initial = fixture(&directory);
        fs::remove_file(&initial.chosen_path).unwrap();
        assert!(save(&initial, &initial.bytes).is_err());
        assert!(!initial.chosen_path.exists());
        assert_no_temporaries(directory.path());
    }

    #[test]
    fn symlink_is_kept_and_retarget_is_detected() {
        let directory = tempdir().unwrap();
        let initial = fixture(&directory);
        let link = directory.path().join("linked.env");
        symlink(&initial.resolved_path, &link).unwrap();
        let linked = open(link.clone()).unwrap();
        let saved = save(&linked, b"VALUE=linked\n").unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(saved.chosen_path, link);
        assert_eq!(fs::read(&initial.resolved_path).unwrap(), b"VALUE=linked\n");
        let other = directory.path().join("other.env");
        fs::write(&other, b"OTHER=synthetic\n").unwrap();
        fs::remove_file(&link).unwrap();
        symlink(&other, &link).unwrap();
        assert!(matches!(
            save(&saved, b"VALUE=ours\n"),
            Err(StorageError::Conflict)
        ));
        assert_eq!(fs::read(&other).unwrap(), b"OTHER=synthetic\n");
        assert_no_temporaries(directory.path());
    }

    #[test]
    fn hardlink_original_is_blocked_and_save_as_preserves_both_links() {
        let directory = tempdir().unwrap();
        let initial = fixture(&directory);
        let alias = directory.path().join("alias.env");
        fs::hard_link(&initial.resolved_path, &alias).unwrap();
        let linked = open(initial.chosen_path.clone()).unwrap();
        assert!(linked.is_hardlinked());
        assert!(matches!(
            save(&linked, b"VALUE=ours\n"),
            Err(StorageError::Hardlink)
        ));
        let recovery = directory.path().join("copy.env");
        let copied = save_as(recovery, b"VALUE=ours\n").unwrap();
        assert!(!copied.is_hardlinked());
        assert_eq!(fs::read(alias).unwrap(), initial.bytes);
        assert_eq!(fs::read(&initial.resolved_path).unwrap(), initial.bytes);
    }

    #[test]
    fn readonly_original_cannot_be_bypassed_with_atomic_rename() {
        let directory = tempdir().unwrap();
        let initial = fixture(&directory);
        fs::set_permissions(&initial.chosen_path, fs::Permissions::from_mode(0o400)).unwrap();
        let readonly = open(initial.chosen_path.clone()).unwrap();
        assert!(matches!(
            save(&readonly, b"VALUE=ours\n"),
            Err(StorageError::Io(_))
        ));
        assert_eq!(fs::read(&initial.chosen_path).unwrap(), initial.bytes);
        assert_no_temporaries(directory.path());
    }

    #[test]
    fn save_as_and_create_empty_are_exclusive_and_private() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("new.env");
        let empty = create_empty(path.clone()).unwrap();
        assert!(empty.bytes.is_empty());
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert!(matches!(
            save_as(path.clone(), b"VALUE=ours\n"),
            Err(StorageError::AlreadyExists)
        ));
        assert!(matches!(
            create_empty(path.clone()),
            Err(StorageError::AlreadyExists)
        ));
        assert_eq!(fs::read(path).unwrap(), b"");
        let dangling = directory.path().join("dangling.env");
        symlink(directory.path().join("missing"), &dangling).unwrap();
        assert!(matches!(
            save_as(dangling.clone(), b"VALUE=ours\n"),
            Err(StorageError::AlreadyExists)
        ));
        assert!(
            fs::symlink_metadata(dangling)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_no_temporaries(directory.path());
    }

    #[test]
    fn injected_pre_replace_failures_leave_original_and_clean_temporaries() {
        for fail in [
            Stage::Write,
            Stage::Sync,
            Stage::Metadata,
            Stage::Revalidate,
            Stage::Replace,
        ] {
            let directory = tempdir().unwrap();
            let initial = fixture(&directory);
            let result = save_with_hook(&initial, b"VALUE=ours\n", &mut |stage, path| {
                if stage == Stage::Write {
                    assert_eq!(fs::metadata(path)?.mode() & 0o777, 0o600);
                }
                if stage == fail {
                    Err(io::Error::other("injected failure"))
                } else {
                    Ok(())
                }
            });
            assert!(matches!(result, Err(StorageError::Io(_))), "{fail:?}");
            assert_eq!(fs::read(&initial.chosen_path).unwrap(), initial.bytes);
            assert_no_temporaries(directory.path());
        }
    }

    #[test]
    fn changes_immediately_before_replace_are_revalidated() {
        let directory = tempdir().unwrap();
        let initial = fixture(&directory);
        let result = save_with_hook(&initial, b"VALUE=ours\n", &mut |stage, _| {
            if stage == Stage::Revalidate {
                fs::write(&initial.chosen_path, b"VALUE=external\n")?;
            }
            Ok(())
        });
        assert!(matches!(result, Err(StorageError::Conflict)));
        assert_eq!(fs::read(&initial.chosen_path).unwrap(), b"VALUE=external\n");
        assert_no_temporaries(directory.path());
    }

    #[test]
    fn post_replace_failures_are_uncertain_and_do_not_claim_rollback() {
        for fail in [Stage::DirectorySync, Stage::ReadBack] {
            let directory = tempdir().unwrap();
            let initial = fixture(&directory);
            let snapshot = b"VALUE=ours\n";
            let result = save_with_hook(&initial, snapshot, &mut |stage, _| {
                if stage == fail {
                    Err(io::Error::other("injected failure"))
                } else {
                    Ok(())
                }
            });
            assert!(matches!(result, Err(StorageError::Uncertain)));
            assert_eq!(fs::read(&initial.chosen_path).unwrap(), snapshot);
            assert_eq!(initial.bytes, b"# synthetic\r\nVALUE=before\r\n");
            assert_no_temporaries(directory.path());
        }
    }

    #[test]
    fn readback_mismatch_is_uncertain_and_never_returns_false_success() {
        let directory = tempdir().unwrap();
        let initial = fixture(&directory);
        let result = save_with_hook(&initial, b"VALUE=ours\n", &mut |stage, path| {
            if stage == Stage::ReadBack {
                fs::write(path, b"VALUE=external\n")?;
            }
            Ok(())
        });
        assert!(matches!(result, Err(StorageError::Uncertain)));
        assert_eq!(fs::read(&initial.chosen_path).unwrap(), b"VALUE=external\n");
        assert_no_temporaries(directory.path());
    }

    #[test]
    fn post_replace_access_changes_are_not_reported_as_verified_success() {
        for attribute in [false, true] {
            let directory = tempdir().unwrap();
            let initial = fixture(&directory);
            let result = save_with_hook(&initial, b"VALUE=ours\n", &mut |stage, path| {
                if stage == Stage::ReadBack {
                    if attribute {
                        let status = Command::new("/usr/bin/xattr")
                            .args(["-w", "local.envapp.synthetic", "synthetic"])
                            .arg(path)
                            .status()?;
                        assert!(status.success());
                    } else {
                        fs::set_permissions(path, fs::Permissions::from_mode(0o644))?;
                    }
                }
                Ok(())
            });
            assert!(matches!(result, Err(StorageError::Uncertain)));
            assert_eq!(fs::read(&initial.chosen_path).unwrap(), b"VALUE=ours\n");
            assert_no_temporaries(directory.path());
        }
    }

    #[test]
    fn save_as_detects_a_late_existing_target_without_overwriting() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("new.env");
        let result = save_as_with_hook(target.clone(), b"VALUE=ours\n", &mut |stage, _| {
            if stage == Stage::Replace {
                fs::write(&target, b"VALUE=external\n")?;
            }
            Ok(())
        });
        assert!(matches!(result, Err(StorageError::AlreadyExists)));
        assert_eq!(fs::read(target).unwrap(), b"VALUE=external\n");
        assert_no_temporaries(directory.path());
    }

    #[test]
    fn save_as_cannot_accept_broader_permissions_before_or_after_creation() {
        for stage_to_change in [Stage::Replace, Stage::DirectorySync] {
            let directory = tempdir().unwrap();
            let target = directory.path().join("new.env");
            let result = save_as_with_hook(
                target.clone(),
                b"VALUE=synthetic\n",
                &mut |stage, temporary| {
                    if stage == stage_to_change {
                        let changed = if stage == Stage::Replace {
                            temporary
                        } else {
                            &target
                        };
                        fs::set_permissions(changed, fs::Permissions::from_mode(0o644))?;
                    }
                    Ok(())
                },
            );
            if stage_to_change == Stage::Replace {
                assert!(matches!(
                    result,
                    Err(StorageError::UnsupportedAccessMetadata)
                ));
                assert!(!target.exists());
            } else {
                assert!(matches!(result, Err(StorageError::Uncertain)));
            }
            assert_no_temporaries(directory.path());
        }
    }

    #[test]
    fn unsupported_acl_and_xattrs_block_original_replacement() {
        for acl in [false, true] {
            let directory = tempdir().unwrap();
            let initial = fixture(&directory);
            let status = if acl {
                Command::new("/bin/chmod")
                    .arg("+a")
                    .arg("everyone allow read")
                    .arg(&initial.chosen_path)
                    .status()
                    .unwrap()
            } else {
                Command::new("/usr/bin/xattr")
                    .args(["-w", "local.envapp.synthetic", "synthetic"])
                    .arg(&initial.chosen_path)
                    .status()
                    .unwrap()
            };
            assert!(status.success());
            let attributed = open(initial.chosen_path.clone()).unwrap();
            assert!(attributed.has_unsupported_access_metadata());
            assert!(matches!(
                save(&attributed, b"VALUE=ours\n"),
                Err(StorageError::UnsupportedAccessMetadata)
            ));
            assert_eq!(fs::read(&initial.chosen_path).unwrap(), initial.bytes);
            assert_no_temporaries(directory.path());
        }
    }

    #[test]
    fn inherited_acl_cannot_make_new_temporary_broadly_accessible() {
        let directory = tempdir().unwrap();
        assert!(
            Command::new("/bin/chmod")
                .arg("+a")
                .arg("everyone allow read,file_inherit,directory_inherit")
                .arg(directory.path())
                .status()
                .unwrap()
                .success()
        );
        let result = save_as(directory.path().join("new.env"), b"VALUE=ours\n");
        assert!(matches!(
            result,
            Err(StorageError::UnsupportedAccessMetadata)
        ));
        assert!(!directory.path().join("new.env").exists());
        assert_no_temporaries(directory.path());
    }

    #[test]
    fn invalid_path_and_directory_are_not_documents() {
        let directory = tempdir().unwrap();
        assert!(matches!(
            open(directory.path().to_path_buf()),
            Err(StorageError::InvalidPath)
        ));
        assert!(open(directory.path().join("missing.env")).is_err());
        assert!(save_as(directory.path().join("missing/new.env"), b"VALUE=ours\n").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn physical_identity_unifies_aliases_but_reference_keeps_chosen_path() {
        let directory = tempdir().unwrap();
        let original = fixture(&directory);
        let symlink = directory.path().join("alias.env");
        let hardlink = directory.path().join("hardlink.env");
        std::os::unix::fs::symlink(&original.chosen_path, &symlink).unwrap();
        std::fs::hard_link(&original.chosen_path, &hardlink).unwrap();
        let current = open(original.chosen_path.clone()).unwrap();
        let symlink = open(symlink).unwrap();
        let hardlink = open(hardlink).unwrap();
        assert_eq!(current.identity(), symlink.identity());
        assert_eq!(current.identity(), hardlink.identity());
        assert!(!current.same_reference(&symlink));
        assert!(!current.same_reference(&hardlink));
        assert!(current.same_reference(&open(current.chosen_path.clone()).unwrap()));

        let copy = directory.path().join("copy.env");
        std::fs::write(&copy, &current.bytes).unwrap();
        let copy = open(copy).unwrap();
        assert_eq!(current.bytes, copy.bytes);
        assert_ne!(current.identity(), copy.identity());
    }

    #[cfg(unix)]
    #[test]
    fn alias_validation_accepts_paths_to_the_same_physical_document() {
        let directory = tempdir().unwrap();
        let original = fixture(&directory);
        let symlink = directory.path().join("alias.env");
        let hardlink = directory.path().join("hardlink.env");
        std::os::unix::fs::symlink(&original.chosen_path, &symlink).unwrap();
        std::fs::hard_link(&original.chosen_path, &hardlink).unwrap();
        let baseline = open(original.chosen_path.clone()).unwrap();
        validate_aliases(&baseline, &[original.chosen_path, symlink, hardlink]).unwrap();
        assert_no_temporaries(directory.path());
    }

    #[cfg(unix)]
    #[test]
    fn retargeted_secondary_alias_conflicts_even_when_content_is_identical() {
        for same_bytes in [false, true] {
            let directory = tempdir().unwrap();
            let original = fixture(&directory);
            let alias = directory.path().join("alias.env");
            std::os::unix::fs::symlink(&original.chosen_path, &alias).unwrap();
            let other = directory.path().join("other.env");
            let bytes = if same_bytes {
                original.bytes.as_slice()
            } else {
                b"EXTERNAL=other\n"
            };
            std::fs::write(&other, bytes).unwrap();
            std::fs::remove_file(&alias).unwrap();
            std::os::unix::fs::symlink(&other, &alias).unwrap();
            assert!(matches!(
                validate_aliases(&original, &[alias]),
                Err(StorageError::Conflict)
            ));
            assert_eq!(
                std::fs::read(&original.chosen_path).unwrap(),
                original.bytes
            );
            assert_eq!(std::fs::read(&other).unwrap(), bytes);
            assert_no_temporaries(directory.path());
        }
    }

    #[cfg(unix)]
    #[test]
    fn deleted_secondary_alias_fails_and_primary_is_still_checked_by_save() {
        let directory = tempdir().unwrap();
        let original = fixture(&directory);
        let alias = directory.path().join("alias.env");
        std::os::unix::fs::symlink(&original.chosen_path, &alias).unwrap();
        std::fs::remove_file(&alias).unwrap();
        assert!(matches!(
            validate_aliases(&original, &[alias]),
            Err(StorageError::Io(error)) if error.kind() == io::ErrorKind::NotFound
        ));
        std::fs::write(&original.chosen_path, b"EXTERNAL=changed\n").unwrap();
        validate_aliases(&original, std::slice::from_ref(&original.chosen_path)).unwrap();
        assert!(matches!(
            save(&original, b"VALUE=ours\n"),
            Err(StorageError::Conflict)
        ));
        assert_eq!(
            std::fs::read(&original.chosen_path).unwrap(),
            b"EXTERNAL=changed\n"
        );
        assert_no_temporaries(directory.path());
    }
}

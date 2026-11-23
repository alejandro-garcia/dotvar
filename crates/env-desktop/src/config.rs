//! Small, versioned catalog of references only. Never serialize document sessions.
//!
//! Call blocking I/O on the background executor. An invalid existing catalog is
//! kept intact; only an explicit reset may replace it. All diagnostics are fixed
//! strings, without parser snippets, document values, or local paths.

use crate::i18n::tr;
#[cfg(any(feature = "native", test))]
use crate::{i18n::LanguagePreference, session::QuoteStyle};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
use tempfile::Builder;

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

pub const SCHEMA_VERSION: u32 = 1;
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(PartialEq, Eq)]
struct OversizedReference {
    len: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl OversizedReference {
    fn read(path: &Path) -> Result<Self, ConfigError> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.len() <= MAX_BYTES {
            return Err(ConfigError::Invalid);
        }
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            identity: (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub projects: Vec<ProjectConfig>,
    /// No selection while the welcome screen is open, even with recent projects.
    pub active_project: Option<u64>,
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            projects: Vec::new(),
            active_project: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub id: u64,
    pub root: PathBuf,
    /// Ordered open references, distinct from newly discovered available files.
    pub tabs: Vec<PathBuf>,
    pub active_tab: Option<PathBuf>,
}

impl Catalog {
    /// Structural validation never accesses a project or `.env` file. Missing or
    /// moved roots and references remain available for locating/removing in UI.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.version != SCHEMA_VERSION || self.projects.len() > 1000 {
            return Err(ConfigError::Invalid);
        }
        let mut ids = HashSet::new();
        let mut roots = HashSet::new();
        let mut count = 0usize;
        for project in &self.projects {
            if project.id == 0
                || !ids.insert(project.id)
                || !valid_path(&project.root)
                || !roots.insert(&project.root)
            {
                return Err(ConfigError::Invalid);
            }
            count = count.saturating_add(project.tabs.len());
            if count > 10_000 {
                return Err(ConfigError::Invalid);
            }
            let mut tabs = HashSet::new();
            for tab in &project.tabs {
                if !valid_path(tab) || !tabs.insert(tab) {
                    return Err(ConfigError::Invalid);
                }
            }
            if project
                .active_tab
                .as_ref()
                .is_some_and(|active| !tabs.contains(active))
                || (project.active_tab.is_none() && !project.tabs.is_empty())
            {
                return Err(ConfigError::Invalid);
            }
        }
        if self
            .active_project
            .is_some_and(|active| !ids.contains(&active))
        {
            return Err(ConfigError::Invalid);
        }
        Ok(())
    }
}

fn valid_path(path: &Path) -> bool {
    path.is_absolute()
        && path
            .to_str()
            .is_some_and(|path| path.len() <= 16_384 && !path.contains('\0') && !path.is_empty())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    Invalid,
    Io,
    Conflict,
    Uncertain,
    Unavailable,
}

impl ConfigError {
    pub fn message(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Invalid => {
                tr!("catalog.invalid")
            }
            Self::Io => {
                tr!("catalog.io")
            }
            Self::Conflict => tr!("catalog.conflict"),
            Self::Uncertain => tr!("catalog.uncertain"),
            Self::Unavailable => tr!("catalog.unavailable"),
        }
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for ConfigError {}

impl From<io::Error> for ConfigError {
    fn from(_: io::Error) -> Self {
        Self::Io
    }
}

#[derive(Clone, Debug)]
pub struct ConfigStore {
    directory: PathBuf,
}

/// Prefer the current name while retaining existing development/QA overrides.
pub(crate) fn app_env(suffix: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(format!("DOTVAR_{suffix}"))
        .or_else(|| std::env::var_os(format!("ENV_APP_{suffix}")))
}

/// Operating-system family that decides where per-user configuration lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfigPlatform {
    Mac,
    Windows,
    /// Linux and other XDG-style systems.
    Unix,
}

impl ConfigPlatform {
    pub(crate) const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::Mac
    } else if cfg!(windows) {
        Self::Windows
    } else {
        Self::Unix
    };
}

/// Environment values consulted to locate configuration. Injected so that
/// every platform's resolution is testable on any host.
#[derive(Clone, Debug, Default)]
pub(crate) struct ConfigEnv {
    pub(crate) config_dir: Option<std::ffi::OsString>,
    pub(crate) home: Option<std::ffi::OsString>,
    pub(crate) userprofile: Option<std::ffi::OsString>,
    pub(crate) appdata: Option<std::ffi::OsString>,
    pub(crate) xdg_config_home: Option<std::ffi::OsString>,
}

impl ConfigEnv {
    fn from_process() -> Self {
        Self {
            config_dir: app_env("CONFIG_DIR"),
            home: std::env::var_os("HOME"),
            userprofile: std::env::var_os("USERPROFILE"),
            appdata: std::env::var_os("APPDATA"),
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME"),
        }
    }
}

/// An unset or empty variable is treated as absent.
fn non_empty(value: &Option<std::ffi::OsString>) -> Option<PathBuf> {
    value
        .as_ref()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Windows uses the profile directory; every other platform uses HOME.
pub(crate) fn home_directory_from(platform: ConfigPlatform, env: &ConfigEnv) -> Option<PathBuf> {
    let home = non_empty(&env.home);
    if platform == ConfigPlatform::Windows {
        non_empty(&env.userprofile).or(home)
    } else {
        home
    }
}

#[cfg(feature = "native")]
pub(crate) fn home_directory() -> Option<PathBuf> {
    home_directory_from(ConfigPlatform::CURRENT, &ConfigEnv::from_process())
}

/// `DOTVAR_CONFIG_DIR` wins everywhere. Otherwise: macOS keeps
/// `~/Library/Application Support`, Windows uses `%APPDATA%\Dotvar` and
/// Linux uses an absolute `$XDG_CONFIG_HOME/dotvar` or `~/.config/dotvar`.
fn resolve_directory(platform: ConfigPlatform, env: &ConfigEnv) -> Result<PathBuf, ConfigError> {
    let directory = if let Some(directory) = &env.config_dir {
        PathBuf::from(directory)
    } else {
        let home = home_directory_from(platform, env);
        match platform {
            ConfigPlatform::Mac => default_directory(&home.ok_or(ConfigError::Unavailable)?)?,
            ConfigPlatform::Windows => match non_empty(&env.appdata) {
                Some(appdata) => appdata.join("Dotvar"),
                None => home
                    .ok_or(ConfigError::Unavailable)?
                    .join("AppData")
                    .join("Roaming")
                    .join("Dotvar"),
            },
            ConfigPlatform::Unix => {
                match non_empty(&env.xdg_config_home).filter(|directory| directory.is_absolute()) {
                    Some(config) => config.join("dotvar"),
                    None => home
                        .ok_or(ConfigError::Unavailable)?
                        .join(".config")
                        .join("dotvar"),
                }
            }
        }
    };
    if directory.is_absolute() {
        Ok(directory)
    } else {
        Err(ConfigError::Unavailable)
    }
}

fn default_directory(home: &Path) -> Result<PathBuf, ConfigError> {
    let support = home.join("Library/Application Support");
    let current = support.join("Dotvar");
    let previous = support.join("Env");
    // Reuse the previous catalog in place so a rename never loses a session.
    if !current.try_exists()? && previous.try_exists()? {
        Ok(previous)
    } else {
        Ok(current)
    }
}

impl ConfigStore {
    pub fn from_directory(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// QA selects its own explicit directory. The environment is only read, never changed.
    pub fn platform_default() -> Result<Self, ConfigError> {
        let directory = resolve_directory(ConfigPlatform::CURRENT, &ConfigEnv::from_process())?;
        Ok(Self { directory })
    }

    pub fn path(&self) -> PathBuf {
        self.directory.join("catalog.json")
    }

    /// Absence is an empty catalog. Invalid/unsupported JSON remains on disk.
    pub fn load(&self) -> Result<Catalog, ConfigError> {
        match Self::read_existing(&self.path())? {
            Some(bytes) => parse(&bytes),
            None => Ok(Catalog::default()),
        }
    }

    #[cfg(any(feature = "native", test))]
    pub(crate) fn load_language(&self) -> Result<LanguagePreference, ConfigError> {
        self.load_preference("language.json")
    }

    #[cfg(any(feature = "native", test))]
    pub(crate) fn save_language(&self, language: LanguagePreference) -> Result<(), ConfigError> {
        self.save_preference("language.json", language)
    }

    #[cfg(any(feature = "native", test))]
    pub(crate) fn load_quote_style(&self) -> Result<QuoteStyle, ConfigError> {
        self.load_preference("quote-style.json")
    }

    #[cfg(any(feature = "native", test))]
    pub(crate) fn save_quote_style(&self, style: QuoteStyle) -> Result<(), ConfigError> {
        self.save_preference("quote-style.json", style)
    }

    #[cfg(any(feature = "native", test))]
    fn load_preference<T: Default + serde::de::DeserializeOwned>(
        &self,
        filename: &str,
    ) -> Result<T, ConfigError> {
        match Self::read_existing(&self.directory.join(filename))? {
            Some(bytes) => serde_json::from_slice(&bytes).map_err(|_| ConfigError::Invalid),
            None => Ok(T::default()),
        }
    }

    #[cfg(any(feature = "native", test))]
    fn save_preference(&self, filename: &str, value: impl Serialize) -> Result<(), ConfigError> {
        let path = self.directory.join(filename);
        let before = Self::read_existing(&path)?;
        let mut bytes = serde_json::to_vec(&value).map_err(|_| ConfigError::Invalid)?;
        bytes.push(b'\n');
        self.write_file(&path, &bytes, before.as_deref(), None)
    }

    /// Rejects an invalid existing catalog, even if a caller ignored load error.
    pub fn save(&self, catalog: &Catalog) -> Result<(), ConfigError> {
        self.write(catalog, false)
    }

    /// The caller must expose this as an explicit reset action after informing
    /// the user that the invalid catalog will be replaced. No `.env` is touched.
    pub fn reset(&self, catalog: &Catalog) -> Result<(), ConfigError> {
        self.write(catalog, true)
    }

    fn read_existing(path: &Path) -> Result<Option<Vec<u8>>, ConfigError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ConfigError::Io),
        };
        if !metadata.is_file() || metadata.len() > MAX_BYTES {
            return Err(ConfigError::Invalid);
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(ConfigError::Invalid);
        }
        Ok(Some(bytes))
    }

    fn write(&self, catalog: &Catalog, reset: bool) -> Result<(), ConfigError> {
        catalog.validate()?;
        let path = self.path();
        // An explicit reset may replace a corrupt oversized regular file
        // without loading arbitrary amounts of invalid data into memory. A
        // symlink/directory is never treated as a catalog, including on reset.
        let (before, oversized) = match Self::read_existing(&path) {
            Ok(before) => (before, None),
            Err(ConfigError::Invalid) if reset => (None, Some(OversizedReference::read(&path)?)),
            Err(error) => return Err(error),
        };
        if !reset && let Some(bytes) = &before {
            parse(bytes)?;
        }
        let mut bytes = serde_json::to_vec_pretty(catalog).map_err(|_| ConfigError::Invalid)?;
        bytes.push(b'\n');
        if bytes.len() as u64 > MAX_BYTES {
            return Err(ConfigError::Invalid);
        }
        self.write_file(&path, &bytes, before.as_deref(), oversized.as_ref())
    }

    fn write_file(
        &self,
        path: &Path,
        bytes: &[u8],
        before: Option<&[u8]>,
        oversized: Option<&OversizedReference>,
    ) -> Result<(), ConfigError> {
        if before == Some(bytes) {
            return Ok(());
        }
        let mut directory = fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        directory.mode(0o700);
        directory.create(&self.directory)?;
        let mut builder = Builder::new();
        builder.prefix(".dotvar-");
        #[cfg(unix)]
        builder.permissions(fs::Permissions::from_mode(0o600));
        let mut temporary = builder.tempfile_in(&self.directory)?;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        if let Some(expected) = oversized {
            if OversizedReference::read(path).as_ref() != Ok(expected) {
                return Err(ConfigError::Conflict);
            }
        } else if Self::read_existing(path)?.as_deref() != before {
            return Err(ConfigError::Conflict);
        }
        let persisted = if before.is_some() || oversized.is_some() {
            temporary.persist(path)
        } else {
            temporary.persist_noclobber(path)
        };
        match persisted {
            Ok(_) => {}
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(ConfigError::Conflict);
            }
            Err(_) => return Err(ConfigError::Io),
        }
        // Replacement is already visible. Subsequent failures are uncertain,
        // rather than falsely claiming that the previous catalog is intact.
        sync_directory(&self.directory).map_err(|_| ConfigError::Uncertain)?;
        match Self::read_existing(path) {
            Ok(Some(actual)) if actual == bytes => Ok(()),
            _ => Err(ConfigError::Uncertain),
        }
    }
}

/// Makes the rename durable. Windows cannot open a directory with `File::open`
/// (it fails with access denied even though the replacement succeeded), and NTFS
/// journals the rename itself, so there is nothing further to flush there.
fn sync_directory(directory: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        let _ = directory;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        File::open(directory)?.sync_all()
    }
}

fn parse(bytes: &[u8]) -> Result<Catalog, ConfigError> {
    let catalog: Catalog = serde_json::from_slice(bytes).map_err(|_| ConfigError::Invalid)?;
    catalog.validate()?;
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_defaults_to_english_and_persists_independently_of_the_catalog() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::from_directory(directory.path().to_owned());
        assert_eq!(store.load_language(), Ok(LanguagePreference::English));

        let catalog = b"{incomplete catalog";
        fs::write(store.path(), catalog).unwrap();
        for language in [
            LanguagePreference::English,
            LanguagePreference::Spanish,
            LanguagePreference::System,
        ] {
            store.save_language(language).unwrap();
            let reopened = ConfigStore::from_directory(directory.path().to_owned());
            assert_eq!(reopened.load_language(), Ok(language));
        }
        assert_eq!(fs::read(store.path()).unwrap(), catalog);
        store.reset(&Catalog::default()).unwrap();
        assert_eq!(store.load_language(), Ok(LanguagePreference::System));
    }
    use tempfile::tempdir;

    #[test]
    fn quote_style_defaults_to_double_and_persists_alongside_language() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::from_directory(directory.path().to_owned());
        store.save_language(LanguagePreference::Spanish).unwrap();
        assert_eq!(store.load_quote_style(), Ok(QuoteStyle::Double));

        for style in [QuoteStyle::Single, QuoteStyle::Unquoted, QuoteStyle::Double] {
            store.save_quote_style(style).unwrap();
            let reopened = ConfigStore::from_directory(directory.path().to_owned());
            assert_eq!(reopened.load_quote_style(), Ok(style));
            assert_eq!(reopened.load_language(), Ok(LanguagePreference::Spanish));
        }
    }

    #[test]
    fn renamed_app_retains_existing_catalog_and_prefers_dotvar_when_present() {
        let home = tempdir().unwrap();
        let support = home.path().join("Library/Application Support");
        let current = support.join("Dotvar");
        let previous = support.join("Env");
        assert_eq!(default_directory(home.path()).unwrap(), current);

        let old_store = ConfigStore::from_directory(previous.clone());
        let expected = catalog(home.path());
        old_store.save(&expected).unwrap();
        let selected = ConfigStore::from_directory(default_directory(home.path()).unwrap());
        assert_eq!(selected.path(), old_store.path());
        assert_eq!(selected.load().unwrap(), expected);

        fs::create_dir(&current).unwrap();
        assert_eq!(default_directory(home.path()).unwrap(), current);
        assert_eq!(old_store.load().unwrap(), expected);
    }

    fn catalog(directory: &Path) -> Catalog {
        let root = directory.join("moved-project");
        let tab = root.join(".env.local");
        Catalog {
            projects: vec![ProjectConfig {
                id: 42,
                root,
                tabs: vec![tab.clone()],
                active_tab: Some(tab),
            }],
            active_project: Some(42),
            ..Catalog::default()
        }
    }

    #[test]
    fn only_reference_fields_are_serialized_and_missing_projects_are_retained() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::from_directory(directory.path().join("config"));
        assert_eq!(store.load().unwrap(), Catalog::default());
        let expected = catalog(directory.path());
        store.save(&expected).unwrap();
        assert_eq!(store.load().unwrap(), expected);
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        let top = value.as_object().unwrap();
        assert_eq!(top.len(), 3);
        let project = value["projects"][0].as_object().unwrap();
        assert_eq!(project.len(), 4);
        assert!(project.contains_key("tabs"));
        assert!(!expected.projects[0].root.exists());
    }

    #[test]
    fn invalid_unknown_and_future_catalogs_are_kept_until_explicit_reset() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::from_directory(directory.path().to_path_buf());
        let future = Catalog {
            version: SCHEMA_VERSION + 1,
            ..Catalog::default()
        };
        for original in [
            b"{not-json synthetic-secret-token".to_vec(),
            br#"{"version":1,"projects":[],"active_project":null,"values":"synthetic-secret-token"}"#.to_vec(),
            serde_json::to_vec(&future).unwrap(),
        ] {
            fs::write(store.path(), &original).unwrap();
            assert_eq!(store.load().unwrap_err(), ConfigError::Invalid);
            assert_eq!(store.save(&Catalog::default()), Err(ConfigError::Invalid));
            assert_eq!(fs::read(store.path()).unwrap(), original);
            let diagnostic = format!("{} {:?}", ConfigError::Invalid, ConfigError::Invalid);
            assert!(!diagnostic.contains("synthetic-secret-token"));
            store.reset(&Catalog::default()).unwrap();
            assert_eq!(store.load().unwrap(), Catalog::default());
        }
    }

    #[test]
    fn tab_order_selection_and_closed_empty_sessions_round_trip() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::from_directory(directory.path().join("config"));
        let mut expected = catalog(directory.path());
        let project = &mut expected.projects[0];
        project.tabs.insert(0, project.root.join(".env.production"));
        project.tabs.push(project.root.join(".env"));
        store.save(&expected).unwrap();
        assert_eq!(store.load().unwrap(), expected);
        expected.projects[0].tabs.clear();
        expected.projects[0].active_tab = None;
        store.save(&expected).unwrap();
        assert_eq!(store.load().unwrap(), expected);
    }

    #[test]
    fn invalid_ids_relative_paths_and_duplicate_tabs_are_rejected() {
        let directory = tempdir().unwrap();
        let expected = catalog(directory.path());
        let mut invalid = expected.clone();
        invalid.projects.push(invalid.projects[0].clone());
        assert_eq!(invalid.validate(), Err(ConfigError::Invalid));
        let mut invalid = expected.clone();
        invalid.projects[0].root = PathBuf::from("relative");
        assert_eq!(invalid.validate(), Err(ConfigError::Invalid));
        let mut invalid = expected.clone();
        let duplicate = invalid.projects[0].tabs[0].clone();
        invalid.projects[0].tabs.push(duplicate);
        assert_eq!(invalid.validate(), Err(ConfigError::Invalid));
        let mut invalid = expected;
        invalid.active_project = Some(99);
        assert_eq!(invalid.validate(), Err(ConfigError::Invalid));
    }

    #[test]
    fn welcome_screen_selection_is_preserved_with_recent_projects() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::from_directory(directory.path().join("config"));
        let mut expected = catalog(directory.path());
        expected.active_project = None;
        store.save(&expected).unwrap();
        assert_eq!(store.load().unwrap(), expected);
    }

    #[test]
    fn save_is_private_and_identical_save_does_not_replace_file() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::from_directory(directory.path().join("config"));
        let expected = catalog(directory.path());
        store.save(&expected).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let before = fs::metadata(store.path()).unwrap();
            assert_eq!(before.permissions().mode() & 0o777, 0o600);
            assert_eq!(
                fs::metadata(&store.directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
            store.save(&expected).unwrap();
            let after = fs::metadata(store.path()).unwrap();
            assert_eq!(after.ino(), before.ino());
            assert_eq!(after.modified().unwrap(), before.modified().unwrap());
        }
        assert_eq!(fs::read_dir(&store.directory).unwrap().count(), 1);
    }

    #[test]
    fn oversized_and_symlink_catalogs_are_rejected_without_following() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::from_directory(directory.path().join("config"));
        fs::create_dir(&store.directory).unwrap();
        // The handle is closed before the reset replaces the file: Windows refuses to
        // replace a file that is still open.
        File::create(store.path())
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        assert_eq!(store.load().unwrap_err(), ConfigError::Invalid);
        assert_eq!(store.save(&Catalog::default()), Err(ConfigError::Invalid));
        assert_eq!(fs::metadata(store.path()).unwrap().len(), MAX_BYTES + 1);
        store.reset(&Catalog::default()).unwrap();
        assert_eq!(store.load().unwrap(), Catalog::default());
        #[cfg(unix)]
        {
            let outside = directory.path().join("outside.json");
            fs::write(&outside, b"synthetic-reference-only").unwrap();
            fs::remove_file(store.path()).unwrap();
            std::os::unix::fs::symlink(&outside, store.path()).unwrap();
            assert_eq!(store.load().unwrap_err(), ConfigError::Invalid);
            assert_eq!(store.reset(&Catalog::default()), Err(ConfigError::Invalid));
            assert_eq!(fs::read(outside).unwrap(), b"synthetic-reference-only");
        }
    }

    fn env(entries: &[(&str, &Path)]) -> ConfigEnv {
        let mut env = ConfigEnv::default();
        for (name, value) in entries {
            let value = Some(value.as_os_str().to_owned());
            match *name {
                "config_dir" => env.config_dir = value,
                "home" => env.home = value,
                "userprofile" => env.userprofile = value,
                "appdata" => env.appdata = value,
                "xdg" => env.xdg_config_home = value,
                _ => unreachable!(),
            }
        }
        env
    }

    #[test]
    fn windows_directory_is_dotvar_under_appdata() {
        let base = tempdir().unwrap();
        let appdata = base.path().join("Roaming");
        let profile = base.path().join("profile");
        let resolved = resolve_directory(
            ConfigPlatform::Windows,
            &env(&[("appdata", &appdata), ("userprofile", &profile)]),
        );
        assert_eq!(resolved, Ok(appdata.join("Dotvar")));
        // Without APPDATA the standard Roaming location under the profile is used.
        let resolved =
            resolve_directory(ConfigPlatform::Windows, &env(&[("userprofile", &profile)]));
        assert_eq!(
            resolved,
            Ok(profile.join("AppData").join("Roaming").join("Dotvar"))
        );
        assert_eq!(
            resolve_directory(ConfigPlatform::Windows, &ConfigEnv::default()),
            Err(ConfigError::Unavailable)
        );
    }

    #[test]
    fn unix_directory_follows_absolute_xdg_config_home_then_dot_config() {
        let base = tempdir().unwrap();
        let xdg = base.path().join("xdg");
        let home = base.path().join("home");
        assert_eq!(
            resolve_directory(
                ConfigPlatform::Unix,
                &env(&[("xdg", &xdg), ("home", &home)])
            ),
            Ok(xdg.join("dotvar"))
        );
        assert_eq!(
            resolve_directory(ConfigPlatform::Unix, &env(&[("home", &home)])),
            Ok(home.join(".config").join("dotvar"))
        );
        // A relative or empty XDG_CONFIG_HOME is ignored, as the spec requires.
        for ignored in [Path::new("relative/config"), Path::new("")] {
            assert_eq!(
                resolve_directory(
                    ConfigPlatform::Unix,
                    &env(&[("xdg", ignored), ("home", &home)])
                ),
                Ok(home.join(".config").join("dotvar"))
            );
        }
        assert_eq!(
            resolve_directory(
                ConfigPlatform::Unix,
                &env(&[("xdg", Path::new("relative"))])
            ),
            Err(ConfigError::Unavailable)
        );
    }

    #[test]
    fn explicit_config_directory_overrides_every_platform_and_must_be_absolute() {
        let base = tempdir().unwrap();
        let chosen = base.path().join("chosen");
        let other = base.path().join("other");
        for platform in [
            ConfigPlatform::Mac,
            ConfigPlatform::Windows,
            ConfigPlatform::Unix,
        ] {
            let all = env(&[
                ("config_dir", &chosen),
                ("home", &other),
                ("appdata", &other),
                ("xdg", &other),
            ]);
            assert_eq!(resolve_directory(platform, &all), Ok(chosen.clone()));
            assert_eq!(
                resolve_directory(platform, &env(&[("config_dir", Path::new("relative"))])),
                Err(ConfigError::Unavailable)
            );
        }
    }

    #[test]
    fn mac_directory_keeps_application_support_and_legacy_reuse() {
        let home = tempdir().unwrap();
        let support = home.path().join("Library/Application Support");
        let resolved =
            |home: &Path| resolve_directory(ConfigPlatform::Mac, &env(&[("home", home)]));
        assert_eq!(resolved(home.path()), Ok(support.join("Dotvar")));
        fs::create_dir_all(support.join("Env")).unwrap();
        assert_eq!(resolved(home.path()), Ok(support.join("Env")));
        assert_eq!(
            resolve_directory(ConfigPlatform::Mac, &ConfigEnv::default()),
            Err(ConfigError::Unavailable)
        );
    }

    #[test]
    fn home_directory_prefers_userprofile_only_on_windows() {
        let base = tempdir().unwrap();
        let home = base.path().join("home");
        let profile = base.path().join("profile");
        let both = env(&[("home", &home), ("userprofile", &profile)]);
        assert_eq!(
            home_directory_from(ConfigPlatform::Windows, &both),
            Some(profile)
        );
        assert_eq!(
            home_directory_from(ConfigPlatform::Unix, &both),
            Some(home.clone())
        );
        assert_eq!(
            home_directory_from(ConfigPlatform::Mac, &both),
            Some(home.clone())
        );
        assert_eq!(
            home_directory_from(ConfigPlatform::Windows, &env(&[("home", &home)])),
            Some(home)
        );
        assert_eq!(
            home_directory_from(ConfigPlatform::Windows, &ConfigEnv::default()),
            None
        );
        assert_eq!(
            home_directory_from(ConfigPlatform::Unix, &env(&[("home", Path::new(""))])),
            None
        );
    }
}

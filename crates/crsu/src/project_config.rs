use crate::git_repository::Repository;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CURRENT_SCHEMA_VERSION: u8 = 2;

#[derive(Debug, Deserialize, Serialize)]
pub struct ProjectConfig {
    pub schema_version: u8,
    pub crucible: CrucibleConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CrucibleConfig {
    pub url: String,
    pub token: String,
    pub project: String,
    pub repository: Option<String>,
    #[serde(default)]
    pub repository_location: Option<String>,
    #[serde(default)]
    pub reviewers: Vec<String>,
}

impl ProjectConfig {
    #[must_use]
    pub fn blank() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            crucible: CrucibleConfig {
                url: String::new(),
                token: String::new(),
                project: String::new(),
                repository: None,
                repository_location: None,
                reviewers: Vec::new(),
            },
        }
    }

    pub fn load() -> Result<Option<Self>, String> {
        load_from(&config_path()?)
    }

    pub fn load_global() -> Result<Option<Self>, String> {
        let Some(path) = global_config_path() else {
            return Ok(None);
        };
        load_from(&path)
    }

    /// Project file wins when present. Global never supplies a `FishEye` repository anchor.
    #[must_use]
    pub fn resolve(project: Option<Self>, global: Option<Self>) -> Option<Self> {
        match (project, global) {
            (Some(project), _) => Some(project),
            (None, Some(mut global)) => {
                global.unset_repository();
                Some(global)
            }
            (None, None) => None,
        }
    }

    pub fn load_resolved() -> Result<Option<Self>, String> {
        let project = match repository_config_path()? {
            Some(path) => load_from(&path)?,
            None => None,
        };
        Ok(Self::resolve(project, Self::load_global()?))
    }

    pub fn save(&self) -> Result<PathBuf, String> {
        self.write_to(&config_path()?)
    }

    pub fn save_global(&mut self) -> Result<PathBuf, String> {
        self.unset_repository();
        let path = global_config_path().ok_or_else(|| {
            "cannot determine global config path; set CRSU_CONFIG_HOME or a home directory"
                .to_owned()
        })?;
        self.write_to(&path)
    }

    fn write_to(&self, path: &Path) -> Result<PathBuf, String> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(format!(
                "cannot save schema version {}; current version is {CURRENT_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        let directory = path
            .parent()
            .ok_or_else(|| "invalid configuration path".to_owned())?;
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let contents = toml::to_string_pretty(self).map_err(|error| error.to_string())?;
        std::fs::write(path, contents).map_err(|error| error.to_string())?;
        set_private_permissions(path)?;
        Ok(path.to_path_buf())
    }

    pub fn set_url(&mut self, value: String) {
        self.crucible.url = value;
    }

    pub fn set_project(&mut self, value: String) {
        self.crucible.project = value;
    }

    pub fn set_repository(&mut self, value: String) {
        self.crucible.repository = Some(value);
        // A repository selected by name has not yet been matched to a remote.
        // Keeping the old location could reject an otherwise valid diff.
        self.crucible.repository_location = None;
    }

    pub fn unset_repository(&mut self) {
        self.crucible.repository = None;
        self.crucible.repository_location = None;
    }

    pub fn add_reviewer(&mut self, username: String) {
        if !self.crucible.reviewers.contains(&username) {
            self.crucible.reviewers.push(username);
        }
    }

    pub fn remove_reviewer(&mut self, username: &str) {
        self.crucible
            .reviewers
            .retain(|reviewer| reviewer != username);
    }
}

fn load_from(path: &Path) -> Result<Option<ProjectConfig>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    parse_and_migrate(&contents).map(Some)
}

/// Overrides [`global_crsu_dir`] for tests and custom installs.
pub const CONFIG_HOME_ENV: &str = "CRSU_CONFIG_HOME";

/// Platform user config directory for crsu (`directories::ProjectDirs`).
#[must_use]
pub fn global_crsu_dir() -> Option<PathBuf> {
    if let Some(directory) = std::env::var_os(CONFIG_HOME_ENV).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(directory));
    }
    let dest = directories::ProjectDirs::from("", "", "crsu")?
        .config_dir()
        .to_path_buf();
    if let Some(home) = directories::BaseDirs::new() {
        migrate_legacy_xdg_into(&home.home_dir().join(".config/crsu"), &dest);
    }
    Some(dest)
}

fn migrate_legacy_xdg_into(legacy: &Path, dest: &Path) {
    if dest == legacy || dest.exists() || !legacy.exists() {
        return;
    }
    if let Some(parent) = dest.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    if std::fs::rename(legacy, dest).is_ok() {
        return;
    }
    if copy_dir(legacy, dest).is_ok() {
        let _ = std::fs::remove_dir_all(legacy);
    }
}

fn copy_dir(source: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let to = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

#[must_use]
pub fn global_config_path() -> Option<PathBuf> {
    global_crsu_dir().map(|directory| directory.join("config.toml"))
}

fn parse_and_migrate(contents: &str) -> Result<ProjectConfig, String> {
    let mut config =
        toml::from_str::<ProjectConfig>(contents).map_err(|error| error.to_string())?;
    match config.schema_version {
        1 => {
            config.schema_version = CURRENT_SCHEMA_VERSION;
            config.crucible.repository_location = None;
            Ok(config)
        }
        CURRENT_SCHEMA_VERSION => Ok(config),
        version => Err(format!(
            "unsupported project configuration schema version {version}; this crsu supports up to {CURRENT_SCHEMA_VERSION}"
        )),
    }
}

/// `Ok(None)` outside a Git repository. Other discovery failures stay errors.
fn repository_config_path() -> Result<Option<PathBuf>, String> {
    match Repository::discover() {
        Ok(repository) => Ok(Some(repository.common_dir().join("crsu/config.toml"))),
        Err(crate::git_repository::Error::NotRepository) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn config_path() -> Result<PathBuf, String> {
    repository_config_path()?.ok_or_else(|| crate::git_repository::Error::NotRepository.to_string())
}

#[cfg(unix)]
fn set_private_permissions(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &std::path::Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CURRENT_SCHEMA_VERSION, CrucibleConfig, ProjectConfig, parse_and_migrate};
    #[test]
    fn config_is_explicit_and_versioned() {
        let config = ProjectConfig {
            schema_version: CURRENT_SCHEMA_VERSION,
            crucible: CrucibleConfig {
                url: "http://cru".to_owned(),
                token: "secret".to_owned(),
                project: "COMMON".to_owned(),
                repository: Some("common-git".to_owned()),
                repository_location: Some("ssh://git/common.git".to_owned()),
                reviewers: vec!["reviewer".to_owned()],
            },
        };
        let text = toml::to_string_pretty(&config).expect("serialize config");
        assert!(text.contains("schema_version = 2"));
        assert!(text.contains("project = \"COMMON\""));
    }

    #[test]
    fn migrates_version_one_without_inventing_repository_location() {
        let config = parse_and_migrate(
            r#"
schema_version = 1

[crucible]
url = "http://cru"
token = "secret"
project = "COMMON"
repository = "logriver"
"#,
        )
        .expect("migrate version one");

        assert_eq!(config.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(config.crucible.repository.as_deref(), Some("logriver"));
        assert_eq!(config.crucible.repository_location, None);
    }

    #[test]
    fn rejects_unknown_future_schema_versions() {
        let error = parse_and_migrate(
            r#"
schema_version = 99

[crucible]
url = "http://cru"
token = "secret"
project = "COMMON"
"#,
        )
        .expect_err("reject future version");

        assert!(error.contains("unsupported project configuration schema version 99"));
    }

    #[test]
    fn unsetting_repository_also_removes_its_location() {
        let mut config = fixture();

        config.unset_repository();

        assert_eq!(config.crucible.repository, None);
        assert_eq!(config.crucible.repository_location, None);
    }

    #[test]
    fn setting_repository_does_not_keep_a_stale_location() {
        let mut config = fixture();

        config.set_repository("another-repository".to_owned());

        assert_eq!(
            config.crucible.repository.as_deref(),
            Some("another-repository")
        );
        assert_eq!(config.crucible.repository_location, None);
    }

    #[test]
    fn reviewer_updates_are_idempotent() {
        let mut config = fixture();

        config.add_reviewer("reviewer".to_owned());
        config.add_reviewer("another".to_owned());
        config.remove_reviewer("reviewer");

        assert_eq!(config.crucible.reviewers, vec!["another"]);
    }

    #[test]
    fn resolve_prefers_project_and_strips_global_anchor() {
        let project = fixture();
        let mut global = fixture();
        global.set_url("http://global".to_owned());
        global.set_repository("global-git".to_owned());

        let resolved = ProjectConfig::resolve(Some(project), Some(global)).expect("project wins");
        assert_eq!(resolved.crucible.url, "http://cru");
        assert_eq!(resolved.crucible.repository.as_deref(), Some("common-git"));

        let mut global = fixture();
        global.set_repository("global-git".to_owned());
        let resolved = ProjectConfig::resolve(None, Some(global)).expect("global fallback");
        assert_eq!(resolved.crucible.url, "http://cru");
        assert_eq!(resolved.crucible.repository, None);
        assert_eq!(resolved.crucible.repository_location, None);
    }

    #[test]
    fn moves_legacy_xdg_directory_when_platform_dir_is_missing() {
        let root = tempfile::tempdir().expect("temp");
        let legacy = root.path().join("legacy");
        let dest = root.path().join("dest");
        std::fs::create_dir_all(legacy.join("hooks")).expect("legacy hooks");
        std::fs::write(legacy.join("config.toml"), "schema_version = 2\n").expect("legacy config");

        super::migrate_legacy_xdg_into(&legacy, &dest);

        assert!(dest.join("config.toml").is_file());
        assert!(dest.join("hooks").is_dir());
        assert!(!legacy.exists());
    }

    fn fixture() -> ProjectConfig {
        ProjectConfig {
            schema_version: CURRENT_SCHEMA_VERSION,
            crucible: CrucibleConfig {
                url: "http://cru".to_owned(),
                token: "secret".to_owned(),
                project: "COMMON".to_owned(),
                repository: Some("common-git".to_owned()),
                repository_location: Some("ssh://git/common.git".to_owned()),
                reviewers: vec!["reviewer".to_owned()],
            },
        }
    }
}

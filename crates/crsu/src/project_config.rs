use crate::git_repository::Repository;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    pub fn load() -> Result<Option<Self>, String> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let contents = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        parse_and_migrate(&contents).map(Some)
    }

    pub fn save(&self) -> Result<PathBuf, String> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(format!(
                "cannot save schema version {}; current version is {CURRENT_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        let path = config_path()?;
        let directory = path
            .parent()
            .ok_or_else(|| "invalid configuration path".to_owned())?;
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let contents = toml::to_string_pretty(self).map_err(|error| error.to_string())?;
        std::fs::write(&path, contents).map_err(|error| error.to_string())?;
        set_private_permissions(&path)?;
        Ok(path)
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

fn config_path() -> Result<PathBuf, String> {
    Repository::discover()
        .map(|repository| repository.common_dir().join("crsu/config.toml"))
        .map_err(|error| error.to_string())
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

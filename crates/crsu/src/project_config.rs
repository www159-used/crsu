use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

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
    pub reviewers: Vec<String>,
}

impl ProjectConfig {
    pub fn load() -> Result<Option<Self>, String> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let contents = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        toml::from_str(&contents)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    pub fn save(&self) -> Result<PathBuf, String> {
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
}

fn config_path() -> Result<PathBuf, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .map_err(|error| format!("Git unavailable: {error}"))?;
    if !output.status.success() {
        return Err("not a Git repository".to_owned());
    }
    Ok(PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()).join("crsu/config.toml"))
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
    use super::{CrucibleConfig, ProjectConfig};
    #[test]
    fn config_is_explicit_and_versioned() {
        let config = ProjectConfig {
            schema_version: 1,
            crucible: CrucibleConfig {
                url: "http://cru".to_owned(),
                token: "secret".to_owned(),
                project: "COMMON".to_owned(),
                repository: Some("common-git".to_owned()),
                reviewers: vec!["reviewer".to_owned()],
            },
        };
        let text = toml::to_string_pretty(&config).expect("serialize config");
        assert!(text.contains("schema_version = 1"));
        assert!(text.contains("project = \"COMMON\""));
    }
}

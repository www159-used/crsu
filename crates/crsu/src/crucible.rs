use crate::review_diff::ReviewDiff;
use serde_json::{Value, json};
use std::env;
use std::fmt;

pub struct Client {
    url: String,
    token: String,
}

#[derive(Debug)]
pub struct User {
    pub username: String,
    pub display_name: String,
}

impl Client {
    pub fn new(url: &str, token: String) -> Self {
        Self {
            url: url.trim_end_matches('/').to_owned(),
            token,
        }
    }

    pub fn login(url: &str, username: &str, password: &str) -> Result<String, CrucibleError> {
        let response = reqwest::blocking::Client::new()
            .post(format!(
                "{}/rest-service-fecru/auth/login",
                url.trim_end_matches('/')
            ))
            .form(&[("userName", username), ("password", password)])
            .send()
            .map_err(CrucibleError::Request)?
            .error_for_status()
            .map_err(CrucibleError::Request)?
            .text()
            .map_err(CrucibleError::Request)?;
        let response = parse_json(&response)?;
        response
            .get("token")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(CrucibleError::MalformedLogin)
    }

    pub fn project_keys(&self) -> Result<Vec<String>, CrucibleError> {
        self.get_names("/rest-service/projects-v1", "/projectData", "key")
    }

    pub fn repository_names(&self) -> Result<Vec<String>, CrucibleError> {
        self.get_names("/rest-service/repositories-v1", "/repoData", "name")
    }

    pub fn users(&self) -> Result<Vec<User>, CrucibleError> {
        let response = self.get("/rest-service/users-v1")?;
        let values = response
            .pointer("/userData")
            .and_then(Value::as_array)
            .ok_or(CrucibleError::MalformedCandidates)?;
        let mut users = values
            .iter()
            .filter_map(|value| {
                Some(User {
                    username: value.get("userName")?.as_str()?.to_owned(),
                    display_name: value
                        .get("displayName")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                })
            })
            .collect::<Vec<_>>();
        users.sort_by(|left, right| left.username.cmp(&right.username));
        Ok(users)
    }

    fn get_names(
        &self,
        path: &str,
        collection: &str,
        field: &str,
    ) -> Result<Vec<String>, CrucibleError> {
        let response = self.get(path)?;
        let values = response
            .pointer(collection)
            .and_then(Value::as_array)
            .ok_or(CrucibleError::MalformedCandidates)?;
        let mut names = values
            .iter()
            .filter_map(|value| value.get(field).and_then(Value::as_str))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        Ok(names)
    }

    fn get(&self, path: &str) -> Result<Value, CrucibleError> {
        let response = reqwest::blocking::Client::new()
            .get(format!("{}{}", self.url, path))
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .query(&[("FEAUTH", &self.token)])
            .send()
            .map_err(CrucibleError::Request)?
            .error_for_status()
            .map_err(CrucibleError::Request)?
            .text()
            .map_err(CrucibleError::Request)?;
        let response = parse_json(&response)?;
        Ok(response)
    }
}

fn parse_json(body: &str) -> Result<Value, CrucibleError> {
    serde_json::from_str(body).map_err(|error| {
        let preview = body.chars().take(200).collect::<String>();
        CrucibleError::InvalidJson(format!("{error}; response starts with {preview:?}"))
    })
}

pub fn submit_if_configured(review_diff: &ReviewDiff) -> Result<Option<String>, CrucibleError> {
    let Some(config) = Config::from_environment()? else {
        return Ok(None);
    };

    let response = reqwest::blocking::Client::new()
        .post(format!("{}/rest-service/reviews-v1", config.url))
        .query(&[("FEAUTH", &config.token)])
        .json(&json!({
            "reviewData": {
                "projectKey": config.project,
                "name": review_diff.title(),
            },
            "patch": review_diff.patch(),
        }))
        .send()
        .map_err(CrucibleError::Request)?
        .error_for_status()
        .map_err(CrucibleError::Request)?
        .json::<Value>()
        .map_err(CrucibleError::Request)?;

    let review_id = response
        .pointer("/permaId/id")
        .and_then(Value::as_str)
        .ok_or(CrucibleError::MalformedResponse)?;
    for reviewer in config.reviewers {
        reqwest::blocking::Client::new()
            .post(format!(
                "{}/rest-service/reviews-v1/{review_id}/reviewers",
                config.url
            ))
            .query(&[("FEAUTH", &config.token)])
            .body(reviewer)
            .send()
            .map_err(CrucibleError::Request)?
            .error_for_status()
            .map_err(CrucibleError::Request)?;
    }
    Ok(Some(review_id.to_owned()))
}

struct Config {
    url: String,
    project: String,
    token: String,
    reviewers: Vec<String>,
}

impl Config {
    fn from_environment() -> Result<Option<Self>, CrucibleError> {
        let url = env::var("CRSU_CRUCIBLE_URL").ok();
        let project = env::var("CRSU_CRUCIBLE_PROJECT").ok();
        let token = env::var("CRSU_CRUCIBLE_TOKEN").ok();
        if url.is_none() && project.is_none() && token.is_none() {
            return crate::project_config::ProjectConfig::load()
                .map_err(CrucibleError::ProjectConfiguration)
                .map(|config| {
                    config.map(|config| Self {
                        url: config.crucible.url,
                        project: config.crucible.project,
                        token: config.crucible.token,
                        reviewers: config.crucible.reviewers,
                    })
                });
        }
        Ok(Some(Self {
            url: required("CRSU_CRUCIBLE_URL", url)?,
            project: required("CRSU_CRUCIBLE_PROJECT", project)?,
            token: required("CRSU_CRUCIBLE_TOKEN", token)?,
            reviewers: Vec::new(),
        }))
    }
}

fn required(name: &'static str, value: Option<String>) -> Result<String, CrucibleError> {
    value.ok_or(CrucibleError::MissingConfiguration(name))
}

#[derive(Debug)]
pub enum CrucibleError {
    MalformedResponse,
    MalformedLogin,
    InvalidJson(String),
    MalformedCandidates,
    MissingConfiguration(&'static str),
    ProjectConfiguration(String),
    Request(reqwest::Error),
}

impl fmt::Display for CrucibleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedResponse => write!(formatter, "Crucible response has no review id"),
            Self::MalformedLogin => write!(formatter, "Crucible login response has no token"),
            Self::InvalidJson(error) => {
                write!(formatter, "Crucible returned invalid JSON: {error}")
            }
            Self::MalformedCandidates => {
                write!(formatter, "Crucible returned no usable candidates")
            }
            Self::MissingConfiguration(name) => write!(formatter, "missing {name}"),
            Self::ProjectConfiguration(error) => {
                write!(formatter, "project configuration failed: {error}")
            }
            Self::Request(error) => write!(formatter, "Crucible request failed: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Client;
    use httpmock::Method::{GET, POST};
    use httpmock::MockServer;

    #[test]
    fn reads_project_and_repository_candidates_with_the_login_token() {
        let server = MockServer::start();
        let projects = server.mock(|when, then| {
            when.method(GET)
                .path("/rest-service/projects-v1")
                .header("accept-encoding", "identity")
                .query_param("FEAUTH", "test-token");
            then.status(200)
                .json_body(serde_json::json!({"projectData":[{"key":"Z"},{"key":"A"}]}));
        });
        let repositories = server.mock(|when, then| {
            when.method(GET)
                .path("/rest-service/repositories-v1")
                .query_param("FEAUTH", "test-token");
            then.status(200)
                .json_body(serde_json::json!({"repoData":[{"name":"repo-b"},{"name":"repo-a"}]}));
        });
        let client = Client::new(&server.base_url(), "test-token".to_owned());
        assert_eq!(client.project_keys().expect("projects"), ["A", "Z"]);
        assert_eq!(
            client.repository_names().expect("repositories"),
            ["repo-a", "repo-b"]
        );
        projects.assert();
        repositories.assert();
    }

    #[test]
    fn exchanges_username_and_password_for_a_token() {
        let server = MockServer::start();
        let login = server.mock(|when, then| {
            when.method(POST)
                .path("/rest-service-fecru/auth/login")
                .body_contains("userName=ww")
                .body_contains("password=secret");
            then.status(200)
                .json_body(serde_json::json!({"token":"new-token"}));
        });
        assert_eq!(
            Client::login(&server.base_url(), "ww", "secret").expect("login"),
            "new-token"
        );
        login.assert();
    }

    #[test]
    fn reads_users_for_reviewer_selection() {
        let server = MockServer::start();
        let list = server.mock(|when, then| {
            when.method(GET)
                .path("/rest-service/users-v1")
                .query_param("FEAUTH", "test-token");
            then.status(200).json_body(serde_json::json!({"userData":[
                {"userName":"z.user","displayName":"Z User"},
                {"userName":"a.user","displayName":"A User"}
            ]}));
        });
        let client = Client::new(&server.base_url(), "test-token".to_owned());
        let users = client.users().expect("users");
        assert_eq!(users[0].username, "a.user");
        assert_eq!(users[0].display_name, "A User");
        list.assert();
    }
}

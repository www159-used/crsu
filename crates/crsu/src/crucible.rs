use crate::git_repository::ReviewDiff;
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

#[derive(Clone, Debug)]
pub struct RepositoryCandidate {
    pub name: String,
    pub scm_type: String,
    pub location: String,
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
            .map_err(request_error)?;
        let response = response_body(response)?;
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

    pub fn has_fisheye(&self) -> Result<bool, CrucibleError> {
        self.get("/rest-service-fecru/server-v1")?
            .get("isFishEye")
            .and_then(Value::as_bool)
            .ok_or(CrucibleError::MalformedCandidates)
    }

    pub fn repositories(&self) -> Result<Vec<RepositoryCandidate>, CrucibleError> {
        let response = self.get("/rest-service/repositories-v1")?;
        let values = response
            .pointer("/repoData")
            .and_then(Value::as_array)
            .ok_or(CrucibleError::MalformedCandidates)?;
        let mut repositories = values
            .iter()
            .filter(|value| {
                value
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
            })
            .filter_map(|value| {
                Some(RepositoryCandidate {
                    name: value.get("name")?.as_str()?.to_owned(),
                    scm_type: value.get("type")?.as_str()?.to_owned(),
                    location: value.get("location")?.as_str()?.to_owned(),
                })
            })
            .collect::<Vec<_>>();
        repositories.sort_by(|left, right| left.name.cmp(&right.name));
        repositories.dedup_by(|left, right| left.name == right.name);
        Ok(repositories)
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
            .map_err(request_error)?;
        let response = response_body(response)?;
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

fn response_body(response: reqwest::blocking::Response) -> Result<String, CrucibleError> {
    let status = response.status();
    let body = response.text().map_err(request_error)?;
    if status.is_success() {
        return Ok(body);
    }

    Err(CrucibleError::HttpResponse {
        status,
        detail: error_detail(&body),
    })
}

fn error_detail(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        let code = value.get("code").and_then(Value::as_str);
        let message = value.get("message").and_then(Value::as_str);
        return match (code, message) {
            (Some(code), Some(message)) => format!("{code}: {message}"),
            (Some(code), None) => code.to_owned(),
            (None, Some(message)) => message.to_owned(),
            (None, None) => value.to_string(),
        };
    }

    let preview = body.chars().take(200).collect::<String>();
    if preview.is_empty() {
        "empty response body".to_owned()
    } else {
        preview
    }
}

fn request_error(error: reqwest::Error) -> CrucibleError {
    CrucibleError::Request(error.without_url())
}

pub struct Submission {
    review_id: String,
    review_url: String,
    created: bool,
    reviewers: Vec<String>,
    title_update: Option<TitleUpdate>,
}

struct TitleUpdate {
    previous: String,
    current: String,
}

impl Submission {
    #[must_use]
    pub fn review_id(&self) -> &str {
        &self.review_id
    }

    #[must_use]
    pub fn review_url(&self) -> &str {
        &self.review_url
    }

    #[must_use]
    pub fn was_created(&self) -> bool {
        self.created
    }

    #[must_use]
    pub fn reviewers(&self) -> &[String] {
        &self.reviewers
    }

    #[must_use]
    pub fn title_update(&self) -> Option<(&str, &str)> {
        self.title_update
            .as_ref()
            .map(|update| (update.previous.as_str(), update.current.as_str()))
    }
}

pub fn submit_if_configured(review_diff: &ReviewDiff) -> Result<Option<Submission>, CrucibleError> {
    let Some(config) = Config::from_environment()? else {
        return Ok(None);
    };
    config.validate_anchor()?;

    if let Some(review_id) = review_diff.review_id() {
        let title_update = update_review(&config, review_id, review_diff)?;
        return Ok(Some(Submission {
            review_id: review_id.to_owned(),
            review_url: format!("{}/cru/{review_id}", config.url),
            created: false,
            reviewers: config.reviewers,
            title_update,
        }));
    }

    let mut payload = json!({
        "reviewData": {
            "projectKey": config.project,
            "name": review_diff.title(),
            "description": review_diff.objectives(),
        },
        "patch": review_diff.patch(),
    });
    if let Some(repository) = &config.repository {
        payload["anchor"] = json!({"anchorRepository": repository});
    }

    let response = config
        .http
        .post(format!("{}/rest-service/reviews-v1", config.url))
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .query(&[("FEAUTH", &config.token)])
        .json(&payload)
        .send()
        .map_err(request_error)?;
    let response = response_body(response)?;
    let response = parse_json(&response)?;

    let review_id = response
        .pointer("/permaId/id")
        .and_then(Value::as_str)
        .ok_or(CrucibleError::MalformedResponse)?;
    for reviewer in &config.reviewers {
        let reviewer_response = config
            .http
            .post(format!(
                "{}/rest-service/reviews-v1/{review_id}/reviewers",
                config.url
            ))
            .query(&[("FEAUTH", &config.token)])
            .body(reviewer.clone())
            .send()
            .map_err(request_error)?;
        response_body(reviewer_response)?;
    }
    if !config.reviewers.is_empty() {
        start_review(&config, review_id)?;
    }
    Ok(Some(Submission {
        review_id: review_id.to_owned(),
        review_url: format!("{}/cru/{review_id}", config.url),
        created: true,
        reviewers: config.reviewers,
        title_update: None,
    }))
}

fn start_review(config: &Config, review_id: &str) -> Result<(), CrucibleError> {
    let response = config
        .http
        .post(format!(
            "{}/rest-service/reviews-v1/{review_id}/transition",
            config.url
        ))
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .query(&[
            ("action", "action:approveReview"),
            ("FEAUTH", config.token.as_str()),
        ])
        .body(" ")
        .send()
        .map_err(request_error)?;
    response_body(response)?;
    Ok(())
}

fn update_review(
    config: &Config,
    review_id: &str,
    review_diff: &ReviewDiff,
) -> Result<Option<TitleUpdate>, CrucibleError> {
    let review = config
        .http
        .get(format!(
            "{}/rest-service/reviews-v1/{review_id}",
            config.url
        ))
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .query(&[("FEAUTH", &config.token)])
        .send()
        .map_err(request_error)?;
    let review = parse_json(&response_body(review)?)?;
    let current_title = review
        .get("name")
        .and_then(Value::as_str)
        .ok_or(CrucibleError::MalformedResponse)?;
    let is_draft = review.get("state").and_then(Value::as_str) == Some("Draft");
    let current_objectives = review
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let mut payload = json!({"patch": review_diff.patch()});
    if let Some(repository) = &config.repository {
        payload["anchor"] = json!({"anchorRepository": repository});
    }
    let patch = config
        .http
        .post(format!(
            "{}/rest-service/reviews-v1/{review_id}/patch",
            config.url
        ))
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .query(&[("FEAUTH", &config.token)])
        .json(&payload)
        .send()
        .map_err(request_error)?;
    response_body(patch)?;

    let title = if current_title == review_diff.title() {
        None
    } else {
        update_review_ajax(
            config,
            review_id,
            "updateReviewTitleAjax",
            &[("title", review_diff.title()), ("adgified", "true")],
            CrucibleError::TitleUpdateRejected,
        )?;
        Some(TitleUpdate {
            previous: current_title.to_owned(),
            current: review_diff.title().to_owned(),
        })
    };
    if current_objectives != review_diff.objectives() {
        update_review_ajax(
            config,
            review_id,
            "updateReviewObjectivesAjax",
            &[("input", review_diff.objectives())],
            CrucibleError::ObjectivesUpdateRejected,
        )?;
    }
    if is_draft && !config.reviewers.is_empty() {
        start_review(config, review_id)?;
    }
    Ok(title)
}

fn update_review_ajax(
    config: &Config,
    review_id: &str,
    endpoint: &str,
    form: &[(&str, &str)],
    rejected: CrucibleError,
) -> Result<(), CrucibleError> {
    let response = config
        .http
        .post(format!("{}/json/cru/{review_id}/{endpoint}", config.url))
        .header("X-Atlassian-Token", "no-check")
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .query(&[("FEAUTH", &config.token)])
        .form(form)
        .send()
        .map_err(request_error)?;
    let response = parse_json(&response_body(response)?)?;
    if response.get("worked").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(rejected)
    }
}

struct Config {
    http: reqwest::blocking::Client,
    url: String,
    project: String,
    token: String,
    repository: Option<String>,
    repository_location: Option<String>,
    reviewers: Vec<String>,
}

impl Config {
    fn from_environment() -> Result<Option<Self>, CrucibleError> {
        let url = env::var("CRSU_CRUCIBLE_URL").ok();
        let project = env::var("CRSU_CRUCIBLE_PROJECT").ok();
        let token = env::var("CRSU_CRUCIBLE_TOKEN").ok();
        let repository = env::var("CRSU_CRUCIBLE_REPOSITORY").ok();
        let repository_location = env::var("CRSU_CRUCIBLE_REPOSITORY_LOCATION").ok();
        let reviewers = env::var("CRSU_CRUCIBLE_REVIEWERS")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if url.is_none() && project.is_none() && token.is_none() {
            return crate::project_config::ProjectConfig::load()
                .map_err(CrucibleError::ProjectConfiguration)
                .map(|config| {
                    config.map(|config| Self {
                        http: reqwest::blocking::Client::new(),
                        url: config.crucible.url,
                        project: config.crucible.project,
                        token: config.crucible.token,
                        repository: config.crucible.repository,
                        repository_location: config.crucible.repository_location,
                        reviewers: config.crucible.reviewers,
                    })
                });
        }
        Ok(Some(Self {
            http: reqwest::blocking::Client::new(),
            url: required("CRSU_CRUCIBLE_URL", url)?,
            project: required("CRSU_CRUCIBLE_PROJECT", project)?,
            token: required("CRSU_CRUCIBLE_TOKEN", token)?,
            repository,
            repository_location,
            reviewers,
        }))
    }

    fn validate_anchor(&self) -> Result<(), CrucibleError> {
        let (Some(repository), Some(expected_remote)) =
            (&self.repository, &self.repository_location)
        else {
            return Ok(());
        };
        let actual_remote = crate::git_repository::Repository::discover()
            .map_err(|error| CrucibleError::ProjectConfiguration(error.to_string()))?
            .origin_url()
            .ok_or_else(|| CrucibleError::AnchorMismatch {
                repository: repository.clone(),
                expected: expected_remote.clone(),
                actual: "no origin remote".to_owned(),
            })?;
        if crate::git_repository::git_remotes_match(expected_remote, &actual_remote) {
            Ok(())
        } else {
            Err(CrucibleError::AnchorMismatch {
                repository: repository.clone(),
                expected: expected_remote.clone(),
                actual: actual_remote,
            })
        }
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
    TitleUpdateRejected,
    ObjectivesUpdateRejected,
    MissingConfiguration(&'static str),
    ProjectConfiguration(String),
    AnchorMismatch {
        repository: String,
        expected: String,
        actual: String,
    },
    HttpResponse {
        status: reqwest::StatusCode,
        detail: String,
    },
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
            Self::TitleUpdateRejected => {
                write!(formatter, "Crucible did not update the review title")
            }
            Self::ObjectivesUpdateRejected => {
                write!(formatter, "Crucible did not update the review objectives")
            }
            Self::MissingConfiguration(name) => write!(formatter, "missing {name}"),
            Self::ProjectConfiguration(error) => {
                write!(formatter, "project configuration failed: {error}")
            }
            Self::AnchorMismatch {
                repository,
                expected,
                actual,
            } => write!(
                formatter,
                "FishEye repository {repository} tracks {expected}, but Git origin is {actual}; run crsu init to select the matching repository"
            ),
            Self::HttpResponse { status, detail } => {
                write!(formatter, "Crucible rejected request ({status}): {detail}")
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
            then.status(200).json_body(serde_json::json!({"repoData":[
                {"name":"repo-b","type":"git","location":"ssh://git/repo-b.git","enabled":true},
                {"name":"repo-a","type":"git","location":"ssh://git/repo-a.git","enabled":true}
            ]}));
        });
        let client = Client::new(&server.base_url(), "test-token".to_owned());
        assert_eq!(client.project_keys().expect("projects"), ["A", "Z"]);
        assert_eq!(
            client
                .repositories()
                .expect("repositories")
                .iter()
                .map(|repository| repository.name.as_str())
                .collect::<Vec<_>>(),
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

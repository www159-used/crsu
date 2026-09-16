//! Shared, deterministic adapters for `crsu` integration tests.

use httpmock::{
    Method::{GET, POST},
    MockServer,
};

/// A reviewer identity returned by a Crucible init fixture.
#[derive(Clone, Debug)]
pub struct FixtureUser {
    pub username: String,
    pub display_name: String,
}

impl FixtureUser {
    #[must_use]
    pub fn new(username: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            display_name: display_name.into(),
        }
    }
}

/// All responses required by one successful Crucible init conversation.
#[derive(Clone, Debug)]
pub struct InitFixture {
    pub username: String,
    pub password: String,
    pub token: String,
    pub projects: Vec<String>,
    pub repositories: Vec<String>,
    pub users: Vec<FixtureUser>,
}

/// HTTP adapter that serves an `InitFixture` as Crucible endpoints.
pub struct MockCrucible {
    server: MockServer,
}

impl MockCrucible {
    #[must_use]
    pub fn start(fixture: InitFixture) -> Self {
        let server = MockServer::start();
        let login_username = fixture.username;
        let login_password = fixture.password;
        let token = fixture.token;
        let projects = serde_json::json!({
            "projectData": fixture
                .projects
                .into_iter()
                .map(|key| serde_json::json!({"key": key}))
                .collect::<Vec<_>>()
        });
        let repositories = serde_json::json!({
            "repoData": fixture
                .repositories
                .into_iter()
                .map(|name| serde_json::json!({"name": name}))
                .collect::<Vec<_>>()
        });
        let users = serde_json::json!({
            "userData": fixture
                .users
                .into_iter()
                .map(|user| serde_json::json!({
                    "userName": user.username,
                    "displayName": user.display_name,
                }))
                .collect::<Vec<_>>()
        });
        server.mock(|when, then| {
            when.method(POST)
                .path("/rest-service-fecru/auth/login")
                .body_contains(format!("userName={login_username}"))
                .body_contains(format!("password={login_password}"));
            then.status(200)
                .json_body(serde_json::json!({"token": token}));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/rest-service/projects-v1")
                .query_param("FEAUTH", &token);
            then.status(200).json_body(projects);
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/rest-service/repositories-v1")
                .query_param("FEAUTH", &token);
            then.status(200).json_body(repositories);
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/rest-service/users-v1")
                .query_param("FEAUTH", &token);
            then.status(200).json_body(users);
        });
        Self { server }
    }

    #[must_use]
    pub fn base_url(&self) -> String {
        self.server.base_url()
    }
}

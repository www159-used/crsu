//! Shared, deterministic adapters for `crsu` integration tests.

use httpmock::{
    Method::{GET, POST},
    Mock, MockServer,
};

/// A reviewer identity returned by a Crucible init fixture.
#[derive(Clone, Debug)]
pub struct FixtureUser {
    pub username: String,
    pub display_name: String,
}

/// Repository metadata returned by Crucible during init.
#[derive(Clone, Debug)]
pub struct FixtureRepository {
    pub name: String,
    pub scm_type: String,
    pub location: String,
    pub enabled: bool,
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
    pub is_fisheye: bool,
    pub projects: Vec<String>,
    pub repositories: Vec<FixtureRepository>,
    pub users: Vec<FixtureUser>,
}

/// One create-review conversation exposed by the mock Crucible server.
#[derive(Clone, Debug)]
pub struct ReviewFixture {
    pub token: String,
    pub project: String,
    pub repository: String,
    pub response: ReviewResponse,
}

/// Result returned by Crucible when crsu creates a review.
#[derive(Clone, Debug)]
pub enum ReviewResponse {
    Created {
        review_id: String,
    },
    Updated {
        review_id: String,
        current_title: String,
        new_title: String,
    },
    Rejected {
        status: u16,
        body: String,
    },
}

/// HTTP adapter that serves an `InitFixture` as Crucible endpoints.
pub struct MockCrucible {
    server: MockServer,
    review_mock_ids: Vec<usize>,
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
                .map(|repository| serde_json::json!({
                    "name": repository.name,
                    "type": repository.scm_type,
                    "location": repository.location,
                    "enabled": repository.enabled,
                }))
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
                .path("/rest-service-fecru/server-v1")
                .query_param("FEAUTH", &token);
            then.status(200).json_body(serde_json::json!({
                "isCrucible": true,
                "isFishEye": fixture.is_fisheye,
            }));
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
        Self {
            server,
            review_mock_ids: Vec::new(),
        }
    }

    /// Starts a Crucible adapter for one create-review request.
    #[must_use]
    pub fn start_review(fixture: ReviewFixture) -> Self {
        let server = MockServer::start();
        let mut review_mock_ids = Vec::new();
        match fixture.response {
            ReviewResponse::Created { review_id } => {
                let review = server.mock(|when, then| {
                    when.method(POST)
                        .path("/rest-service/reviews-v1")
                        .header("accept", "application/json")
                        .header("accept-encoding", "identity")
                        .query_param("FEAUTH", &fixture.token)
                        .body_contains(format!("\"projectKey\":\"{}\"", fixture.project))
                        .body_contains(format!("\"anchorRepository\":\"{}\"", fixture.repository))
                        .body_contains("\"patch\":");
                    then.status(200)
                        .header("content-type", "application/json")
                        .json_body(serde_json::json!({"permaId":{"id":review_id}}));
                });
                review_mock_ids.push(review.id);
            }
            ReviewResponse::Updated {
                review_id,
                current_title,
                new_title,
            } => {
                let get = server.mock(|when, then| {
                    when.method(GET)
                        .path(format!("/rest-service/reviews-v1/{review_id}"))
                        .query_param("FEAUTH", &fixture.token);
                    then.status(200).json_body(serde_json::json!({
                        "permaId":{"id":review_id}, "name":current_title
                    }));
                });
                let patch = server.mock(|when, then| {
                    when.method(POST)
                        .path(format!("/rest-service/reviews-v1/{review_id}/patch"))
                        .query_param("FEAUTH", &fixture.token)
                        .body_contains("\"patch\":");
                    then.status(200)
                        .json_body(serde_json::json!({"state":"Draft"}));
                });
                let title = server.mock(|when, then| {
                    when.method(POST)
                        .path(format!("/json/cru/{review_id}/updateReviewTitleAjax"))
                        .query_param("FEAUTH", &fixture.token)
                        .header("x-atlassian-token", "no-check")
                        .body_contains(format!(
                            "title={}",
                            new_title.replace(' ', "+").replace(':', "%3A")
                        ));
                    then.status(200)
                        .json_body(serde_json::json!({"worked":true,"title":new_title}));
                });
                review_mock_ids.extend([get.id, patch.id, title.id]);
            }
            ReviewResponse::Rejected { status, body } => {
                let review = server.mock(|when, then| {
                    when.method(POST)
                        .path("/rest-service/reviews-v1")
                        .query_param("FEAUTH", &fixture.token);
                    then.status(status)
                        .header("content-type", "application/json")
                        .body(body);
                });
                review_mock_ids.push(review.id);
            }
        }
        Self {
            server,
            review_mock_ids,
        }
    }

    #[must_use]
    pub fn base_url(&self) -> String {
        self.server.base_url()
    }

    /// Asserts that the expected create-review request occurred exactly once.
    ///
    /// # Panics
    ///
    /// Panics when this mock was not started with [`Self::start_review`], or
    /// when the expected request did not occur exactly once.
    pub fn assert_review_request(&self) {
        assert!(
            !self.review_mock_ids.is_empty(),
            "mock Crucible has no review expectations"
        );
        for id in &self.review_mock_ids {
            Mock::new(*id, &self.server).assert();
        }
    }
}

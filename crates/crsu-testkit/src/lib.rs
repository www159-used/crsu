//! Shared, deterministic adapters for `crsu` integration tests.

use httpmock::{
    Method::{GET, POST},
    Mock, MockServer,
};
use serde::Deserialize;
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;
use tiny_http::{Header, Method, Response, Server, StatusCode};

/// A reviewer identity returned by a Crucible init fixture.
#[derive(Clone, Debug, Deserialize)]
pub struct FixtureUser {
    pub username: String,
    pub display_name: String,
}

/// Repository metadata returned by Crucible during init.
#[derive(Clone, Debug, Deserialize)]
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
    pub response: ReviewResponse,
}

/// One land conversation: fetch review/reviewers, then close.
#[derive(Clone, Debug)]
pub struct LandFixture {
    pub token: String,
    pub review_id: String,
    pub title: String,
    pub state: String,
    pub objectives: String,
    pub reviewers: Vec<LandReviewer>,
}

/// A reviewer row returned while preparing to land.
#[derive(Clone, Debug)]
pub struct LandReviewer {
    pub username: String,
    pub completed: bool,
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
        current_objectives: String,
        current_state: String,
    },
    Rejected {
        status: u16,
        body: String,
    },
}

/// Externally observable review state exposed by the fake Crucible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewSnapshot {
    pub id: String,
    pub title: String,
    pub objectives: String,
    pub state: String,
    pub reviewers: Vec<String>,
}

/// HTTP adapter that serves an `InitFixture` as Crucible endpoints.
pub struct MockCrucible {
    server: Option<MockServer>,
    stateful: Option<StatefulCrucible>,
    review_mock_ids: Vec<usize>,
}

struct StatefulCrucible {
    base_url: String,
    review: Arc<Mutex<Option<ReviewSnapshot>>>,
    interactions: Arc<AtomicUsize>,
    errors: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
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
            server: Some(server),
            stateful: None,
            review_mock_ids: Vec::new(),
        }
    }

    /// Starts a Crucible adapter for one create-review request.
    #[must_use]
    pub fn start_review(fixture: ReviewFixture) -> Self {
        match &fixture.response {
            ReviewResponse::Rejected { status, body } => {
                Self::start_rejected_review(&fixture.token, *status, body)
            }
            ReviewResponse::Created { .. } | ReviewResponse::Updated { .. } => Self {
                server: None,
                stateful: Some(StatefulCrucible::start(fixture)),
                review_mock_ids: Vec::new(),
            },
        }
    }

    fn start_rejected_review(token: &str, status: u16, body: &str) -> Self {
        let server = MockServer::start();
        let review_id = {
            let review = server.mock(|when, then| {
                when.method(POST)
                    .path("/rest-service/reviews-v1")
                    .query_param("FEAUTH", token);
                then.status(status)
                    .header("content-type", "application/json")
                    .body(body);
            });
            review.id
        };
        Self {
            server: Some(server),
            stateful: None,
            review_mock_ids: vec![review_id],
        }
    }

    /// Starts a Crucible adapter for landing an accepted review.
    #[must_use]
    pub fn start_land(fixture: &LandFixture) -> Self {
        let server = MockServer::start();
        let review_path = format!("/rest-service/reviews-v1/{}", fixture.review_id);
        let reviewers_path = format!("{review_path}/reviewers");
        let close_path = format!("{review_path}/close");
        server.mock(|when, then| {
            when.method(GET)
                .path(&review_path)
                .query_param("FEAUTH", &fixture.token);
            then.status(200).json_body(review_json(&ReviewSnapshot {
                id: fixture.review_id.clone(),
                title: fixture.title.clone(),
                objectives: fixture.objectives.clone(),
                state: fixture.state.clone(),
                reviewers: Vec::new(),
            }));
        });
        let reviewers = fixture
            .reviewers
            .iter()
            .map(|reviewer| {
                serde_json::json!({
                    "userName": reviewer.username,
                    "completed": reviewer.completed,
                })
            })
            .collect::<Vec<_>>();
        server.mock(|when, then| {
            when.method(GET)
                .path(&reviewers_path)
                .query_param("FEAUTH", &fixture.token);
            then.status(200)
                .json_body(serde_json::json!({"reviewer": reviewers}));
        });
        let close_id = {
            let close = server.mock(|when, then| {
                when.method(POST)
                    .path(&close_path)
                    .query_param("FEAUTH", &fixture.token);
                then.status(200)
                    .json_body(serde_json::json!({"state":"Closed"}));
            });
            close.id
        };
        Self {
            server: Some(server),
            stateful: None,
            review_mock_ids: vec![close_id],
        }
    }

    /// Returns the local HTTP origin of this mock server.
    ///
    /// # Panics
    ///
    /// Panics if the mock server was not initialized correctly.
    #[must_use]
    pub fn base_url(&self) -> String {
        self.stateful.as_ref().map_or_else(
            || self.server.as_ref().expect("mock server").base_url(),
            |server| server.base_url.clone(),
        )
    }

    /// Returns the review exactly as a Crucible client would observe it.
    ///
    /// # Panics
    ///
    /// Panics if this is not a stateful review mock or no review exists yet.
    #[must_use]
    pub fn review(&self) -> ReviewSnapshot {
        self.stateful
            .as_ref()
            .expect("mock Crucible is not stateful")
            .review
            .lock()
            .expect("review state lock")
            .clone()
            .expect("review was not created")
    }

    /// Asserts that the expected create-review request occurred exactly once.
    ///
    /// # Panics
    ///
    /// Panics when this mock was not started with [`Self::start_review`], or
    /// when the expected request did not occur exactly once.
    pub fn assert_review_request(&self) {
        if let Some(server) = &self.stateful {
            let errors = server.errors.lock().expect("mock errors lock");
            assert!(errors.is_empty(), "mock Crucible errors: {errors:#?}");
            assert!(
                server.interactions.load(Ordering::Relaxed) > 0,
                "mock Crucible did not receive a review mutation"
            );
            assert!(
                server.review.lock().expect("review state lock").is_some(),
                "mock Crucible did not receive a review"
            );
            return;
        }
        assert!(
            !self.review_mock_ids.is_empty(),
            "mock Crucible has no review expectations"
        );
        let server = self.server.as_ref().expect("mock server");
        for id in &self.review_mock_ids {
            Mock::new(*id, server).assert();
        }
    }
}

impl StatefulCrucible {
    fn start(fixture: ReviewFixture) -> Self {
        let server = Server::http("127.0.0.1:0").expect("start stateful Crucible");
        let base_url = format!("http://{}", server.server_addr());
        let review = Arc::new(Mutex::new(match &fixture.response {
            ReviewResponse::Updated {
                review_id,
                current_title,
                current_objectives,
                current_state,
            } => Some(ReviewSnapshot {
                id: review_id.clone(),
                title: current_title.clone(),
                objectives: current_objectives.clone(),
                state: current_state.clone(),
                reviewers: Vec::new(),
            }),
            ReviewResponse::Created { .. } | ReviewResponse::Rejected { .. } => None,
        }));
        let interactions = Arc::new(AtomicUsize::new(0));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_review = Arc::clone(&review);
        let thread_interactions = Arc::clone(&interactions);
        let thread_errors = Arc::clone(&errors);
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match server.recv_timeout(Duration::from_millis(50)) {
                    Ok(Some(request)) => {
                        if let Err(error) = handle_review_request(
                            request,
                            &fixture,
                            &thread_review,
                            &thread_interactions,
                        ) {
                            thread_errors.lock().expect("mock errors lock").push(error);
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        thread_errors
                            .lock()
                            .expect("mock errors lock")
                            .push(error.to_string());
                        break;
                    }
                }
            }
        });
        Self {
            base_url,
            review,
            interactions,
            errors,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for StatefulCrucible {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("join stateful Crucible");
        }
    }
}

#[derive(Deserialize)]
struct CreateReviewRequest {
    #[serde(rename = "reviewData")]
    review_data: CreateReviewData,
}

#[derive(Deserialize)]
struct CreateReviewData {
    name: String,
    description: String,
}

fn handle_review_request(
    mut request: tiny_http::Request,
    fixture: &ReviewFixture,
    review: &Mutex<Option<ReviewSnapshot>>,
    interactions: &AtomicUsize,
) -> Result<(), String> {
    let path = request
        .url()
        .split('?')
        .next()
        .unwrap_or(request.url())
        .to_owned();
    let method = request.method().clone();
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|error| error.to_string())?;

    let (status, response_body) = if method == Method::Post && path == "/rest-service/reviews-v1" {
        let payload: CreateReviewRequest =
            serde_json::from_str(&body).map_err(|error| error.to_string())?;
        let ReviewResponse::Created { review_id } = &fixture.response else {
            return Err("unexpected create-review request".to_owned());
        };
        *locked_review(review)? = Some(ReviewSnapshot {
            id: review_id.clone(),
            title: payload.review_data.name,
            objectives: payload.review_data.description,
            state: "Draft".to_owned(),
            reviewers: Vec::new(),
        });
        interactions.fetch_add(1, Ordering::Relaxed);
        (
            200,
            serde_json::json!({"permaId":{"id":review_id}}).to_string(),
        )
    } else if method == Method::Get && path.starts_with("/rest-service/reviews-v1/") {
        let snapshot = locked_review(review)?
            .clone()
            .ok_or_else(|| "review does not exist".to_owned())?;
        (200, review_json(&snapshot).to_string())
    } else if method == Method::Post && path.ends_with("/reviewers") {
        let reviewer = serde_json::from_str::<String>(&body).unwrap_or(body);
        mutate_existing(review, |snapshot| snapshot.reviewers.push(reviewer))?;
        interactions.fetch_add(1, Ordering::Relaxed);
        (204, String::new())
    } else if method == Method::Post && path.ends_with("/transition") {
        mutate_existing(review, |snapshot| "Review".clone_into(&mut snapshot.state))?;
        interactions.fetch_add(1, Ordering::Relaxed);
        (200, serde_json::json!({"state":"Review"}).to_string())
    } else if method == Method::Post && path.ends_with("/patch") {
        interactions.fetch_add(1, Ordering::Relaxed);
        (200, serde_json::json!({"state":"Draft"}).to_string())
    } else if method == Method::Post && path.ends_with("/updateReviewTitleAjax") {
        let title = form_field(&body, "title")?;
        mutate_existing(review, |snapshot| snapshot.title.clone_from(&title))?;
        interactions.fetch_add(1, Ordering::Relaxed);
        (
            200,
            serde_json::json!({"worked":true,"title":title}).to_string(),
        )
    } else if method == Method::Post && path.ends_with("/updateReviewObjectivesAjax") {
        let objectives = form_field(&body, "input")?;
        mutate_existing(review, |snapshot| {
            snapshot.objectives.clone_from(&objectives);
        })?;
        interactions.fetch_add(1, Ordering::Relaxed);
        (
            200,
            serde_json::json!({"worked":true,"payload":objectives}).to_string(),
        )
    } else {
        (404, format!("unhandled {method:?} {path}"))
    };

    let mut response = Response::from_string(response_body).with_status_code(StatusCode(status));
    if status != 204 {
        response.add_header(
            Header::from_bytes("content-type", "application/json").expect("valid header"),
        );
    }
    request.respond(response).map_err(|error| error.to_string())
}

fn locked_review(
    review: &Mutex<Option<ReviewSnapshot>>,
) -> Result<MutexGuard<'_, Option<ReviewSnapshot>>, String> {
    review.lock().map_err(|error| error.to_string())
}

fn mutate_existing(
    review: &Mutex<Option<ReviewSnapshot>>,
    mutate: impl FnOnce(&mut ReviewSnapshot),
) -> Result<(), String> {
    let mut guard = locked_review(review)?;
    let snapshot = guard
        .as_mut()
        .ok_or_else(|| "review does not exist".to_owned())?;
    mutate(snapshot);
    Ok(())
}

fn form_field(body: &str, key: &str) -> Result<String, String> {
    let form: std::collections::HashMap<String, String> =
        serde_urlencoded::from_str(body).map_err(|error| error.to_string())?;
    Ok(form.get(key).cloned().unwrap_or_default())
}

fn review_json(review: &ReviewSnapshot) -> serde_json::Value {
    serde_json::json!({
        "permaId":{"id":review.id},
        "name":review.title,
        "description":review.objectives,
        "state":review.state,
    })
}

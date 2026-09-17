//! Shared, deterministic adapters for `crsu` integration tests.

use serde::Deserialize;
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
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
#[derive(Clone, Debug, Deserialize)]
pub struct LandReviewer {
    pub username: String,
    pub completed: bool,
}

/// One comments conversation: fetch comments and review items.
#[derive(Clone, Debug)]
pub struct CommentsFixture {
    pub token: String,
    pub review_id: String,
    pub comments: serde_json::Value,
    pub review_items: serde_json::Value,
    pub reply: Option<CommentsReplyExpectation>,
    pub resolution: Option<CommentsResolutionExpectation>,
    pub delete: Option<CommentsDeleteExpectation>,
    pub edit: Option<CommentsEditExpectation>,
    pub defect: Option<CommentsDefectExpectation>,
}

/// Expected POST when an e2e scenario replies to a comment.
#[derive(Clone, Debug, Deserialize)]
pub struct CommentsReplyExpectation {
    pub comment_id: String,
    pub message: String,
    pub reply_id: String,
}

/// Expected ajax POST when an e2e scenario changes comment resolution.
#[derive(Clone, Debug, Deserialize)]
pub struct CommentsResolutionExpectation {
    pub endpoint: String,
    pub comment_id: String,
    pub status: String,
    #[serde(default)]
    pub form_contains: Vec<(String, String)>,
}

/// Expected DELETE when an e2e scenario removes a comment.
#[derive(Clone, Debug, Deserialize)]
pub struct CommentsDeleteExpectation {
    pub comment_id: String,
    pub parent_id: Option<String>,
}

/// Expected POST when an e2e scenario rewrites a comment.
#[derive(Clone, Debug, Deserialize)]
pub struct CommentsEditExpectation {
    pub comment_id: String,
    pub parent_id: Option<String>,
    pub message: String,
}

/// Expected POST when an e2e scenario raises or clears a defect.
#[derive(Clone, Debug, Deserialize)]
pub struct CommentsDefectExpectation {
    pub comment_id: String,
    pub parent_id: Option<String>,
    pub defect: bool,
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

/// One in-process Crucible for a single fixture conversation.
pub struct MockCrucible {
    base_url: String,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct Shared {
    script: Script,
    review: Mutex<Option<ReviewSnapshot>>,
    interactions: Mutex<Vec<Interaction>>,
    errors: Mutex<Vec<String>>,
}

#[derive(Clone, Debug)]
enum Script {
    Init(Box<InitFixture>),
    Review(Box<ReviewFixture>),
    Land(Box<LandFixture>),
    Comments(Box<CommentsFixture>),
}

#[derive(Clone, Debug)]
struct Interaction {
    method: String,
    path: String,
    body: String,
}

impl MockCrucible {
    #[must_use]
    pub fn start(fixture: InitFixture) -> Self {
        Self::spawn(Script::Init(Box::new(fixture)), None)
    }

    /// Starts a Crucible adapter for one create-review request.
    #[must_use]
    pub fn start_review(fixture: ReviewFixture) -> Self {
        let review = match &fixture.response {
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
        };
        Self::spawn(Script::Review(Box::new(fixture)), review)
    }

    /// Starts a Crucible adapter for landing an accepted review.
    #[must_use]
    pub fn start_land(fixture: &LandFixture) -> Self {
        Self::spawn(Script::Land(Box::new(fixture.clone())), None)
    }

    /// Starts a Crucible adapter that serves comments and review items.
    #[must_use]
    pub fn start_comments(fixture: &CommentsFixture) -> Self {
        Self::spawn(Script::Comments(Box::new(fixture.clone())), None)
    }

    fn spawn(script: Script, review: Option<ReviewSnapshot>) -> Self {
        let server = Server::http("127.0.0.1:0").expect("start mock Crucible");
        let base_url = format!("http://{}", server.server_addr());
        let shared = Arc::new(Shared {
            script,
            review: Mutex::new(review),
            interactions: Mutex::new(Vec::new()),
            errors: Mutex::new(Vec::new()),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let thread_shared = Arc::clone(&shared);
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match server.recv_timeout(Duration::from_millis(50)) {
                    Ok(Some(request)) => {
                        if let Err(error) = handle_request(request, &thread_shared) {
                            thread_shared
                                .errors
                                .lock()
                                .expect("mock errors lock")
                                .push(error);
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        thread_shared
                            .errors
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
            shared,
            stop,
            thread: Some(thread),
        }
    }

    /// Returns the local HTTP origin of this mock server.
    #[must_use]
    pub fn base_url(&self) -> String {
        self.base_url.clone()
    }

    /// Returns the review exactly as a Crucible client would observe it.
    ///
    /// # Panics
    ///
    /// Panics if no review exists yet.
    #[must_use]
    pub fn review(&self) -> ReviewSnapshot {
        self.shared
            .review
            .lock()
            .expect("review state lock")
            .clone()
            .expect("review was not created")
    }

    /// Asserts that the fixture's expected mutation occurred.
    ///
    /// # Panics
    ///
    /// Panics when the mock recorded errors or the expected request is missing.
    pub fn assert_review_request(&self) {
        let errors = self.shared.errors.lock().expect("mock errors lock");
        assert!(errors.is_empty(), "mock Crucible errors: {errors:#?}");
        let interactions = self.shared.interactions.lock().expect("interactions lock");
        match &self.shared.script {
            Script::Init(_) => {}
            Script::Review(fixture) => {
                assert_review_script(fixture, &self.shared.review, &interactions);
            }
            Script::Land(fixture) => assert_recorded(
                &interactions,
                "POST",
                &format!("/rest-service/reviews-v1/{}/close", fixture.review_id),
                &[],
            ),
            Script::Comments(fixture) => assert_comments_script(fixture, &interactions),
        }
    }
}

impl Drop for MockCrucible {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("join mock Crucible");
        }
    }
}

fn assert_review_script(
    fixture: &ReviewFixture,
    review: &Mutex<Option<ReviewSnapshot>>,
    interactions: &[Interaction],
) {
    match &fixture.response {
        ReviewResponse::Rejected { .. } => {
            assert_recorded(interactions, "POST", "/rest-service/reviews-v1", &[]);
        }
        ReviewResponse::Created { .. } | ReviewResponse::Updated { .. } => {
            assert!(
                !interactions.is_empty(),
                "mock Crucible did not receive a review mutation"
            );
            assert!(
                review.lock().expect("review state lock").is_some(),
                "mock Crucible did not receive a review"
            );
        }
    }
}

fn assert_comments_script(fixture: &CommentsFixture, interactions: &[Interaction]) {
    if let Some(reply) = &fixture.reply {
        assert_recorded(
            interactions,
            "POST",
            &format!(
                "/rest-service/reviews-v1/{}/comments/{}/replies",
                fixture.review_id, reply.comment_id
            ),
            &[&reply.message],
        );
        return;
    }
    if let Some(resolution) = &fixture.resolution {
        let mut needles = vec![
            format!("resolutionStatus={}", resolution.status),
            format!("commentId={}", resolution.comment_id),
        ];
        needles.extend(
            resolution
                .form_contains
                .iter()
                .map(|(key, value)| format!("{key}={value}")),
        );
        let refs: Vec<&str> = needles.iter().map(String::as_str).collect();
        assert_recorded(
            interactions,
            "POST",
            &format!("/json/cru/{}/{}/", fixture.review_id, resolution.endpoint),
            &refs,
        );
        return;
    }
    if let Some(delete) = &fixture.delete {
        assert_recorded(
            interactions,
            "DELETE",
            &comment_http_path(
                &fixture.review_id,
                &delete.comment_id,
                delete.parent_id.as_deref(),
            ),
            &[],
        );
        return;
    }
    if let Some(edit) = &fixture.edit {
        assert_recorded(
            interactions,
            "POST",
            &comment_http_path(
                &fixture.review_id,
                &edit.comment_id,
                edit.parent_id.as_deref(),
            ),
            &[&edit.message],
        );
        return;
    }
    if let Some(defect) = &fixture.defect {
        let flag = if defect.defect {
            "\"defectRaised\":true"
        } else {
            "\"defectRaised\":false"
        };
        assert_recorded(
            interactions,
            "POST",
            &comment_http_path(
                &fixture.review_id,
                &defect.comment_id,
                defect.parent_id.as_deref(),
            ),
            &[flag],
        );
        return;
    }
    assert_recorded(
        interactions,
        "GET",
        &format!("/rest-service/reviews-v1/{}/comments", fixture.review_id),
        &[],
    );
    assert_recorded(
        interactions,
        "GET",
        &format!("/rest-service/reviews-v1/{}/reviewitems", fixture.review_id),
        &[],
    );
}

fn assert_recorded(interactions: &[Interaction], method: &str, path: &str, body: &[&str]) {
    let found = interactions.iter().any(|interaction| {
        interaction.method == method
            && interaction.path == path
            && body.iter().all(|needle| interaction.body.contains(needle))
    });
    assert!(
        found,
        "expected {method} {path} containing {body:?}, got {interactions:#?}"
    );
}

fn handle_request(mut request: tiny_http::Request, shared: &Shared) -> Result<(), String> {
    let path = request_path(request.url());
    let method = method_name(request.method()).to_owned();
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|error| error.to_string())?;
    shared
        .interactions
        .lock()
        .expect("interactions lock")
        .push(Interaction {
            method: method.clone(),
            path: path.clone(),
            body: body.clone(),
        });

    let (status, response_body) = match &shared.script {
        Script::Init(fixture) => handle_init(fixture, &method, &path, &body),
        Script::Review(fixture) => handle_review(fixture, &shared.review, &method, &path, &body)?,
        Script::Land(fixture) => handle_land(fixture, &method, &path),
        Script::Comments(fixture) => handle_comments(fixture, &method, &path),
    };

    let mut response = Response::from_string(response_body).with_status_code(StatusCode(status));
    if status != 204 {
        response.add_header(
            Header::from_bytes("content-type", "application/json").expect("valid header"),
        );
    }
    request.respond(response).map_err(|error| error.to_string())
}

fn handle_init(fixture: &InitFixture, method: &str, path: &str, body: &str) -> (u16, String) {
    if method == "POST" && path == "/rest-service-fecru/auth/login" {
        let username = format!("userName={}", fixture.username);
        let password = format!("password={}", fixture.password);
        if body.contains(&username) && body.contains(&password) {
            return (200, serde_json::json!({"token": fixture.token}).to_string());
        }
        return (401, String::new());
    }
    if method == "GET" && path == "/rest-service-fecru/server-v1" {
        return (
            200,
            serde_json::json!({
                "isCrucible": true,
                "isFishEye": fixture.is_fisheye,
            })
            .to_string(),
        );
    }
    if method == "GET" && path == "/rest-service/projects-v1" {
        return (
            200,
            serde_json::json!({
                "projectData": fixture
                    .projects
                    .iter()
                    .map(|key| serde_json::json!({"key": key}))
                    .collect::<Vec<_>>()
            })
            .to_string(),
        );
    }
    if method == "GET" && path == "/rest-service/repositories-v1" {
        return (
            200,
            serde_json::json!({
                "repoData": fixture
                    .repositories
                    .iter()
                    .map(|repository| serde_json::json!({
                        "name": repository.name,
                        "type": repository.scm_type,
                        "location": repository.location,
                        "enabled": repository.enabled,
                    }))
                    .collect::<Vec<_>>()
            })
            .to_string(),
        );
    }
    if method == "GET" && path == "/rest-service/users-v1" {
        return (
            200,
            serde_json::json!({
                "userData": fixture
                    .users
                    .iter()
                    .map(|user| serde_json::json!({
                        "userName": user.username,
                        "displayName": user.display_name,
                    }))
                    .collect::<Vec<_>>()
            })
            .to_string(),
        );
    }
    (404, format!("unhandled {method} {path}"))
}

fn handle_review(
    fixture: &ReviewFixture,
    review: &Mutex<Option<ReviewSnapshot>>,
    method: &str,
    path: &str,
    body: &str,
) -> Result<(u16, String), String> {
    if method == "POST" && path == "/rest-service/reviews-v1" {
        return match &fixture.response {
            ReviewResponse::Rejected { status, body } => Ok((*status, body.clone())),
            ReviewResponse::Created { review_id } => {
                let payload: CreateReviewRequest =
                    serde_json::from_str(body).map_err(|error| error.to_string())?;
                *locked_review(review)? = Some(ReviewSnapshot {
                    id: review_id.clone(),
                    title: payload.review_data.name,
                    objectives: payload.review_data.description,
                    state: "Draft".to_owned(),
                    reviewers: Vec::new(),
                });
                Ok((
                    200,
                    serde_json::json!({"permaId":{"id":review_id}}).to_string(),
                ))
            }
            ReviewResponse::Updated { .. } => Err("unexpected create-review request".to_owned()),
        };
    }
    if method == "GET"
        && let Some(review_id) = review_id(&fixture.response)
        && path == format!("/rest-service/reviews-v1/{review_id}")
    {
        let snapshot = locked_review(review)?
            .clone()
            .ok_or_else(|| "review does not exist".to_owned())?;
        return Ok((200, review_json(&snapshot).to_string()));
    }
    if method == "POST" && path.ends_with("/reviewers") {
        let reviewer = serde_json::from_str::<String>(body).unwrap_or_else(|_| body.to_owned());
        mutate_existing(review, |snapshot| snapshot.reviewers.push(reviewer))?;
        return Ok((204, String::new()));
    }
    if method == "POST" && path.ends_with("/transition") {
        mutate_existing(review, |snapshot| "Review".clone_into(&mut snapshot.state))?;
        return Ok((200, serde_json::json!({"state":"Review"}).to_string()));
    }
    if method == "POST" && path.ends_with("/patch") {
        return Ok((200, serde_json::json!({"state":"Draft"}).to_string()));
    }
    if method == "POST" && path.ends_with("/updateReviewTitleAjax") {
        let title = form_field(body, "title")?;
        mutate_existing(review, |snapshot| snapshot.title.clone_from(&title))?;
        return Ok((
            200,
            serde_json::json!({"worked":true,"title":title}).to_string(),
        ));
    }
    if method == "POST" && path.ends_with("/updateReviewObjectivesAjax") {
        let objectives = form_field(body, "input")?;
        mutate_existing(review, |snapshot| {
            snapshot.objectives.clone_from(&objectives);
        })?;
        return Ok((
            200,
            serde_json::json!({"worked":true,"payload":objectives}).to_string(),
        ));
    }
    Ok((404, format!("unhandled {method} {path}")))
}

fn handle_land(fixture: &LandFixture, method: &str, path: &str) -> (u16, String) {
    let review_path = format!("/rest-service/reviews-v1/{}", fixture.review_id);
    if method == "GET" && path == review_path {
        return (
            200,
            review_json(&ReviewSnapshot {
                id: fixture.review_id.clone(),
                title: fixture.title.clone(),
                objectives: fixture.objectives.clone(),
                state: fixture.state.clone(),
                reviewers: Vec::new(),
            })
            .to_string(),
        );
    }
    if method == "GET" && path == format!("{review_path}/reviewers") {
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
        return (200, serde_json::json!({"reviewer": reviewers}).to_string());
    }
    if method == "POST" && path == format!("{review_path}/close") {
        return (200, serde_json::json!({"state":"Closed"}).to_string());
    }
    (404, format!("unhandled {method} {path}"))
}

fn handle_comments(fixture: &CommentsFixture, method: &str, path: &str) -> (u16, String) {
    let prefix = format!("/rest-service/reviews-v1/{}", fixture.review_id);
    if method == "GET" && path == format!("{prefix}/comments") {
        return (200, fixture.comments.to_string());
    }
    if method == "GET" && path == format!("{prefix}/reviewitems") {
        return (200, fixture.review_items.to_string());
    }
    if let Some(reply) = &fixture.reply
        && method == "POST"
        && path == format!("{prefix}/comments/{}/replies", reply.comment_id)
    {
        return (
            200,
            serde_json::json!({"permaId":{"id": reply.reply_id}}).to_string(),
        );
    }
    if let Some(resolution) = &fixture.resolution
        && method == "POST"
        && path == format!("/json/cru/{}/{}/", fixture.review_id, resolution.endpoint)
    {
        return (200, serde_json::json!({"worked": true}).to_string());
    }
    if let Some(delete) = &fixture.delete
        && method == "DELETE"
        && path
            == comment_http_path(
                &fixture.review_id,
                &delete.comment_id,
                delete.parent_id.as_deref(),
            )
    {
        return (204, String::new());
    }
    if let Some(edit) = &fixture.edit
        && method == "POST"
        && path
            == comment_http_path(
                &fixture.review_id,
                &edit.comment_id,
                edit.parent_id.as_deref(),
            )
    {
        return (
            200,
            serde_json::json!({
                "permaId": {"id": edit.comment_id},
                "message": edit.message,
            })
            .to_string(),
        );
    }
    if let Some(defect) = &fixture.defect
        && method == "POST"
        && path
            == comment_http_path(
                &fixture.review_id,
                &defect.comment_id,
                defect.parent_id.as_deref(),
            )
    {
        return (
            200,
            serde_json::json!({
                "permaId": {"id": defect.comment_id},
                "defectRaised": defect.defect,
            })
            .to_string(),
        );
    }
    (404, format!("unhandled {method} {path}"))
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

fn review_id(response: &ReviewResponse) -> Option<&str> {
    match response {
        ReviewResponse::Created { review_id } | ReviewResponse::Updated { review_id, .. } => {
            Some(review_id)
        }
        ReviewResponse::Rejected { .. } => None,
    }
}

fn comment_http_path(review_id: &str, comment_id: &str, parent_id: Option<&str>) -> String {
    match parent_id {
        Some(parent_id) => format!(
            "/rest-service/reviews-v1/{review_id}/comments/{parent_id}/replies/{comment_id}"
        ),
        None => format!("/rest-service/reviews-v1/{review_id}/comments/{comment_id}"),
    }
}

fn review_json(review: &ReviewSnapshot) -> serde_json::Value {
    serde_json::json!({
        "permaId":{"id":review.id},
        "name":review.title,
        "description":review.objectives,
        "state":review.state,
    })
}

fn request_path(url: &str) -> String {
    url.split('?')
        .next()
        .unwrap_or(url)
        .replace("%3A", ":")
        .replace("%3a", ":")
}

fn method_name(method: &Method) -> &'static str {
    match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Delete => "DELETE",
        Method::Put => "PUT",
        Method::Head => "HEAD",
        Method::Options => "OPTIONS",
        Method::Patch => "PATCH",
        _ => "OTHER",
    }
}

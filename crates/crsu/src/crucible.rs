use crate::git_repository::ReviewDiff;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::env;
use std::fmt;

pub struct Client {
    http: reqwest::blocking::Client,
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
            http: reqwest::blocking::Client::new(),
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
        let response = authenticated(
            &self.http,
            &self.url,
            &self.token,
            reqwest::Method::GET,
            path.trim_start_matches('/'),
        )
        .send()
        .map_err(request_error)?;
        parse_json(&response_body(response)?)
    }
}

/// Builds a request against `base_url` authenticated with the Crucible `FEAUTH` token.
fn authenticated(
    http: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
    method: reqwest::Method,
    path: &str,
) -> reqwest::blocking::RequestBuilder {
    http.request(method, format!("{base_url}/{path}"))
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .query(&[("FEAUTH", token)])
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
    objectives_updated: bool,
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

    #[must_use]
    pub fn objectives_were_updated(&self) -> bool {
        self.objectives_updated
    }
}

pub fn submit_if_configured(review_diff: &ReviewDiff) -> Result<Option<Submission>, CrucibleError> {
    let Some(config) = Config::from_environment()? else {
        return Ok(None);
    };
    config.validate_anchor()?;

    if let Some(review_id) = review_to_update(&config, review_diff.review_id())? {
        let update = update_review(&config, review_id, review_diff)?;
        return Ok(Some(Submission {
            review_id: review_id.to_owned(),
            review_url: review_url(&config.url, review_id),
            created: false,
            reviewers: config.reviewers,
            title_update: update.title,
            objectives_updated: update.objectives,
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

    let response = config_request(&config, reqwest::Method::POST, "rest-service/reviews-v1")
        .header(reqwest::header::CONTENT_TYPE, "application/json")
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
        let reviewer_response = config_request(
            &config,
            reqwest::Method::POST,
            &format!("rest-service/reviews-v1/{review_id}/reviewers"),
        )
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
        review_url: review_url(&config.url, review_id),
        created: true,
        reviewers: config.reviewers,
        title_update: None,
        objectives_updated: false,
    }))
}

/// Returns the interactive confirmation prompt when a Crucible submit is configured.
///
/// # Errors
///
/// Returns configuration or anchor-validation errors before the caller prompts.
pub fn submit_confirmation(review_diff: &ReviewDiff) -> Result<Option<String>, CrucibleError> {
    let Some(config) = Config::from_environment()? else {
        return Ok(None);
    };
    config.validate_anchor()?;
    Ok(Some(
        if let Some(review_id) = review_to_update(&config, review_diff.review_id())? {
            format!("submit update patch to {review_id}? [y/N] ")
        } else {
            format!("submit new review to {}? [y/N] ", config.project)
        },
    ))
}

fn review_to_update<'a>(
    config: &Config,
    review_id: Option<&'a str>,
) -> Result<Option<&'a str>, CrucibleError> {
    let Some(review_id) = review_id else {
        return Ok(None);
    };
    let review = get_json(config, &format!("rest-service/reviews-v1/{review_id}"))?;
    let state = review.get("state").and_then(Value::as_str).unwrap_or("");
    if finished_review_state(state) {
        Ok(None)
    } else {
        Ok(Some(review_id))
    }
}

pub(crate) fn finished_review_state(state: &str) -> bool {
    matches!(state, "Closed" | "Dead" | "Abandoned")
}

pub struct LandReview {
    pub review_id: String,
    pub review_url: String,
    pub state: String,
    pub objectives: String,
    pub reviewers: Vec<String>,
    pub reviewed_by: Vec<String>,
}

/// One reviewer row from `GET .../reviewers`.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ReviewerStatus {
    pub username: String,
    pub completed: bool,
}

/// Protocol snapshot of one review: `GET reviews-v1/{id}` plus reviewers.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ReviewStatus {
    pub review_id: String,
    pub url: String,
    pub title: String,
    pub state: String,
    pub objectives: String,
    pub target: Option<String>,
    pub reviewers: Vec<ReviewerStatus>,
}

/// Reads review state and reviewers without deciding whether land is allowed.
pub fn inspect_review(config: &Config, review_id: &str) -> Result<LandReview, CrucibleError> {
    let status = review_status(config, review_id)?;
    Ok(LandReview {
        review_id: status.review_id,
        review_url: status.url,
        state: status.state,
        objectives: status.objectives,
        reviewers: status
            .reviewers
            .iter()
            .map(|reviewer| reviewer.username.clone())
            .collect(),
        reviewed_by: status
            .reviewers
            .iter()
            .filter(|reviewer| reviewer.completed)
            .map(|reviewer| reviewer.username.clone())
            .collect(),
    })
}

/// Reads `[ target: ]` from a review description. Used by `copy` when git fallback still looks like a feature branch.
pub(crate) fn review_copy_target(config: &Config, review_id: &str) -> Option<String> {
    let review = get_json(config, &format!("rest-service/reviews-v1/{review_id}")).ok()?;
    let objectives = review.get("description").and_then(Value::as_str)?;
    crate::git_repository::target_from_objectives(objectives).map(str::to_owned)
}

/// Fetches one review and its reviewers from Crucible.
pub fn review_status(config: &Config, review_id: &str) -> Result<ReviewStatus, CrucibleError> {
    let review = get_json(config, &format!("rest-service/reviews-v1/{review_id}"))?;
    let title = review
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let state = review
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let objectives = review
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let reviewers_json = get_json(
        config,
        &format!("rest-service/reviews-v1/{review_id}/reviewers"),
    )?;
    let mut reviewers = Vec::new();
    if let Some(entries) = reviewers_json.get("reviewer").and_then(Value::as_array) {
        for entry in entries {
            let Some(username) = entry.get("userName").and_then(Value::as_str) else {
                continue;
            };
            reviewers.push(ReviewerStatus {
                username: username.to_owned(),
                completed: entry.get("completed").and_then(Value::as_bool) == Some(true),
            });
        }
    }
    Ok(ReviewStatus {
        review_id: review_id.to_owned(),
        url: review_url(&config.url, review_id),
        title,
        state,
        target: crate::git_repository::target_from_objectives(&objectives).map(str::to_owned),
        objectives,
        reviewers,
    })
}

pub fn land_review(config: &Config, review_id: &str) -> Result<LandReview, CrucibleError> {
    let review = inspect_review(config, review_id)?;
    if finished_review_state(&review.state) {
        return Err(CrucibleError::ReviewFinished {
            review_id: review.review_id,
            state: review.state,
        });
    }
    if review.reviewed_by.is_empty() {
        return Err(CrucibleError::WaitingForReview);
    }
    Ok(review)
}

/// Closes a Crucible review after a successful land push. Already-closed reviews succeed.
///
/// `state` is the review state the caller already read, so an accepted review is not
/// fetched a second time.
pub fn close_review(config: &Config, review_id: &str, state: &str) -> Result<(), CrucibleError> {
    if state == "Closed" {
        return Ok(());
    }
    let response = config_request(
        config,
        reqwest::Method::POST,
        &format!("rest-service/reviews-v1/{review_id}/close"),
    )
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body("{}")
    .send()
    .map_err(request_error)?;
    response_body(response)?;
    Ok(())
}

/// A review comment flattened for agent and CLI consumers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReviewComment {
    pub id: String,
    pub kind: CommentKind,
    pub author: String,
    pub message: String,
    pub draft: bool,
    pub deleted: bool,
    pub defect: bool,
    pub path: Option<String>,
    pub line: Option<String>,
    pub created: Option<String>,
    pub replies: Vec<ReviewComment>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentKind {
    General,
    Line,
}

/// Comments visible on a review, already mapped onto file paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReviewComments {
    pub review_id: String,
    pub comments: Vec<ReviewComment>,
}

/// Loads every visible comment on a review.
///
/// # Errors
///
/// Returns configuration or HTTP errors.
pub fn review_comments(review_id: &str) -> Result<ReviewComments, CrucibleError> {
    let config = configured()?;
    let comments = get_json(
        &config,
        &format!("rest-service/reviews-v1/{review_id}/comments"),
    )?;
    let items = get_json(
        &config,
        &format!("rest-service/reviews-v1/{review_id}/reviewitems"),
    )?;
    Ok(parse_review_comments(review_id, &comments, &items))
}

/// One uploaded patch on a review, without the diff body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReviewPatch {
    pub id: String,
    pub source: String,
    pub file: String,
    pub uploaded: Option<String>,
    pub comments: usize,
    pub latest: bool,
}

/// Patches visible on a review.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReviewPatches {
    pub review_id: String,
    pub patches: Vec<ReviewPatch>,
}

/// A patch left in place because it still has line comments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PatchSkip {
    pub id: String,
    pub reason: String,
}

/// Result of deleting one or more patches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PatchChange {
    pub review_id: String,
    pub deleted: Vec<String>,
    pub kept: Vec<String>,
    pub skipped: Vec<PatchSkip>,
}

/// Lists every patch on a review.
///
/// # Errors
///
/// Returns configuration or HTTP errors.
pub fn review_patches(review_id: &str) -> Result<ReviewPatches, CrucibleError> {
    let config = configured()?;
    Ok(ReviewPatches {
        review_id: review_id.to_owned(),
        patches: load_patches(&config, review_id)?,
    })
}

/// Deletes the named patches. Patches with live line comments are skipped.
///
/// # Errors
///
/// Returns configuration, lookup, or HTTP errors.
pub fn delete_patches(review_id: &str, patch_ids: &[String]) -> Result<PatchChange, CrucibleError> {
    if patch_ids.is_empty() {
        return Err(CrucibleError::NoPatchesToUpdate);
    }
    let config = configured()?;
    let patches = load_patches(&config, review_id)?;
    apply_patch_deletes(&config, review_id, &patches, patch_ids)
}

/// Deletes every uncommented patch except the newest.
///
/// # Errors
///
/// Returns configuration or HTTP errors.
pub fn prune_patches(review_id: &str) -> Result<PatchChange, CrucibleError> {
    let config = configured()?;
    let patches = load_patches(&config, review_id)?;
    let stale: Vec<String> = patches
        .iter()
        .filter(|patch| !patch.latest)
        .map(|patch| patch.id.clone())
        .collect();
    apply_patch_deletes(&config, review_id, &patches, &stale)
}

fn load_patches(config: &Config, review_id: &str) -> Result<Vec<ReviewPatch>, CrucibleError> {
    let groups = get_json(
        config,
        &format!("rest-service/reviews-v1/{review_id}/patch"),
    )?;
    let comments = get_json(
        config,
        &format!("rest-service/reviews-v1/{review_id}/comments"),
    )?;
    let items = get_json(
        config,
        &format!("rest-service/reviews-v1/{review_id}/reviewitems"),
    )?;
    Ok(inventory_from_payloads(&groups, &comments, &items))
}

fn apply_patch_deletes(
    config: &Config,
    review_id: &str,
    patches: &[ReviewPatch],
    patch_ids: &[String],
) -> Result<PatchChange, CrucibleError> {
    let mut deleted = Vec::new();
    let mut skipped = Vec::new();
    for requested in patch_ids {
        let id = patch_id_key(requested);
        let Some(patch) = patches.iter().find(|patch| patch.id == id) else {
            return Err(CrucibleError::PatchNotFound(id));
        };
        if patch.comments > 0 {
            skipped.push(PatchSkip {
                id,
                reason: "has_comments".to_owned(),
            });
            continue;
        }
        let response = config_request(
            config,
            reqwest::Method::DELETE,
            &format!("rest-service/reviews-v1/{review_id}/patch/{id}"),
        )
        .send()
        .map_err(request_error)?;
        response_body(response)?;
        deleted.push(id);
    }
    let kept = patches
        .iter()
        .map(|patch| patch.id.clone())
        .filter(|id| !deleted.contains(id))
        .collect();
    Ok(PatchChange {
        review_id: review_id.to_owned(),
        deleted,
        kept,
        skipped,
    })
}

fn inventory_from_payloads(groups: &Value, comments: &Value, items: &Value) -> Vec<ReviewPatch> {
    let item_patches = review_item_patch_ids(items);
    let mut counts = BTreeMap::<String, usize>::new();
    visit_live_comments(comments, &mut |comment| {
        if let Some(item_id) = review_item_id(comment)
            && let Some(patch_id) = item_patches.get(item_id)
        {
            *counts.entry(patch_id.clone()).or_default() += 1;
        }
    });

    let mut patches = Vec::new();
    for group in patch_groups(groups) {
        let source = group
            .get("sourceName")
            .and_then(Value::as_str)
            .unwrap_or_default();
        for patch in group
            .get("patches")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let Some(id) = json_id(patch) else {
                continue;
            };
            patches.push(ReviewPatch {
                comments: counts.get(&id).copied().unwrap_or_default(),
                source: if source.is_empty() {
                    format!("PATCH:{id}")
                } else {
                    source.to_owned()
                },
                file: patch
                    .get("fileName")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                uploaded: json_timestamp(patch.get("uploadDate")),
                id,
                latest: false,
            });
        }
    }
    if let Some(latest) = patches
        .iter()
        .enumerate()
        .max_by_key(|(_, patch)| (patch.uploaded.clone(), patch.id.clone()))
        .map(|(index, _)| index)
    {
        patches[latest].latest = true;
    }
    patches
}

fn patch_groups(value: &Value) -> impl Iterator<Item = &Value> {
    value
        .get("patchGroup")
        .and_then(Value::as_array)
        .map(|groups| groups.iter())
        .into_iter()
        .flatten()
}

fn review_item_patch_ids(items: &Value) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Some(entries) = items.get("reviewItem").and_then(Value::as_array) else {
        return map;
    };
    for item in entries {
        let Some(id) = item
            .pointer("/permId/id")
            .or_else(|| item.pointer("/permaId/id"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if let Some(patch_id) = item_patch_id(item) {
            map.insert(id.to_owned(), patch_id);
        }
    }
    map
}

fn item_patch_id(item: &Value) -> Option<String> {
    if let Some(url) = item.get("patchUrl").and_then(Value::as_str)
        && let Some((_, rest)) = url.split_once("/downloadpatch/")
        && let Some(id) = rest.split('/').next()
        && !id.is_empty()
    {
        return Some(id.to_owned());
    }
    item.get("repositoryName")
        .and_then(Value::as_str)
        .map(patch_id_key)
        .filter(|id| !id.is_empty())
}

fn visit_live_comments(value: &Value, visit: &mut impl FnMut(&Value)) {
    for comment in comment_values(value) {
        if !comment
            .get("deleted")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            visit(comment);
        }
        if let Some(replies) = comment.get("replies") {
            visit_live_comments(replies, visit);
        }
    }
}

fn patch_id_key(name: &str) -> String {
    let text = name.trim();
    text.strip_prefix("PATCH:")
        .or_else(|| text.strip_prefix("PATCH-"))
        .unwrap_or(text)
        .to_owned()
}

fn json_id(value: &Value) -> Option<String> {
    match value.get("id")? {
        Value::Number(value) => Some(value.to_string()),
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn json_timestamp(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

/// A reply posted under an existing review comment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommentReply {
    pub review_id: String,
    pub comment_id: String,
    pub reply_id: String,
    pub message: String,
}

/// Resolution written back to the review UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStatus {
    Unresolved,
    Resolved,
}

impl ResolutionStatus {
    fn as_ajax(self) -> &'static str {
        match self {
            Self::Unresolved => "UNRESOLVED",
            Self::Resolved => "RESOLVED",
        }
    }
}

/// One comment after a resolution change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommentResolutionResult {
    pub comment_id: String,
    pub status: ResolutionStatus,
}

/// Result of marking one or more comments Needs resolution / Resolved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommentResolution {
    pub review_id: String,
    pub results: Vec<CommentResolutionResult>,
}

/// Replies to a review comment.
///
/// # Errors
///
/// Returns configuration, validation, or HTTP errors.
pub fn reply_comment(
    review_id: &str,
    comment_id: &str,
    message: &str,
) -> Result<CommentReply, CrucibleError> {
    if message.trim().is_empty() {
        return Err(CrucibleError::EmptyComment);
    }
    let config = configured()?;
    let comment_id = rest_comment_id(comment_id);
    let response = post_json(
        &config,
        &format!("rest-service/reviews-v1/{review_id}/comments/{comment_id}/replies"),
        &json!({"message": message, "draft": false}),
    )?;
    Ok(CommentReply {
        review_id: review_id.to_owned(),
        comment_id,
        reply_id: response
            .pointer("/permaId/id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        message: message.to_owned(),
    })
}

/// Result of deleting a review comment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommentDelete {
    pub review_id: String,
    pub comment_id: String,
    pub deleted: bool,
}

/// Result of rewriting a review comment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommentEdit {
    pub review_id: String,
    pub comment_id: String,
    pub message: String,
}

/// Deletes a review comment or reply.
///
/// # Errors
///
/// Returns configuration, lookup, or HTTP errors.
pub fn delete_comment(review_id: &str, comment_id: &str) -> Result<CommentDelete, CrucibleError> {
    let config = configured()?;
    let path = comment_resource(&config, review_id, comment_id)?;
    let response = config_request(&config, reqwest::Method::DELETE, &path)
        .send()
        .map_err(request_error)?;
    response_body(response)?;
    Ok(CommentDelete {
        review_id: review_id.to_owned(),
        comment_id: rest_comment_id(comment_id),
        deleted: true,
    })
}

/// Rewrites a review comment or reply.
///
/// # Errors
///
/// Returns configuration, validation, lookup, or HTTP errors.
pub fn edit_comment(
    review_id: &str,
    comment_id: &str,
    message: &str,
) -> Result<CommentEdit, CrucibleError> {
    if message.trim().is_empty() {
        return Err(CrucibleError::EmptyComment);
    }
    let config = configured()?;
    let path = comment_resource(&config, review_id, comment_id)?;
    post_json(&config, &path, &json!({"message": message}))?;
    Ok(CommentEdit {
        review_id: review_id.to_owned(),
        comment_id: rest_comment_id(comment_id),
        message: message.to_owned(),
    })
}

/// Sets comment resolution to Needs resolution or Resolved.
///
/// # Errors
///
/// Returns configuration, lookup, or HTTP errors.
pub fn set_comment_resolutions(
    review_id: &str,
    comment_ids: &[String],
    status: ResolutionStatus,
    all_top_level: bool,
) -> Result<CommentResolution, CrucibleError> {
    let config = configured()?;
    let comments = get_json(
        &config,
        &format!("rest-service/reviews-v1/{review_id}/comments"),
    )?;
    let targets = resolution_targets(&comments, comment_ids, all_top_level)?;
    let mut results = Vec::new();
    for target in targets {
        apply_resolution(&config, review_id, &target, status)?;
        results.push(CommentResolutionResult {
            comment_id: rest_comment_id(&target.id),
            status,
        });
    }
    Ok(CommentResolution {
        review_id: review_id.to_owned(),
        results,
    })
}

/// One comment after a defect flag change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommentDefectResult {
    pub comment_id: String,
    pub defect: bool,
}

/// Result of raising or clearing defects on comments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommentDefects {
    pub review_id: String,
    pub results: Vec<CommentDefectResult>,
}

/// Raises or clears the Crucible defect flag on comments.
///
/// # Errors
///
/// Returns configuration, lookup, or HTTP errors.
pub fn set_comment_defects(
    review_id: &str,
    comment_ids: &[String],
    defect: bool,
    all_top_level: bool,
) -> Result<CommentDefects, CrucibleError> {
    let config = configured()?;
    let comments = get_json(
        &config,
        &format!("rest-service/reviews-v1/{review_id}/comments"),
    )?;
    let targets = resolution_targets(&comments, comment_ids, all_top_level)?;
    let mut results = Vec::new();
    for target in targets {
        let path =
            comment_resource_path(review_id, target.parent, &comment_perma_id(target.comment));
        let message = target
            .comment
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        post_json(
            &config,
            &path,
            &json!({"message": message, "defectRaised": defect}),
        )?;
        results.push(CommentDefectResult {
            comment_id: rest_comment_id(&target.id),
            defect,
        });
    }
    Ok(CommentDefects {
        review_id: review_id.to_owned(),
        results,
    })
}

pub(crate) fn configured() -> Result<Config, CrucibleError> {
    Config::from_environment()?.ok_or(CrucibleError::NotConfigured)
}

/// The Crucible web URL of a review.
pub(crate) fn review_url(base_url: &str, review_id: &str) -> String {
    format!("{}/cru/{review_id}", base_url.trim_end_matches('/'))
}

fn config_request(
    config: &Config,
    method: reqwest::Method,
    path: &str,
) -> reqwest::blocking::RequestBuilder {
    authenticated(&config.http, &config.url, &config.token, method, path)
}

/// Builds a Crucible ajax request. `endpoint` is the trailing path segment, including
/// the trailing slash some endpoints require.
fn ajax_request(
    config: &Config,
    review_id: &str,
    endpoint: &str,
) -> reqwest::blocking::RequestBuilder {
    config_request(
        config,
        reqwest::Method::POST,
        &format!("json/cru/{review_id}/{endpoint}"),
    )
    .header("X-Atlassian-Token", "no-check")
}

/// Sends a JSON POST. Endpoints that answer with an empty body yield [`Value::Null`].
fn post_json(config: &Config, path: &str, payload: &Value) -> Result<Value, CrucibleError> {
    let body = post_json_body(config, path, payload)?;
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    parse_json(&body)
}

fn post_json_body(config: &Config, path: &str, payload: &Value) -> Result<String, CrucibleError> {
    let response = config_request(config, reqwest::Method::POST, path)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(payload)
        .send()
        .map_err(request_error)?;
    response_body(response)
}

/// One comment selected for a resolution change, already located in the comment tree.
struct ResolutionTarget<'a> {
    id: String,
    parent: Option<&'a Value>,
    comment: &'a Value,
}

fn resolution_targets<'a>(
    comments: &'a Value,
    comment_ids: &[String],
    all_top_level: bool,
) -> Result<Vec<ResolutionTarget<'a>>, CrucibleError> {
    let targets: Vec<ResolutionTarget<'a>> = if all_top_level {
        comment_values(comments)
            .filter_map(|comment| {
                let id = comment_perma_id(comment);
                (!id.is_empty()).then_some(ResolutionTarget {
                    id,
                    parent: None,
                    comment,
                })
            })
            .collect()
    } else {
        comment_ids
            .iter()
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .map(|id| {
                let (parent, comment) = find_comment(comments, id)?;
                Ok(ResolutionTarget {
                    id: id.to_owned(),
                    parent,
                    comment,
                })
            })
            .collect::<Result<Vec<_>, CrucibleError>>()?
    };
    if targets.is_empty() {
        return Err(CrucibleError::NoCommentsToUpdate);
    }
    Ok(targets)
}

fn comment_resource(
    config: &Config,
    review_id: &str,
    comment_id: &str,
) -> Result<String, CrucibleError> {
    let comments = get_json(
        config,
        &format!("rest-service/reviews-v1/{review_id}/comments"),
    )?;
    let (parent, comment) = find_comment(&comments, comment_id)?;
    Ok(comment_resource_path(
        review_id,
        parent,
        &comment_perma_id(comment),
    ))
}

fn comment_resource_path(review_id: &str, parent: Option<&Value>, comment_id: &str) -> String {
    let comment_id = rest_comment_id(comment_id);
    match parent {
        Some(parent) => format!(
            "rest-service/reviews-v1/{review_id}/comments/{}/replies/{comment_id}",
            rest_comment_id(&comment_perma_id(parent))
        ),
        None => format!("rest-service/reviews-v1/{review_id}/comments/{comment_id}"),
    }
}

fn find_comment<'a>(
    comments: &'a Value,
    comment_id: &str,
) -> Result<(Option<&'a Value>, &'a Value), CrucibleError> {
    let wanted_numeric = numeric_comment_id(comment_id);
    let wanted_rest = rest_comment_id(comment_id);
    for comment in comment_values(comments) {
        if comment_id_matches(comment, &wanted_numeric, &wanted_rest) {
            return Ok((None, comment));
        }
        for reply in comment.get("replies").into_iter().flat_map(comment_values) {
            if comment_id_matches(reply, &wanted_numeric, &wanted_rest) {
                return Ok((Some(comment), reply));
            }
        }
    }
    Err(CrucibleError::CommentNotFound(comment_id.to_owned()))
}

fn comment_id_matches(comment: &Value, numeric: &str, rest: &str) -> bool {
    let perma = comment_perma_id(comment);
    perma == rest || numeric_comment_id(&perma) == numeric
}

#[derive(Debug, PartialEq, Eq)]
struct ResolutionPost {
    endpoint: String,
    fields: Vec<(String, String)>,
}

fn resolution_post(
    parent: Option<&Value>,
    comment: &Value,
    comment_id: &str,
    status: ResolutionStatus,
) -> ResolutionPost {
    let perma = comment_perma_id(comment);
    let numeric_id = numeric_comment_id(if perma.is_empty() { comment_id } else { &perma });
    let mut fields = vec![
        ("resolutionStatus".to_owned(), status.as_ajax().to_owned()),
        ("commentId".to_owned(), numeric_id),
    ];
    if let Some(parent) = parent {
        fields.push((
            "replyToId".to_owned(),
            numeric_comment_id(&comment_perma_id(parent)),
        ));
        fields.push(("type".to_owned(), "reply".to_owned()));
        return ResolutionPost {
            endpoint: "replyCommentResolutionAjax".to_owned(),
            fields,
        };
    }
    if let Some(frx_id) = numeric_frx_id(comment) {
        let line = comment_to_line_range(comment);
        let kind = if line.is_empty() {
            "revision"
        } else {
            "inline"
        };
        fields.push(("frxId".to_owned(), frx_id));
        fields.push(("toLineRange".to_owned(), line));
        fields.push(("fromLineRange".to_owned(), String::new()));
        fields.push(("type".to_owned(), kind.to_owned()));
        return ResolutionPost {
            endpoint: "revisionCommentResolutionAjax".to_owned(),
            fields,
        };
    }
    fields.push(("type".to_owned(), "general".to_owned()));
    ResolutionPost {
        endpoint: "generalCommentResolutionAjax".to_owned(),
        fields,
    }
}

fn apply_resolution(
    config: &Config,
    review_id: &str,
    target: &ResolutionTarget<'_>,
    status: ResolutionStatus,
) -> Result<(), CrucibleError> {
    let post = resolution_post(target.parent, target.comment, &target.id, status);
    match post_resolution(config, review_id, &post) {
        Ok(()) => Ok(()),
        Err(error)
            if status == ResolutionStatus::Resolved && is_null_to_resolved(&error) =>
        {
            post_resolution(
                config,
                review_id,
                &resolution_post(
                    target.parent,
                    target.comment,
                    &target.id,
                    ResolutionStatus::Unresolved,
                ),
            )?;
            post_resolution(config, review_id, &post)
        }
        Err(error) => Err(error),
    }
}

fn is_null_to_resolved(error: &CrucibleError) -> bool {
    matches!(
        error,
        CrucibleError::HttpResponse { detail, .. } if detail.contains("from null to RESOLVED")
    )
}

fn post_resolution(
    config: &Config,
    review_id: &str,
    post: &ResolutionPost,
) -> Result<(), CrucibleError> {
    let form: Vec<(&str, &str)> = post
        .fields
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let response = ajax_request(config, review_id, &format!("{}/", post.endpoint))
        .form(&form)
        .send()
        .map_err(request_error)?;
    response_body(response)?;
    Ok(())
}

fn rest_comment_id(comment_id: &str) -> String {
    format!("CMT:{}", numeric_comment_id(comment_id))
}

fn numeric_comment_id(comment_id: &str) -> String {
    let text = comment_id.trim();
    text.strip_prefix("CMT:")
        .or_else(|| text.strip_prefix("CMT-"))
        .unwrap_or(text)
        .to_owned()
}

fn comment_perma_id(comment: &Value) -> String {
    json_text(comment.get("permaId"))
        .or_else(|| json_text(comment.get("permId")))
        .unwrap_or_default()
}

fn json_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Object(object) => match object.get("id") {
            Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
            Some(Value::Number(number)) => Some(number.to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// Reads whichever review-item id field Crucible returned for a comment.
fn review_item_id(comment: &Value) -> Option<&str> {
    comment
        .pointer("/reviewItemId/id")
        .or_else(|| comment.pointer("/reviewItem/permId/id"))
        .or_else(|| comment.pointer("/reviewItem/permaId/id"))
        .and_then(Value::as_str)
}

fn numeric_frx_id(comment: &Value) -> Option<String> {
    let id = review_item_id(comment)?;
    Some(id.strip_prefix("CFR-").unwrap_or(id).to_owned())
}

fn comment_to_line_range(comment: &Value) -> String {
    comment
        .get("toLineRange")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| comment_line(comment))
        .unwrap_or_default()
}

fn parse_review_comments(review_id: &str, comments: &Value, items: &Value) -> ReviewComments {
    let paths = review_item_paths(items);
    ReviewComments {
        review_id: review_id.to_owned(),
        comments: comment_values(comments)
            .filter_map(|comment| parse_comment(comment, &paths))
            .collect(),
    }
}

fn review_item_paths(items: &Value) -> BTreeMap<String, String> {
    let mut paths = BTreeMap::new();
    let Some(entries) = items.get("reviewItem").and_then(Value::as_array) else {
        return paths;
    };
    for item in entries {
        let Some(id) = item
            .pointer("/permId/id")
            .or_else(|| item.pointer("/permaId/id"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let path = item
            .get("toPath")
            .or_else(|| item.get("fromPath"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !path.is_empty() {
            paths.insert(id.to_owned(), path.to_owned());
        }
    }
    paths
}

/// Iterates the comments nested in a Crucible comment collection or a single comment.
fn comment_values(value: &Value) -> impl Iterator<Item = &Value> {
    value
        .get("comments")
        .and_then(Value::as_array)
        .or_else(|| {
            value
                .pointer("/generalComments/comments")
                .and_then(Value::as_array)
        })
        .or_else(|| value.as_array())
        .map(|comments| comments.iter())
        .into_iter()
        .flatten()
}

fn parse_comment(comment: &Value, paths: &BTreeMap<String, String>) -> Option<ReviewComment> {
    let message = comment.get("message").and_then(Value::as_str)?;
    let item_id = review_item_id(comment);
    let replies = comment
        .get("replies")
        .into_iter()
        .flat_map(comment_values)
        .filter_map(|reply| parse_comment(reply, paths))
        .collect();
    Some(ReviewComment {
        id: comment_perma_id(comment),
        kind: if item_id.is_some() {
            CommentKind::Line
        } else {
            CommentKind::General
        },
        author: comment
            .pointer("/user/userName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        message: message.to_owned(),
        draft: comment
            .get("draft")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        deleted: comment
            .get("deleted")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        defect: comment
            .get("defectRaised")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        path: item_id.and_then(|id| paths.get(id).cloned()),
        line: comment_line(comment),
        created: comment_created(comment),
        replies,
    })
}

fn comment_line(comment: &Value) -> Option<String> {
    let ranges = comment.get("lineRanges")?.as_array()?;
    ranges.last()?.get("range")?.as_str().map(str::to_owned)
}

fn comment_created(comment: &Value) -> Option<String> {
    match comment.get("createDate")? {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn get_json(config: &Config, path: &str) -> Result<Value, CrucibleError> {
    let response = config_request(config, reqwest::Method::GET, path)
        .send()
        .map_err(request_error)?;
    parse_json(&response_body(response)?)
}

fn start_review(config: &Config, review_id: &str) -> Result<(), CrucibleError> {
    let response = config_request(
        config,
        reqwest::Method::POST,
        &format!("rest-service/reviews-v1/{review_id}/transition"),
    )
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .query(&[("action", "action:approveReview")])
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
) -> Result<ReviewUpdate, CrucibleError> {
    let review = get_json(config, &format!("rest-service/reviews-v1/{review_id}"))?;
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
    let patch = config_request(
        config,
        reqwest::Method::POST,
        &format!("rest-service/reviews-v1/{review_id}/patch"),
    )
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
    let objectives = current_objectives != review_diff.objectives();
    if objectives {
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
    Ok(ReviewUpdate { title, objectives })
}

struct ReviewUpdate {
    title: Option<TitleUpdate>,
    objectives: bool,
}

fn update_review_ajax(
    config: &Config,
    review_id: &str,
    endpoint: &str,
    form: &[(&str, &str)],
    rejected: CrucibleError,
) -> Result<(), CrucibleError> {
    let response = ajax_request(config, review_id, endpoint)
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

pub(crate) struct Config {
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
            return match crate::project_config::ProjectConfig::load_resolved() {
                Ok(Some(config)) => Self::from_stored(config).map(Some),
                Ok(None) => Ok(None),
                Err(error) => Err(CrucibleError::ProjectConfiguration(error)),
            };
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

    fn from_stored(config: crate::project_config::ProjectConfig) -> Result<Self, CrucibleError> {
        Ok(Self {
            http: reqwest::blocking::Client::new(),
            url: require_stored("url", config.crucible.url)?,
            project: require_stored("project", config.crucible.project)?,
            token: require_stored("token", config.crucible.token)?,
            repository: config.crucible.repository,
            repository_location: config.crucible.repository_location,
            reviewers: config.crucible.reviewers,
        })
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
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or(CrucibleError::MissingConfiguration(name))
}

fn require_stored(field: &'static str, value: String) -> Result<String, CrucibleError> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        Err(CrucibleError::IncompleteConfiguration(field))
    } else {
        Ok(value)
    }
}

#[derive(Debug)]
pub enum CrucibleError {
    MalformedResponse,
    MalformedLogin,
    InvalidJson(String),
    MalformedCandidates,
    TitleUpdateRejected,
    ObjectivesUpdateRejected,
    WaitingForReview,
    ReviewFinished {
        review_id: String,
        state: String,
    },
    EmptyComment,
    CommentNotFound(String),
    NoCommentsToUpdate,
    PatchNotFound(String),
    NoPatchesToUpdate,
    MissingConfiguration(&'static str),
    IncompleteConfiguration(&'static str),
    NotConfigured,
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
            Self::WaitingForReview => write!(formatter, "waiting for review"),
            Self::ReviewFinished { review_id, state } => {
                write!(formatter, "{review_id} is {state}; refuse to land")
            }
            Self::EmptyComment => write!(formatter, "comment message must not be empty"),
            Self::CommentNotFound(comment_id) => {
                write!(formatter, "comment not found: {comment_id}")
            }
            Self::NoCommentsToUpdate => {
                write!(
                    formatter,
                    "no comments to update; pass comment ids or --all"
                )
            }
            Self::PatchNotFound(patch_id) => write!(formatter, "patch not found: {patch_id}"),
            Self::NoPatchesToUpdate => write!(formatter, "no patches to delete; pass patch ids"),
            Self::MissingConfiguration(name) => write!(formatter, "missing {name}"),
            Self::IncompleteConfiguration(field) => write!(
                formatter,
                "configuration is incomplete: {field} is empty; run `crsu init`"
            ),
            Self::NotConfigured => write!(
                formatter,
                "not configured; run `crsu init` or set CRSU_CRUCIBLE_URL / CRSU_CRUCIBLE_PROJECT / CRSU_CRUCIBLE_TOKEN"
            ),
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
    use super::{Client, CrucibleError};
    use httpmock::Method::{GET, POST};
    use httpmock::MockServer;

    #[test]
    fn incomplete_configuration_points_at_init() {
        let error = CrucibleError::IncompleteConfiguration("project");
        let message = error.to_string();
        assert!(message.contains("project is empty"));
        assert!(message.contains("crsu init"));
    }

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
    fn flattens_general_and_line_comments_onto_review_items() {
        let comments = serde_json::json!({
            "comments": [
                {
                    "permaId": {"id": "CMT:1"},
                    "message": "Looks good",
                    "draft": false,
                    "deleted": false,
                    "defectRaised": false,
                    "user": {"userName": "alice"},
                    "createDate": "2024-01-02T03:04:05.000+0000"
                },
                {
                    "permaId": {"id": "CMT:2"},
                    "message": "Extract this",
                    "draft": false,
                    "deleted": false,
                    "defectRaised": true,
                    "user": {"userName": "bob"},
                    "createDate": 1_700_000_000_000_u64,
                    "reviewItemId": {"id": "CFR-1"},
                    "lineRanges": [{"range": "12-14"}],
                    "replies": {
                        "comments": [{
                            "permaId": {"id": "CMT:3"},
                            "message": "Done",
                            "user": {"userName": "alice"},
                            "createDate": "2024-01-03T00:00:00.000+0000"
                        }]
                    }
                }
            ]
        });
        let items = serde_json::json!({
            "reviewItem": [
                {"permId": {"id": "CFR-1"}, "toPath": "src/lib.rs"}
            ]
        });
        let parsed = super::parse_review_comments("COMMON-99", &comments, &items);
        assert_eq!(parsed.review_id, "COMMON-99");
        assert_eq!(parsed.comments.len(), 2);
        assert_eq!(parsed.comments[0].kind, super::CommentKind::General);
        assert_eq!(parsed.comments[0].author, "alice");
        assert_eq!(parsed.comments[0].path, None);
        assert_eq!(parsed.comments[1].kind, super::CommentKind::Line);
        assert_eq!(parsed.comments[1].path.as_deref(), Some("src/lib.rs"));
        assert_eq!(parsed.comments[1].line.as_deref(), Some("12-14"));
        assert!(parsed.comments[1].defect);
        assert_eq!(parsed.comments[1].replies[0].message, "Done");
        assert_eq!(parsed.comments[1].created.as_deref(), Some("1700000000000"));
    }

    #[test]
    fn reads_string_perma_id_from_comment_payload() {
        let comments = serde_json::json!({
            "comments": [
                {
                    "permaId": "CMT:2",
                    "message": "Extract this helper",
                    "user": {"userName": "bob"},
                    "reviewItemId": {"id": "CFR-1"},
                    "lineRanges": [{"range": "12-14"}]
                }
            ]
        });
        let items = serde_json::json!({
            "reviewItem": [
                {"permId": {"id": "CFR-1"}, "toPath": "src/lib.rs"}
            ]
        });
        let parsed = super::parse_review_comments("COMMON-99", &comments, &items);
        assert_eq!(parsed.comments[0].id, "CMT:2");
        assert_eq!(parsed.comments[0].kind, super::CommentKind::Line);
    }

    #[test]
    fn detects_illegal_null_to_resolved_transition() {
        let error = super::CrucibleError::HttpResponse {
            status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            detail: "{\"errorMessages\":[\"Performing illegal comment resolution transition from null to RESOLVED\"]}"
                .to_owned(),
        };
        assert!(super::is_null_to_resolved(&error));
    }

    #[test]
    fn selects_ajax_resolution_endpoints_by_comment_kind() {
        let general = serde_json::json!({
            "permaId": {"id": "CMT:1"},
            "message": "Looks good"
        });
        let line = serde_json::json!({
            "permaId": {"id": "CMT:2"},
            "reviewItemId": {"id": "CFR-309468"},
            "toLineRange": "12-14"
        });
        let file = serde_json::json!({
            "permaId": {"id": "CMT:3"},
            "reviewItemId": {"id": "CFR-10"}
        });
        let reply = serde_json::json!({
            "permaId": {"id": "CMT:4"},
            "message": "Done"
        });
        assert_eq!(
            super::resolution_post(None, &general, "CMT:1", super::ResolutionStatus::Resolved)
                .endpoint,
            "generalCommentResolutionAjax"
        );
        let inline = super::resolution_post(None, &line, "2", super::ResolutionStatus::Unresolved);
        assert_eq!(inline.endpoint, "revisionCommentResolutionAjax");
        assert!(
            inline
                .fields
                .contains(&("type".to_owned(), "inline".to_owned()))
        );
        assert!(
            inline
                .fields
                .contains(&("frxId".to_owned(), "309468".to_owned()))
        );
        assert_eq!(
            super::resolution_post(None, &file, "CMT:3", super::ResolutionStatus::Resolved)
                .fields
                .iter()
                .find(|(key, _)| key == "type")
                .map(|(_, value)| value.as_str()),
            Some("revision")
        );
        let reply_post = super::resolution_post(
            Some(&line),
            &reply,
            "CMT:4",
            super::ResolutionStatus::Resolved,
        );
        assert_eq!(reply_post.endpoint, "replyCommentResolutionAjax");
        assert!(
            reply_post
                .fields
                .contains(&("replyToId".to_owned(), "2".to_owned()))
        );
    }

    #[test]
    fn finds_nested_replies_and_normalizes_comment_ids() {
        let comments = serde_json::json!({
            "comments": [{
                "permaId": {"id": "CMT:2"},
                "message": "Extract this",
                "replies": {
                    "comments": [{
                        "permaId": {"id": "CMT:4"},
                        "message": "Done"
                    }]
                }
            }]
        });
        let (parent, reply) = super::find_comment(&comments, "4").expect("find reply");
        assert_eq!(
            parent.and_then(|comment| comment
                .pointer("/permaId/id")
                .and_then(serde_json::Value::as_str)),
            Some("CMT:2")
        );
        assert_eq!(
            reply
                .pointer("/permaId/id")
                .and_then(serde_json::Value::as_str),
            Some("CMT:4")
        );
        assert_eq!(super::rest_comment_id("4"), "CMT:4");
        assert_eq!(super::rest_comment_id("CMT-4"), "CMT:4");
        assert!(super::find_comment(&comments, "missing").is_err());
        assert_eq!(
            super::comment_resource_path("COMMON-99", parent, "CMT:4"),
            "rest-service/reviews-v1/COMMON-99/comments/CMT:2/replies/CMT:4"
        );
        assert_eq!(
            super::comment_resource_path("COMMON-99", None, "CMT:2"),
            "rest-service/reviews-v1/COMMON-99/comments/CMT:2"
        );
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

    #[test]
    fn maps_line_comments_onto_patches_and_marks_the_newest() {
        let groups = serde_json::json!({
            "patchGroup": [
                {
                    "sourceName": "PATCH:1",
                    "patches": [{
                        "id": 1,
                        "fileName": "old.txt",
                        "uploadDate": 10
                    }]
                },
                {
                    "sourceName": "PATCH:5",
                    "patches": [{
                        "id": 5,
                        "fileName": "new.txt",
                        "uploadDate": 20
                    }]
                }
            ]
        });
        let comments = serde_json::json!({
            "comments": [
                {
                    "message": "old note",
                    "deleted": false,
                    "reviewItemId": {"id": "CFR-1"}
                },
                {
                    "message": "gone",
                    "deleted": true,
                    "reviewItemId": {"id": "CFR-1"}
                },
                {"message": "general"}
            ]
        });
        let items = serde_json::json!({
            "reviewItem": [
                {
                    "permId": {"id": "CFR-1"},
                    "patchUrl": "/cru/LP-1/downloadpatch/1/old.txt"
                },
                {
                    "permId": {"id": "CFR-5"},
                    "repositoryName": "PATCH:5"
                }
            ]
        });
        let patches = super::inventory_from_payloads(&groups, &comments, &items);
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].id, "1");
        assert_eq!(patches[0].comments, 1);
        assert!(!patches[0].latest);
        assert_eq!(patches[1].id, "5");
        assert_eq!(patches[1].comments, 0);
        assert!(patches[1].latest);
        assert_eq!(super::patch_id_key("PATCH:37473"), "37473");
        assert_eq!(super::patch_id_key("37473"), "37473");
    }
}

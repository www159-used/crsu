//! Executes the declarative scenarios in `e2e/comments/*.yaml`.

use super::common::{
    CrucibleEnv, Expectation, assert_no_token_leak, assert_scenario, init_repository, load_yaml,
    run_crsu,
};
use crsu_testkit::{
    CommentsDefectExpectation, CommentsDeleteExpectation, CommentsEditExpectation, CommentsFixture,
    CommentsReplyExpectation, CommentsResolutionExpectation, MockCrucible,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use tempfile::TempDir;

pub fn run(path: &Path) {
    run_scenario(&load_yaml(path, "comments scenario"));
}

fn run_scenario(scenario: &Scenario) {
    let repository = scenario
        .repository
        .as_ref()
        .map(|repository| init_repository("main", &repository.files, &repository.message));
    let server = MockCrucible::start_comments(&scenario.crucible.fixture());
    let env = CrucibleEnv {
        url: server.base_url(),
        project: &scenario.crucible.project,
        token: &scenario.crucible.token,
        reviewers: &[],
        repository: None,
    };
    let output = run_crsu(
        repository.as_ref().map(TempDir::path),
        &scenario.command,
        Some(&env),
    );
    if scenario.expect.success {
        server.assert_review_request();
    }

    assert_scenario(&scenario.name, &output, &scenario.expect);
    assert_no_token_leak(&scenario.name, &output, &scenario.crucible.token);
}

#[derive(Deserialize)]
struct Scenario {
    name: String,
    #[serde(default)]
    repository: Option<Repository>,
    command: Vec<String>,
    crucible: Crucible,
    expect: Expectation,
}

#[derive(Deserialize)]
struct Crucible {
    project: String,
    token: String,
    review_id: String,
    comments: serde_json::Value,
    review_items: serde_json::Value,
    reply: Option<CommentsReplyExpectation>,
    resolution: Option<CommentsResolutionExpectation>,
    delete: Option<CommentsDeleteExpectation>,
    edit: Option<CommentsEditExpectation>,
    defect: Option<CommentsDefectExpectation>,
}

impl Crucible {
    fn fixture(&self) -> CommentsFixture {
        CommentsFixture {
            token: self.token.clone(),
            review_id: self.review_id.clone(),
            comments: self.comments.clone(),
            review_items: self.review_items.clone(),
            reply: self.reply.clone(),
            resolution: self.resolution.clone(),
            delete: self.delete.clone(),
            edit: self.edit.clone(),
            defect: self.defect.clone(),
        }
    }
}

#[derive(Deserialize)]
struct Repository {
    files: BTreeMap<String, String>,
    message: String,
}

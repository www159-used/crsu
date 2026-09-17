//! Executes the declarative scenarios in `e2e/status/*.yaml`.

use super::common::{
    CrucibleEnv, Expectation, assert_no_token_leak, assert_scenario, assert_stdout_json,
    init_repository, load_yaml, run_crsu, text,
};
use crsu_testkit::{LandFixture, LandReviewer, MockCrucible};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use tempfile::TempDir;

pub fn run(path: &Path) {
    run_scenario(&load_yaml(path, "status scenario"));
}

fn run_scenario(scenario: &Scenario) {
    let repository = scenario
        .repository
        .as_ref()
        .map(|repository| init_repository("main", &repository.files, &repository.message));
    let fixtures = scenario
        .crucible
        .reviews
        .iter()
        .map(|review| LandFixture {
            token: scenario.crucible.token.clone(),
            review_id: review.review_id.clone(),
            title: review.title.clone(),
            state: review.state.clone(),
            objectives: review.objectives.clone(),
            reviewers: review.reviewers.clone(),
        })
        .collect::<Vec<_>>();
    let server = MockCrucible::start_reviews(&fixtures);
    let env = CrucibleEnv {
        url: server.base_url(),
        project: scenario.crucible.project.as_str(),
        token: scenario.crucible.token.as_str(),
        reviewers: &[],
        repository: None,
    };
    let output = run_crsu(
        repository.as_ref().map(TempDir::path),
        &scenario.command,
        Some(&env),
    );
    assert_scenario(&scenario.name, &output, &scenario.expect);
    let expected = scenario
        .expect
        .stdout_json
        .as_ref()
        .expect("status scenario must lock stdout_json");
    assert_stdout_json(
        &scenario.name,
        &text(&output.stdout),
        expected,
        Some((server.base_url().as_str(), "http://crucible")),
    );
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
    #[serde(default = "default_project")]
    project: String,
    #[serde(default = "default_token")]
    token: String,
    reviews: Vec<ReviewFixture>,
}

fn default_project() -> String {
    "COMMON".to_owned()
}

fn default_token() -> String {
    "test-token".to_owned()
}

#[derive(Deserialize)]
struct ReviewFixture {
    review_id: String,
    title: String,
    state: String,
    #[serde(default)]
    objectives: String,
    #[serde(default)]
    reviewers: Vec<LandReviewer>,
}

#[derive(Deserialize)]
struct Repository {
    #[serde(default)]
    files: BTreeMap<String, String>,
    message: String,
}

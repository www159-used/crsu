//! Executes the declarative scenarios in `e2e/copy/*.yaml`.

use super::common::{
    CrucibleEnv, Expectation, assert_scenario, create_origin, init_repository, load_yaml, run_crsu,
    run_git,
};
use crsu_testkit::{LandFixture, MockCrucible};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use tempfile::TempDir;

pub fn run(path: &Path) {
    run_scenario(&load_yaml(path, "copy scenario"));
}

fn run_scenario(scenario: &Scenario) {
    let repository = ScenarioRepository::create(&scenario.repository);
    let server = scenario.crucible.as_ref().map(|crucible| {
        let fixtures = crucible
            .reviews
            .iter()
            .map(|review| LandFixture {
                token: crucible.token.clone(),
                review_id: review.review_id.clone(),
                title: review.title.clone(),
                state: review.state.clone(),
                objectives: review.objectives.clone(),
                reviewers: review.reviewers.clone(),
            })
            .collect::<Vec<_>>();
        MockCrucible::start_reviews(&fixtures)
    });
    let env = server.as_ref().map(|server| {
        let crucible = scenario.crucible.as_ref().expect("server implies crucible");
        CrucibleEnv {
            url: server.base_url(),
            project: crucible.project.as_str(),
            token: crucible.token.as_str(),
            reviewers: &[],
            repository: None,
        }
    });
    let output = run_crsu(
        Some(&repository.working_directory),
        &scenario.command,
        env.as_ref(),
    );
    assert_scenario(&scenario.name, &output, &scenario.expect);
}

struct ScenarioRepository {
    _repository: TempDir,
    _origin: Option<TempDir>,
    working_directory: std::path::PathBuf,
}

impl ScenarioRepository {
    fn create(specification: &Repository) -> Self {
        let repository = init_repository(
            "main",
            &BTreeMap::from([("README.md".into(), "base\n".into())]),
            "base",
        );
        let origin = specification
            .track_origin
            .then(|| create_origin(repository.path(), "main", "main"));
        for remote_branch in &specification.remotes {
            run_git(repository.path(), ["branch", remote_branch, "main"]);
            run_git(repository.path(), ["push", "origin", remote_branch]);
        }
        for review in &specification.reviews {
            run_git(repository.path(), ["checkout", "-B", &review.branch]);
            let message = format!(
                "{title}\n\nSummary:\n\nReviewers:\n\nReviewed By:\n\nUrl: {url}\n",
                title = review.title,
                url = review.url,
            );
            run_git(
                repository.path(),
                ["commit", "--allow-empty", "-m", &message],
            );
            if let Some(upstream) = review.upstream.as_deref() {
                run_git(
                    repository.path(),
                    ["branch", "--set-upstream-to", upstream, &review.branch],
                );
            }
        }
        run_git(repository.path(), ["checkout", "main"]);
        let working_directory = repository.path().to_path_buf();
        Self {
            _repository: repository,
            _origin: origin,
            working_directory,
        }
    }
}

#[derive(Deserialize)]
struct Scenario {
    name: String,
    repository: Repository,
    command: Vec<String>,
    #[serde(default)]
    crucible: Option<Crucible>,
    expect: Expectation,
}

#[derive(Deserialize)]
struct Repository {
    reviews: Vec<ReviewCommit>,
    #[serde(default)]
    track_origin: bool,
    /// Extra branches pushed to `origin` so a review can set `upstream: origin/<name>`.
    #[serde(default)]
    remotes: Vec<String>,
}

#[derive(Deserialize)]
struct ReviewCommit {
    branch: String,
    title: String,
    #[serde(default)]
    upstream: Option<String>,
    url: String,
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
    #[serde(default = "default_state")]
    state: String,
    #[serde(default)]
    objectives: String,
    #[serde(default)]
    reviewers: Vec<crsu_testkit::LandReviewer>,
}

fn default_state() -> String {
    "Review".to_owned()
}

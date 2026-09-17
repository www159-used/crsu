//! Executes the declarative scenarios in `e2e/copy/*.yaml`.

use super::common::{Expectation, assert_scenario, init_repository, load_yaml, run_crsu, run_git};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use tempfile::TempDir;

pub fn run(path: &Path) {
    run_scenario(&load_yaml(path, "copy scenario"));
}

fn run_scenario(scenario: &Scenario) {
    let repository = ScenarioRepository::create(&scenario.repository);
    let output = run_crsu(Some(&repository.working_directory), &scenario.command, None);
    assert_scenario(&scenario.name, &output, &scenario.expect);
}

struct ScenarioRepository {
    _repository: TempDir,
    working_directory: std::path::PathBuf,
}

impl ScenarioRepository {
    fn create(specification: &Repository) -> Self {
        let repository = init_repository(
            "main",
            &BTreeMap::from([("README.md".into(), "base\n".into())]),
            "base",
        );
        for review in &specification.reviews {
            run_git(repository.path(), ["checkout", "-B", &review.branch]);
            let target = review
                .target
                .as_deref()
                .map(|target| format!("[ target: {target} ]\n"))
                .unwrap_or_default();
            let message = format!(
                "{title}\n\n[ branch: {branch} ]\n{target}[ last_tag: None ]\n\nUrl: {url}\n",
                title = review.title,
                branch = review.branch,
                url = review.url,
            );
            run_git(
                repository.path(),
                ["commit", "--allow-empty", "-m", &message],
            );
        }
        run_git(repository.path(), ["checkout", "main"]);
        let working_directory = repository.path().to_path_buf();
        Self {
            _repository: repository,
            working_directory,
        }
    }
}

#[derive(Deserialize)]
struct Scenario {
    name: String,
    repository: Repository,
    command: Vec<String>,
    expect: Expectation,
}

#[derive(Deserialize)]
struct Repository {
    reviews: Vec<ReviewCommit>,
}

#[derive(Deserialize)]
struct ReviewCommit {
    branch: String,
    title: String,
    target: Option<String>,
    url: String,
}

//! Executes the declarative scenarios in `e2e/land/*.yaml`.

use super::common::{
    CrucibleEnv, Expectation, assert_contains_all, assert_hooks, assert_no_token_leak,
    assert_scenario, create_origin, init_repository, install_crsu_hooks, install_global_hooks,
    load_yaml, run_crsu_with_xdg, run_git, write_files,
};
use crsu_testkit::{LandFixture, LandReviewer, MockCrucible};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub fn run(path: &Path) {
    run_scenario(&load_yaml(path, "land scenario"));
}

fn run_scenario(scenario: &Scenario) {
    let repository = ScenarioRepository::create(&scenario.repository);
    install_crsu_hooks(&repository.working_directory, &scenario.hooks);
    let xdg = TempDir::new().expect("isolate XDG_CONFIG_HOME");
    install_global_hooks(xdg.path(), &scenario.global_hooks);
    let server = MockCrucible::start_land(&LandFixture {
        token: scenario.crucible.token.clone(),
        review_id: scenario.crucible.review_id.clone(),
        title: scenario.crucible.title.clone(),
        state: scenario.crucible.state.clone(),
        objectives: scenario.crucible.objectives.clone(),
        reviewers: scenario.crucible.reviewers.clone(),
    });
    let env = CrucibleEnv {
        url: server.base_url(),
        project: &scenario.crucible.project,
        token: &scenario.crucible.token,
        reviewers: &[],
        repository: None,
    };
    let output = run_crsu_with_xdg(
        Some(&repository.working_directory),
        &scenario.command,
        Some(&env),
        xdg.path(),
    );
    if scenario.expect.success {
        server.assert_review_request();
    }

    assert_scenario(&scenario.name, &output, &scenario.expect);
    let body = repository.git_output(["show", "-s", "--format=%b", "HEAD"]);
    assert_contains_all(&scenario.name, &body, &scenario.expect.head_body_contains);
    assert_hooks(
        &scenario.name,
        &repository.working_directory,
        &scenario.expect,
        Some((server.base_url().as_str(), "http://crucible")),
    );
    assert_no_token_leak(&scenario.name, &output, &scenario.crucible.token);
}

struct ScenarioRepository {
    _repository: TempDir,
    _origin: TempDir,
    working_directory: PathBuf,
}

impl ScenarioRepository {
    fn create(specification: &Repository) -> Self {
        let repository = init_repository(
            &specification.base_branch,
            &specification.base_files,
            "base",
        );
        let origin = create_origin(
            repository.path(),
            &specification.base_branch,
            &specification.origin.default_branch,
        );
        // Publish the feature branch tip at base so upstream exists, then commit locally.
        run_git(
            repository.path(),
            [
                "push",
                "origin",
                &format!(
                    "{}:{}",
                    specification.base_branch, specification.feature_branch
                ),
            ],
        );
        run_git(
            repository.path(),
            ["checkout", "-b", &specification.feature_branch],
        );
        run_git(
            repository.path(),
            [
                "branch",
                "--set-upstream-to",
                &format!("origin/{}", specification.feature_branch),
            ],
        );
        write_files(repository.path(), &specification.feature_files);
        run_git(repository.path(), ["add", "."]);
        run_git(
            repository.path(),
            ["commit", "-m", specification.feature_message.as_str()],
        );

        Self {
            working_directory: repository.path().to_path_buf(),
            _repository: repository,
            _origin: origin,
        }
    }

    fn git_output<const N: usize>(&self, arguments: [&str; N]) -> String {
        super::common::git_output(&self.working_directory, arguments)
    }
}

#[derive(Deserialize)]
struct Scenario {
    name: String,
    repository: Repository,
    command: Vec<String>,
    crucible: Crucible,
    #[serde(default)]
    hooks: BTreeMap<String, String>,
    #[serde(default)]
    global_hooks: BTreeMap<String, String>,
    expect: Expectation,
}

#[derive(Deserialize)]
struct Crucible {
    project: String,
    token: String,
    review_id: String,
    title: String,
    state: String,
    #[serde(default)]
    objectives: String,
    reviewers: Vec<LandReviewer>,
}

#[derive(Deserialize)]
struct Repository {
    base_branch: String,
    base_files: BTreeMap<String, String>,
    feature_branch: String,
    feature_files: BTreeMap<String, String>,
    feature_message: String,
    origin: Origin,
}

#[derive(Deserialize)]
struct Origin {
    default_branch: String,
}

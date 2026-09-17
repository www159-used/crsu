//! Executes the declarative scenarios in `e2e/land/*.yaml`.

use super::common::{assert_contains_all, crsu_binary, load_yaml, run_git, text, write_files};
use crsu_testkit::{LandFixture, LandReviewer, MockCrucible};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

pub fn run(path: &Path) {
    run_scenario(&load_yaml(path, "land scenario"));
}

fn run_scenario(scenario: &Scenario) {
    let repository = ScenarioRepository::create(&scenario.repository);
    let server = MockCrucible::start_land(&LandFixture {
        token: scenario.crucible.token.clone(),
        review_id: scenario.crucible.review_id.clone(),
        title: scenario.crucible.title.clone(),
        state: scenario.crucible.state.clone(),
        objectives: scenario.crucible.objectives.clone(),
        reviewers: scenario
            .crucible
            .reviewers
            .iter()
            .map(|reviewer| LandReviewer {
                username: reviewer.username.clone(),
                completed: reviewer.completed,
            })
            .collect(),
    });
    let output = repository.run_crsu(&scenario.command, &server, &scenario.crucible);
    if scenario.expect.success {
        server.assert_review_request();
    }

    assert_eq!(
        output.status.success(),
        scenario.expect.success,
        "scenario failed: {}\nstdout: {}\nstderr: {}",
        scenario.name,
        text(&output.stdout),
        text(&output.stderr),
    );
    assert_contains_all(
        &scenario.name,
        &text(&output.stdout),
        &scenario.expect.stdout_contains,
    );
    assert_contains_all(
        &scenario.name,
        &text(&output.stderr),
        &scenario.expect.stderr_contains,
    );
    let body = repository.git_output(["show", "-s", "--format=%b", "HEAD"]);
    assert_contains_all(&scenario.name, &body, &scenario.expect.head_body_contains);
    let combined = format!("{}{}", text(&output.stdout), text(&output.stderr));
    assert!(
        !combined.contains(&scenario.crucible.token),
        "scenario '{}' leaked the Crucible token",
        scenario.name
    );
}

struct ScenarioRepository {
    _repository: TempDir,
    _origin: TempDir,
    working_directory: std::path::PathBuf,
}

impl ScenarioRepository {
    fn create(specification: &Repository) -> Self {
        let repository = TempDir::new().expect("create temporary Git repository");
        run_git(
            repository.path(),
            ["init", "-b", &specification.base_branch],
        );
        run_git(repository.path(), ["config", "user.name", "Test User"]);
        run_git(
            repository.path(),
            ["config", "user.email", "test@example.com"],
        );
        write_files(repository.path(), &specification.base_files);
        run_git(repository.path(), ["add", "."]);
        run_git(repository.path(), ["commit", "-m", "base"]);

        let origin = TempDir::new().expect("create origin repository");
        run_git(origin.path(), ["init", "--bare"]);
        run_git(
            repository.path(),
            [
                "remote",
                "add",
                "origin",
                origin.path().to_str().expect("origin path is UTF-8"),
            ],
        );
        run_git(
            repository.path(),
            ["push", "-u", "origin", &specification.base_branch],
        );
        run_git(
            origin.path(),
            [
                "symbolic-ref",
                "HEAD",
                &format!("refs/heads/{}", specification.origin.default_branch),
            ],
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

    fn run_crsu(&self, arguments: &[String], server: &MockCrucible, crucible: &Crucible) -> Output {
        Command::new(crsu_binary())
            .args(arguments)
            .current_dir(&self.working_directory)
            .env("CRSU_CRUCIBLE_URL", server.base_url())
            .env("CRSU_CRUCIBLE_PROJECT", &crucible.project)
            .env("CRSU_CRUCIBLE_TOKEN", &crucible.token)
            .output()
            .expect("run crsu")
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
    reviewers: Vec<Reviewer>,
}

#[derive(Deserialize)]
struct Reviewer {
    username: String,
    completed: bool,
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

#[derive(Deserialize)]
struct Expectation {
    success: bool,
    #[serde(default)]
    stdout_contains: Vec<String>,
    #[serde(default)]
    stderr_contains: Vec<String>,
    #[serde(default)]
    head_body_contains: Vec<String>,
}

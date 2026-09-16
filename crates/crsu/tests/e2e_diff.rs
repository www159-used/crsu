//! Executes the declarative scenarios in `e2e/diff/*.yaml`.

use crsu_testkit::{MockCrucible, ReviewFixture, ReviewResponse};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

#[test]
fn diff_scenarios_are_executable() {
    for scenario_path in scenario_paths() {
        let scenario: Scenario = serde_yaml::from_str(
            &std::fs::read_to_string(&scenario_path).expect("read scenario file"),
        )
        .expect("parse scenario file");
        run_scenario(&scenario);
    }
}

fn scenario_paths() -> Vec<std::path::PathBuf> {
    let mut paths = std::fs::read_dir("tests/e2e/diff")
        .expect("read scenario directory")
        .map(|entry| entry.expect("read scenario entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "yaml")
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn run_scenario(scenario: &Scenario) {
    let repository = ScenarioRepository::create(&scenario.repository);
    let output = run_with_optional_crucible(&repository, scenario);

    assert_eq!(
        output.status.success(),
        scenario.expect.success,
        "scenario failed: {}\nstdout: {}\nstderr: {}",
        scenario.name,
        text(&output.stdout),
        text(&output.stderr),
    );
    assert_contains_all(
        &text(&output.stdout),
        &scenario.expect.stdout_contains,
        scenario,
    );
    assert_contains_all(
        &text(&output.stderr),
        &scenario.expect.stderr_contains,
        scenario,
    );
    if let Some(crucible) = &scenario.crucible {
        let output = format!("{}{}", text(&output.stdout), text(&output.stderr));
        assert!(
            !output.contains(&crucible.token),
            "scenario '{}' leaked the Crucible token in command output",
            scenario.name
        );
    }
}

fn run_with_optional_crucible(repository: &ScenarioRepository, scenario: &Scenario) -> Output {
    let Some(crucible) = &scenario.crucible else {
        return repository.run_crsu(&scenario.command, None);
    };

    let server = MockCrucible::start_review(ReviewFixture {
        token: crucible.token.clone(),
        project: crucible.project.clone(),
        repository: crucible.repository.clone(),
        response: crucible.response.as_ref().map_or_else(
            || {
                let review_id = crucible.review_id.clone().expect("successful review id");
                match &crucible.current_title {
                    None => ReviewResponse::Created { review_id },
                    Some(current_title) => ReviewResponse::Updated {
                        review_id,
                        current_title: current_title.clone(),
                        new_title: crucible.new_title.clone().expect("updated review title"),
                    },
                }
            },
            |response| ReviewResponse::Rejected {
                status: response.status,
                body: response.body.clone(),
            },
        ),
    });

    let output = repository.run_crsu(&scenario.command, Some((&server, crucible)));
    server.assert_review_request();
    output
}

fn assert_contains_all(actual: &str, expected: &[String], scenario: &Scenario) {
    for expected in expected {
        assert!(
            actual.contains(expected),
            "scenario '{}' expected output to contain '{expected}', actual output: {actual}",
            scenario.name
        );
    }
}

struct ScenarioRepository {
    _repository: TempDir,
    _worktree: Option<TempDir>,
    _origin: Option<TempDir>,
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

        let origin = specification.origin.as_ref().map(|origin| {
            let remote = TempDir::new().expect("create origin repository");
            run_git(remote.path(), ["init", "--bare"]);
            run_git(
                repository.path(),
                [
                    "remote",
                    "add",
                    "origin",
                    remote.path().to_str().expect("origin path is UTF-8"),
                ],
            );
            run_git(
                repository.path(),
                ["push", "-u", "origin", &specification.base_branch],
            );
            run_git(
                remote.path(),
                [
                    "symbolic-ref",
                    "HEAD",
                    &format!("refs/heads/{}", origin.default_branch),
                ],
            );
            run_git(
                repository.path(),
                ["remote", "set-head", "origin", "--auto"],
            );
            remote
        });

        let (worktree, working_directory) = if specification.linked_worktree {
            let worktree = TempDir::new().expect("create linked worktree directory");
            run_git(
                repository.path(),
                [
                    "worktree",
                    "add",
                    "-b",
                    &specification.feature_branch,
                    worktree.path().to_str().expect("worktree path is UTF-8"),
                ],
            );
            let path = worktree.path().to_path_buf();
            (Some(worktree), path)
        } else {
            run_git(
                repository.path(),
                ["checkout", "-b", &specification.feature_branch],
            );
            (None, repository.path().to_path_buf())
        };
        write_files(&working_directory, &specification.feature_files);
        run_git(&working_directory, ["add", "."]);
        let feature_message = specification
            .feature_message
            .as_deref()
            .unwrap_or("feature change");
        run_git(&working_directory, ["commit", "-m", feature_message]);
        write_files(&working_directory, &specification.uncommitted_files);

        Self {
            _repository: repository,
            _worktree: worktree,
            _origin: origin,
            working_directory,
        }
    }

    fn run_crsu(
        &self,
        arguments: &[String],
        crucible: Option<(&MockCrucible, &Crucible)>,
    ) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_crsu"));
        command.args(arguments).current_dir(&self.working_directory);
        if let Some((server, crucible)) = crucible {
            command
                .env("CRSU_CRUCIBLE_URL", server.base_url())
                .env("CRSU_CRUCIBLE_PROJECT", &crucible.project)
                .env("CRSU_CRUCIBLE_TOKEN", &crucible.token)
                .env("CRSU_CRUCIBLE_REPOSITORY", &crucible.repository);
        }
        command.output().expect("run crsu")
    }
}

fn write_files(repository: &Path, files: &BTreeMap<String, String>) {
    for (path, contents) in files {
        let path = repository.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create file parent");
        }
        std::fs::write(path, contents).expect("write repository file");
    }
}

fn run_git<const N: usize>(repository: &Path, arguments: [&str; N]) {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git failed: {}",
        text(&output.stderr)
    );
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_owned()).expect("command output is UTF-8")
}

#[derive(Deserialize)]
struct Scenario {
    name: String,
    repository: Repository,
    command: Vec<String>,
    crucible: Option<Crucible>,
    expect: Expectation,
}

#[derive(Deserialize)]
struct Crucible {
    project: String,
    repository: String,
    token: String,
    review_id: Option<String>,
    current_title: Option<String>,
    new_title: Option<String>,
    response: Option<CrucibleResponse>,
}

#[derive(Deserialize)]
struct CrucibleResponse {
    status: u16,
    body: String,
}

#[derive(Deserialize)]
struct Repository {
    base_branch: String,
    base_files: BTreeMap<String, String>,
    feature_branch: String,
    feature_files: BTreeMap<String, String>,
    feature_message: Option<String>,
    #[serde(default)]
    linked_worktree: bool,
    origin: Option<Origin>,
    #[serde(default)]
    uncommitted_files: BTreeMap<String, String>,
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
}

//! Executes the declarative scenarios in `e2e/diff/*.yaml`.

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
    let output = repository.run_crsu(&scenario.command);

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
    repository: TempDir,
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

        run_git(
            repository.path(),
            ["checkout", "-b", &specification.feature_branch],
        );
        write_files(repository.path(), &specification.feature_files);
        run_git(repository.path(), ["add", "."]);
        run_git(repository.path(), ["commit", "-m", "feature change"]);
        write_files(repository.path(), &specification.uncommitted_files);

        Self { repository }
    }

    fn run_crsu(&self, arguments: &[String]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_crsu"))
            .args(arguments)
            .current_dir(self.repository.path())
            .output()
            .expect("run crsu")
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
    expect: Expectation,
}

#[derive(Deserialize)]
struct Repository {
    base_branch: String,
    base_files: BTreeMap<String, String>,
    feature_branch: String,
    feature_files: BTreeMap<String, String>,
    #[serde(default)]
    uncommitted_files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Expectation {
    success: bool,
    #[serde(default)]
    stdout_contains: Vec<String>,
    #[serde(default)]
    stderr_contains: Vec<String>,
}

use crsu::init_test_support::OverflowScreen;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use tempfile::TempDir;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Screen {
    Project,
    Repository,
    ReviewerCandidates,
    SelectedReviewers,
}

#[derive(Debug, Deserialize)]
pub struct Terminal {
    pub width: u16,
    pub height: u16,
}

pub fn load_yaml<T: DeserializeOwned>(path: &Path, what: &str) -> T {
    serde_yaml::from_str(&std::fs::read_to_string(path).unwrap_or_else(|error| {
        panic!("read {what}: {error}");
    }))
    .unwrap_or_else(|error| panic!("parse {what}: {error}"))
}

pub fn overflow_screen(screen: Screen) -> OverflowScreen {
    match screen {
        Screen::Project => OverflowScreen::Project,
        Screen::Repository => OverflowScreen::Repository,
        Screen::ReviewerCandidates => OverflowScreen::ReviewerCandidates,
        Screen::SelectedReviewers => OverflowScreen::SelectedReviewers,
    }
}

pub fn crsu_binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .expect("workspace root");
            let status = Command::new("cargo")
                .args(["build", "--quiet", "-p", "crsu", "--bin", "crsu"])
                .current_dir(workspace)
                .status()
                .expect("build crsu binary for E2E tests");
            assert!(status.success(), "building crsu binary failed");
            let profile = std::env::current_exe()
                .expect("current E2E executable")
                .parent()
                .and_then(Path::parent)
                .expect("Cargo profile directory")
                .to_path_buf();
            profile.join(format!("crsu{}", std::env::consts::EXE_SUFFIX))
        })
        .as_path()
}

pub fn write_files(repository: &Path, files: &BTreeMap<String, String>) {
    for (path, contents) in files {
        let path = repository.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create file parent");
        }
        std::fs::write(path, contents).expect("write repository file");
    }
}

pub fn run_git<const N: usize>(repository: &Path, arguments: [&str; N]) {
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

pub fn git_output<const N: usize>(directory: &Path, arguments: [&str; N]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git failed: {}",
        text(&output.stderr)
    );
    text(&output.stdout).trim().to_owned()
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_owned()).expect("command output is UTF-8")
}

/// Creates a repository with one commit on `branch` holding `files`.
pub fn init_repository(branch: &str, files: &BTreeMap<String, String>, message: &str) -> TempDir {
    let repository = TempDir::new().expect("create temporary Git repository");
    run_git(repository.path(), ["init", "-b", branch]);
    run_git(repository.path(), ["config", "user.name", "Test User"]);
    run_git(
        repository.path(),
        ["config", "user.email", "test@example.com"],
    );
    write_files(repository.path(), files);
    run_git(repository.path(), ["add", "."]);
    run_git(repository.path(), ["commit", "-m", message]);
    repository
}

/// Creates a bare origin holding `base_branch`, with `default_branch` as its HEAD.
pub fn create_origin(repository: &Path, base_branch: &str, default_branch: &str) -> TempDir {
    let origin = TempDir::new().expect("create origin repository");
    run_git(origin.path(), ["init", "--bare"]);
    run_git(
        repository,
        [
            "remote",
            "add",
            "origin",
            origin.path().to_str().expect("origin path is UTF-8"),
        ],
    );
    run_git(repository, ["push", "-u", "origin", base_branch]);
    run_git(
        origin.path(),
        [
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{default_branch}"),
        ],
    );
    origin
}

/// The Crucible environment one scenario runs `crsu` against.
pub struct CrucibleEnv<'a> {
    pub url: String,
    pub project: &'a str,
    pub token: &'a str,
    pub reviewers: &'a [String],
    pub repository: Option<&'a str>,
}

const CRUCIBLE_ENV: [&str; 5] = [
    "CRSU_CRUCIBLE_URL",
    "CRSU_CRUCIBLE_PROJECT",
    "CRSU_CRUCIBLE_TOKEN",
    "CRSU_CRUCIBLE_REPOSITORY",
    "CRSU_CRUCIBLE_REVIEWERS",
];

/// Runs the built `crsu` binary in `directory`, with only the environment the
/// scenario asked for; everything else is cleared so the developer's shell cannot
/// leak into the scenario.
pub fn run_crsu(
    directory: Option<&Path>,
    arguments: &[String],
    crucible: Option<&CrucibleEnv<'_>>,
) -> Output {
    let mut command = Command::new(crsu_binary());
    command.args(arguments);
    command.env("CRSU_NO_CLIPBOARD", "1");
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    match crucible {
        Some(crucible) => {
            command
                .env("CRSU_CRUCIBLE_URL", &crucible.url)
                .env("CRSU_CRUCIBLE_PROJECT", crucible.project)
                .env("CRSU_CRUCIBLE_TOKEN", crucible.token)
                .env("CRSU_CRUCIBLE_REVIEWERS", crucible.reviewers.join(","));
            match crucible.repository {
                Some(repository) => {
                    command.env("CRSU_CRUCIBLE_REPOSITORY", repository);
                }
                None => {
                    command.env_remove("CRSU_CRUCIBLE_REPOSITORY");
                }
            }
        }
        None => {
            for name in CRUCIBLE_ENV {
                command.env_remove(name);
            }
        }
    }
    command.output().expect("run crsu")
}

/// The pass/fail contract every scenario shares.
#[derive(Deserialize)]
pub struct Expectation {
    pub success: bool,
    #[serde(default)]
    pub stdout_contains: Vec<String>,
    #[serde(default)]
    pub stderr_contains: Vec<String>,
    pub head_subject: Option<String>,
    #[serde(default)]
    pub head_body_contains: Vec<String>,
    pub review: Option<ExpectedReview>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct ExpectedReview {
    pub title: String,
    pub objectives: String,
    pub state: String,
}

pub fn assert_scenario(name: &str, output: &Output, expect: &Expectation) {
    assert_eq!(
        output.status.success(),
        expect.success,
        "scenario failed: {name}\nstdout: {}\nstderr: {}",
        text(&output.stdout),
        text(&output.stderr),
    );
    assert_contains_all(name, &text(&output.stdout), &expect.stdout_contains);
    assert_contains_all(name, &text(&output.stderr), &expect.stderr_contains);
}

/// Asserts the Crucible token never reached command output.
pub fn assert_no_token_leak(name: &str, output: &Output, token: &str) {
    let combined = format!("{}{}", text(&output.stdout), text(&output.stderr));
    assert!(
        !combined.contains(token),
        "scenario '{name}' leaked the Crucible token"
    );
}

pub fn assert_contains_all(name: &str, actual: &str, expected: &[String]) {
    for expected in expected {
        assert!(
            actual.contains(expected),
            "scenario '{name}' expected output to contain '{expected}', actual output: {actual}"
        );
    }
}

pub fn assert_visible(name: &str, frame: &str, contains: &[String], not_contains: &[String]) {
    for expected in contains {
        assert!(
            frame.contains(expected),
            "{name} should show {expected:?}\n{frame}"
        );
    }
    for unexpected in not_contains {
        assert!(
            !frame.contains(unexpected),
            "{name} should hide {unexpected:?}\n{frame}"
        );
    }
}

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

/// Writes executable hook scripts under `.git/crsu/hooks/`.
pub fn install_crsu_hooks(repository: &Path, hooks: &BTreeMap<String, String>) {
    install_hooks(&repository.join(".git/crsu/hooks"), hooks);
}

/// Writes executable hook scripts under `{config_home}/hooks/`.
pub fn install_global_hooks(config_home: &Path, hooks: &BTreeMap<String, String>) {
    install_hooks(&config_home.join("hooks"), hooks);
}

fn install_hooks(directory: &Path, hooks: &BTreeMap<String, String>) {
    if hooks.is_empty() {
        return;
    }
    std::fs::create_dir_all(directory).expect("create crsu hooks directory");
    for (name, body) in hooks {
        assert!(
            name.chars()
                .all(|character| character.is_ascii_lowercase() || character == '-'),
            "invalid hook name {name}"
        );
        let path = directory.join(name);
        std::fs::write(&path, body).expect("write hook script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("make hook executable");
        }
    }
}

pub fn captured_hook_json(repository: &Path) -> String {
    std::fs::read_to_string(repository.join(".git/crsu/hooks/captured.json"))
        .expect("read captured hook json")
}

pub fn captured_hook_order(repository: &Path) -> Vec<String> {
    std::fs::read_to_string(repository.join(".git/crsu/hooks/order.log"))
        .expect("read hook order log")
        .lines()
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
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
    let isolated = TempDir::new().expect("isolate CRSU_CONFIG_HOME");
    run_crsu_with_config_home(directory, arguments, crucible, isolated.path())
}

/// Like [`run_crsu`], but uses `config_home` as the user-level crsu directory.
pub fn run_crsu_with_config_home(
    directory: Option<&Path>,
    arguments: &[String],
    crucible: Option<&CrucibleEnv<'_>>,
    config_home: &Path,
) -> Output {
    let mut command = Command::new(crsu_binary());
    command.args(arguments);
    command.env("CRSU_NO_CLIPBOARD", "1");
    command.env("CRSU_CONFIG_HOME", config_home);
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
    /// Parsed JSON contract for commands whose product is stable JSON.
    #[serde(default)]
    pub stdout_json: Option<serde_json::Value>,
    #[serde(default)]
    pub stderr_contains: Vec<String>,
    pub head_subject: Option<String>,
    #[serde(default)]
    pub head_body_contains: Vec<String>,
    #[serde(default)]
    pub head_body_not_contains: Vec<String>,
    pub review: Option<ExpectedReview>,
    /// JSON written by a hook script to `.git/crsu/hooks/captured.json`.
    #[serde(default)]
    pub hook_json: Option<serde_json::Value>,
    /// Lines written by hook scripts to `.git/crsu/hooks/order.log`.
    #[serde(default)]
    pub hook_order: Vec<String>,
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

/// Locks stdout as JSON. `rewrite_origin` maps the live mock base URL to a stable prefix.
pub fn assert_stdout_json(
    name: &str,
    stdout: &str,
    expected: &serde_json::Value,
    rewrite_origin: Option<(&str, &str)>,
) {
    let mut actual: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("scenario '{name}' stdout is not JSON: {error}\n{stdout}"));
    if let Some((from, to)) = rewrite_origin {
        rewrite_string_prefix(&mut actual, from, to);
    }
    assert_eq!(
        actual,
        *expected,
        "scenario '{name}' JSON stdout mismatch\nactual: {}\nexpected: {}",
        serde_json::to_string_pretty(&actual).expect("actual json"),
        serde_json::to_string_pretty(expected).expect("expected json"),
    );
}

fn rewrite_string_prefix(value: &mut serde_json::Value, from: &str, to: &str) {
    match value {
        serde_json::Value::String(text) => {
            if let Some(rest) = text.strip_prefix(from) {
                *text = format!("{to}{rest}");
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                rewrite_string_prefix(item, from, to);
            }
        }
        serde_json::Value::Object(fields) => {
            for item in fields.values_mut() {
                rewrite_string_prefix(item, from, to);
            }
        }
        _ => {}
    }
}

/// Asserts the Crucible token never reached command output.
pub fn assert_no_token_leak(name: &str, output: &Output, token: &str) {
    let combined = format!("{}{}", text(&output.stdout), text(&output.stderr));
    assert!(
        !combined.contains(token),
        "scenario '{name}' leaked the Crucible token"
    );
}

pub fn assert_hooks(
    name: &str,
    repository: &Path,
    expect: &Expectation,
    rewrite_origin: Option<(&str, &str)>,
) {
    if let Some(expected) = &expect.hook_json {
        assert_stdout_json(
            name,
            &captured_hook_json(repository),
            expected,
            rewrite_origin,
        );
    }
    if !expect.hook_order.is_empty() {
        assert_eq!(
            captured_hook_order(repository),
            expect.hook_order,
            "scenario '{name}' hook order"
        );
    }
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

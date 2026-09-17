use crsu::init_test_support::OverflowScreen;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

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

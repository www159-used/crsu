//! `crsu diff` 的人类可读端到端场景。

use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

#[test]
fn given_a_clean_feature_branch_when_diffing_against_main_then_it_reports_a_reviewable_patch() {
    let feature_branch = FeatureBranch::with_one_commit();

    let result = feature_branch.run_crsu(["diff", "main"]);

    assert!(result.status.success());
    let stdout = text(&result.stdout);
    assert!(stdout.contains("Base: main"));
    assert!(stdout.contains("Commits: 1"));
    assert!(stdout.contains("Patch bytes: "));
}

#[test]
fn given_an_uncommitted_file_when_diffing_then_it_refuses_to_create_an_unreproducible_patch() {
    let feature_branch = FeatureBranch::with_one_commit();
    std::fs::write(feature_branch.path().join("uncommitted.txt"), "dirty\n")
        .expect("write uncommitted file");

    let result = feature_branch.run_crsu(["diff", "main"]);

    assert!(!result.status.success());
    assert!(text(&result.stderr).contains("working tree has uncommitted changes"));
}

struct FeatureBranch {
    repository: TempDir,
}

impl FeatureBranch {
    fn with_one_commit() -> Self {
        let repository = TempDir::new().expect("create temporary Git repository");
        run_git(repository.path(), ["init", "-b", "main"]);
        run_git(repository.path(), ["config", "user.name", "Test User"]);
        run_git(
            repository.path(),
            ["config", "user.email", "test@example.com"],
        );

        std::fs::write(repository.path().join("README.md"), "base\n").expect("write base file");
        run_git(repository.path(), ["add", "README.md"]);
        run_git(repository.path(), ["commit", "-m", "base"]);

        run_git(repository.path(), ["checkout", "-b", "feature"]);
        std::fs::write(repository.path().join("README.md"), "feature\n")
            .expect("write feature file");
        run_git(repository.path(), ["commit", "-am", "feature change"]);

        Self { repository }
    }

    fn path(&self) -> &Path {
        self.repository.path()
    }

    fn run_crsu<const N: usize>(&self, arguments: [&str; N]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_crsu"))
            .args(arguments)
            .current_dir(self.path())
            .output()
            .expect("run crsu")
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

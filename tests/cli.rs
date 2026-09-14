use std::process::Command;
use tempfile::TempDir;

fn crsu() -> Command {
    Command::new(env!("CARGO_BIN_EXE_crsu"))
}

#[test]
fn help_lists_the_initial_workflow_commands() {
    let output = crsu().arg("--help").output().expect("run crsu");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(stdout.contains("doctor"));
    assert!(stdout.contains("diff"));
    assert!(stdout.contains("land"));
}

#[test]
fn doctor_reports_the_current_git_repository() {
    let output = crsu().arg("doctor").output().expect("run crsu doctor");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 doctor output");
    assert!(stdout.contains("Git repository:"));
}

#[test]
fn diff_summarizes_commits_and_patch_against_the_given_base() {
    let repository = repository_with_feature_commit();
    let output = crsu()
        .args(["diff", "main"])
        .current_dir(repository.path())
        .output()
        .expect("run crsu diff");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(stdout.contains("Base: main"));
    assert!(stdout.contains("Commits: 1"));
    assert!(stdout.contains("Patch bytes: "));
}

#[test]
fn diff_rejects_a_dirty_worktree() {
    let repository = repository_with_feature_commit();
    std::fs::write(repository.path().join("uncommitted.txt"), "dirty\n")
        .expect("write uncommitted file");

    let output = crsu()
        .args(["diff", "main"])
        .current_dir(repository.path())
        .output()
        .expect("run crsu diff");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 error");
    assert!(stderr.contains("working tree has uncommitted changes"));
}

#[test]
fn land_accepts_an_optional_target_branch() {
    let output = crsu()
        .args(["land", "master"])
        .output()
        .expect("run crsu land");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 error");
    assert!(stderr.contains("not implemented"));
}

fn repository_with_feature_commit() -> TempDir {
    let repository = TempDir::new().expect("temporary repository");
    git(repository.path(), ["init", "-b", "main"]);
    git(repository.path(), ["config", "user.name", "Test User"]);
    git(
        repository.path(),
        ["config", "user.email", "test@example.com"],
    );
    std::fs::write(repository.path().join("README.md"), "base\n").expect("write base file");
    git(repository.path(), ["add", "README.md"]);
    git(repository.path(), ["commit", "-m", "base"]);
    git(repository.path(), ["checkout", "-b", "feature"]);
    std::fs::write(repository.path().join("README.md"), "feature\n").expect("write feature file");
    git(repository.path(), ["commit", "-am", "feature change"]);
    repository
}

fn git<const N: usize>(repository: &std::path::Path, arguments: [&str; N]) {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .status()
        .expect("run git");
    assert!(status.success());
}

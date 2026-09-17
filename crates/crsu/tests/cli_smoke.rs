//! 命令接口的轻量冒烟测试；Git 工作流场景见 `e2e_diff.rs`。

use std::process::Command;

fn crsu() -> Command {
    Command::new(env!("CARGO_BIN_EXE_crsu"))
}

#[test]
fn help_lists_the_initial_workflow_commands() {
    let output = crsu().arg("--help").output().expect("run crsu");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(stdout.contains("doctor"));
    assert!(stdout.contains("config"));
    assert!(stdout.contains("diff"));
    assert!(stdout.contains("copy"));
    assert!(stdout.contains("completions"));
    assert!(stdout.contains("land"));
    assert!(stdout.contains("comments"));
    assert!(stdout.contains("patches"));
}

#[test]
fn comments_help_lists_reply_and_resolution_commands() {
    let output = crsu()
        .args(["comments", "--help"])
        .output()
        .expect("run crsu comments --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(stdout.contains("list"));
    assert!(stdout.contains("reply"));
    assert!(stdout.contains("resolve"));
    assert!(stdout.contains("unresolve"));
    assert!(stdout.contains("delete"));
    assert!(stdout.contains("edit"));
    assert!(stdout.contains("defect"));
    assert!(stdout.contains("undefect"));
}

#[test]
fn patches_help_lists_list_delete_and_prune() {
    let output = crsu()
        .args(["patches", "--help"])
        .output()
        .expect("run crsu patches --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(stdout.contains("list"));
    assert!(stdout.contains("delete"));
    assert!(stdout.contains("prune"));
}

#[test]
fn config_show_redacts_token_and_unset_repository_disables_anchoring() {
    let repository = tempfile::tempdir().expect("temporary repository");
    git(repository.path(), &["init", "-q"]);
    let config_dir = repository.path().join(".git/crsu");
    std::fs::create_dir_all(&config_dir).expect("create config directory");
    let config_path = config_dir.join("config.toml");
    std::fs::write(
        &config_path,
        r#"schema_version = 2

[crucible]
url = "http://cru"
token = "top-secret"
project = "LP"
repository = "logriver"
repository_location = "ssh://git/log_parser2.git"
reviewers = ["alice"]
"#,
    )
    .expect("write config");

    let show = crsu()
        .current_dir(repository.path())
        .args(["config", "show"])
        .output()
        .expect("show config");
    assert!(show.status.success());
    let stdout = String::from_utf8(show.stdout).expect("UTF-8 config");
    assert!(stdout.contains("repository = logriver"));
    assert!(stdout.contains("token = <redacted>"));
    assert!(!stdout.contains("top-secret"));

    let unset = crsu()
        .current_dir(repository.path())
        .args(["config", "unset", "repository"])
        .output()
        .expect("unset repository");
    assert!(unset.status.success());
    let saved = std::fs::read_to_string(config_path).expect("read updated config");
    assert!(!saved.contains("repository ="));
    assert!(!saved.contains("repository_location ="));
}

#[test]
fn diff_attach_preserves_subject_and_records_the_review_url() {
    let repository = tempfile::tempdir().expect("temporary repository");
    git(repository.path(), &["init", "-q"]);
    git(repository.path(), &["config", "user.name", "Test User"]);
    git(
        repository.path(),
        &["config", "user.email", "test@example.com"],
    );
    std::fs::write(repository.path().join("README.md"), "content\n").expect("write file");
    git(repository.path(), &["add", "README.md"]);
    git(
        repository.path(),
        &["commit", "-q", "-m", "fix: keep this subject"],
    );
    let config_dir = repository.path().join(".git/crsu");
    std::fs::create_dir_all(&config_dir).expect("create config directory");
    std::fs::write(
        config_dir.join("config.toml"),
        r#"schema_version = 2

[crucible]
url = "http://crucible"
token = "secret"
project = "LP"
reviewers = ["alice", "bob"]
"#,
    )
    .expect("write config");

    let output = crsu()
        .current_dir(repository.path())
        .args(["diff", "--attach", "LP-1475"])
        .output()
        .expect("attach review");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let subject = git_text(repository.path(), &["show", "-s", "--format=%s", "HEAD"]);
    let body = git_text(repository.path(), &["show", "-s", "--format=%b", "HEAD"]);
    assert_eq!(subject, "fix: keep this subject");
    assert!(body.contains("Summary:"));
    assert!(body.contains("Reviewers: alice, bob"));
    assert!(body.contains("Reviewed By:"));
    assert!(body.contains("Url: http://crucible/cru/LP-1475"));
}

fn git(repository: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(repository)
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success());
}

fn git_text(repository: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repository)
        .args(args)
        .output()
        .expect("run git");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("UTF-8 git output")
        .trim()
        .to_owned()
}

#[test]
fn doctor_reports_the_current_git_repository() {
    let output = crsu().arg("doctor").output().expect("run crsu doctor");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 doctor output");
    assert!(stdout.contains("Git repository:"));
}

#[test]
fn land_rejects_cross_branch_targets() {
    let repository = tempfile::tempdir().expect("temporary repository");
    git(repository.path(), &["init", "-b", "main", "-q"]);
    git(repository.path(), &["config", "user.name", "Test"]);
    git(
        repository.path(),
        &["config", "user.email", "t@example.com"],
    );
    std::fs::write(repository.path().join("README"), "x\n").expect("write");
    git(repository.path(), &["add", "."]);
    git(repository.path(), &["commit", "-m", "base", "-q"]);
    let output = crsu()
        .args(["land", "master"])
        .current_dir(repository.path())
        .output()
        .expect("run crsu land");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 error");
    assert!(stderr.contains("cross-branch land"));
}

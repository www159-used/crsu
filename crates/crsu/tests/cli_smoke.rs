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
    assert!(stdout.contains("land"));
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

fn git(repository: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(repository)
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success());
}

#[test]
fn doctor_reports_the_current_git_repository() {
    let output = crsu().arg("doctor").output().expect("run crsu doctor");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 doctor output");
    assert!(stdout.contains("Git repository:"));
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

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
fn land_accepts_an_optional_target_branch() {
    let output = crsu()
        .args(["land", "master"])
        .output()
        .expect("run crsu land");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 error");
    assert!(stderr.contains("not implemented"));
}

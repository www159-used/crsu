use clap::{Parser, Subcommand};
use std::process::{Command as ProcessCommand, ExitCode};

mod crucible;
mod review_diff;

#[derive(Debug, Parser)]
#[command(about = "Focused Git and Crucible review workflow")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 检查本地 Git 与评审工具环境。
    Doctor,
    /// 基于可选的基线分支创建或更新评审。
    Diff {
        /// 作为比较基线的 Git ref；默认使用当前 upstream。
        base: Option<String>,
    },
    /// 将当前分支合入可选的目标分支。
    Land {
        /// 本地目标分支；默认使用当前分支。
        target: Option<String>,
    },
}

/// 执行请求的 `crsu` 工作流。
#[must_use]
pub fn run(cli: Cli) -> ExitCode {
    match cli.command {
        Command::Doctor => doctor(),
        Command::Diff { base } => diff(base.as_deref()),
        Command::Land { target } => not_implemented("land", target.as_deref()),
    }
}

fn diff(base: Option<&str>) -> ExitCode {
    match review_diff::from_current_repository(base) {
        Ok(review_diff) => {
            println!("Base: {}", review_diff.base());
            println!("Commits: {}", review_diff.commit_count());
            println!("Patch bytes: {}", review_diff.patch_len());
            match crucible::submit_if_configured(&review_diff) {
                Ok(Some(review_id)) => println!("Review: {review_id}"),
                Ok(None) => {}
                Err(error) => {
                    eprintln!("diff failed: {error}");
                    return ExitCode::FAILURE;
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("diff failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn doctor() -> ExitCode {
    let output = match ProcessCommand::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            eprintln!("Git unavailable: {error}");
            return ExitCode::FAILURE;
        }
    };

    if !output.status.success() {
        eprintln!("Not a Git repository");
        return ExitCode::FAILURE;
    }

    let repository = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    println!("Git repository: {repository}");
    ExitCode::SUCCESS
}

fn not_implemented(command: &str, target: Option<&str>) -> ExitCode {
    let _ = target;
    eprintln!("{command} is not implemented");
    ExitCode::FAILURE
}

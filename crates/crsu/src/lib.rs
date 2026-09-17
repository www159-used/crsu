use clap::{Parser, Subcommand, ValueEnum};
use std::process::ExitCode;

mod crucible;
mod git_repository;
mod init_model;
mod init_tui;
mod init_workflow;
mod legacy_cru;
mod project_config;

/// Narrow test-only interface for exercising init behavior from a separate crate.
/// This feature is intentionally opt-in and is not part of the default CLI surface.
#[cfg(feature = "test-support")]
pub mod init_test_support {
    pub use crate::init_model::{FormFlow, InputMode, Screen, Step};
    pub use crate::init_tui::{
        OverflowScreen, render_exit_confirmation, render_login_dialog, render_overflow_screen,
        render_required_field_error, render_search_results,
    };

    #[derive(Debug)]
    pub struct Candidates {
        pub token: String,
        pub projects: Vec<String>,
        pub repositories: Vec<Repository>,
        pub reviewers: Vec<Reviewer>,
    }

    #[derive(Debug)]
    pub struct Repository {
        pub name: String,
        pub scm_type: String,
        pub location: String,
    }

    impl Candidates {
        #[must_use]
        pub fn detected_repository(&self, remote_url: &str) -> Option<&str> {
            let repositories = self
                .repositories
                .iter()
                .map(|repository| crate::crucible::RepositoryCandidate {
                    name: repository.name.clone(),
                    scm_type: repository.scm_type.clone(),
                    location: repository.location.clone(),
                })
                .collect::<Vec<_>>();
            crate::init_workflow::detected_repository(remote_url, &repositories)
                .and_then(|recommended| {
                    self.repositories
                        .iter()
                        .find(|repository| repository.name == recommended.name)
                })
                .map(|repository| repository.name.as_str())
        }
    }

    #[derive(Debug)]
    pub struct Reviewer {
        pub username: String,
        pub display_name: String,
    }

    /// Signs in and loads all candidates required by the init form.
    ///
    /// # Errors
    ///
    /// Returns an error when Crucible rejects the credentials or cannot provide candidates.
    pub fn load_candidates(
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<Candidates, String> {
        let candidates = crate::init_workflow::load_candidates(url, username, password)?;
        Ok(Candidates {
            token: candidates.token,
            projects: candidates.projects,
            repositories: candidates
                .repositories
                .into_iter()
                .map(|repository| Repository {
                    name: repository.name,
                    scm_type: repository.scm_type,
                    location: repository.location,
                })
                .collect(),
            reviewers: candidates
                .reviewers
                .into_iter()
                .map(|user| Reviewer {
                    username: user.username,
                    display_name: user.display_name,
                })
                .collect(),
        })
    }
}

#[derive(Debug, Parser)]
#[command(about = "Focused Git and Crucible review workflow")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 以交互式向导配置当前仓库。
    Init,
    /// 查看或精确修改当前仓库的配置。
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// 检查本地 Git 与评审工具环境。
    Doctor,
    /// 基于可选的基线分支创建或更新评审。
    Diff {
        /// 作为比较基线的 Git ref；默认使用当前 upstream。
        base: Option<String>,
        /// 将当前 HEAD 关联到已经存在的 Crucible review。
        #[arg(long, value_name = "REVIEW_ID", conflicts_with = "base")]
        attach: Option<String>,
    },
    /// 将当前分支合入可选的目标分支。
    Land {
        /// 本地目标分支；默认使用当前分支。
        target: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// 显示当前配置（认证 token 始终脱敏）。
    Show,
    /// 设置单个配置项。
    Set {
        #[arg(value_enum)]
        key: ConfigKey,
        value: String,
    },
    /// 清除单个可选配置项。
    Unset {
        #[arg(value_enum)]
        key: UnsetConfigKey,
    },
    /// 管理默认 reviewers。
    Reviewer {
        #[command(subcommand)]
        command: ReviewerCommand,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ConfigKey {
    Url,
    Project,
    Repository,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum UnsetConfigKey {
    Repository,
}

#[derive(Debug, Subcommand)]
enum ReviewerCommand {
    /// 列出默认 reviewers。
    List,
    /// 添加默认 reviewer（重复添加无副作用）。
    Add { username: String },
    /// 删除默认 reviewer（不存在时无副作用）。
    Remove { username: String },
}

/// 执行请求的 `crsu` 工作流。
#[must_use]
pub fn run(cli: Cli) -> ExitCode {
    match cli.command {
        Command::Init => init_tui::run(),
        Command::Config { command } => config(command),
        Command::Doctor => doctor(),
        Command::Diff { base, attach } => diff(base.as_deref(), attach.as_deref()),
        Command::Land { target } => not_implemented("land", target.as_deref()),
    }
}

fn config(command: ConfigCommand) -> ExitCode {
    let mut config = match project_config::ProjectConfig::load() {
        Ok(Some(config)) => config,
        Ok(None) => {
            eprintln!("config failed: project is not initialized; run `crsu init`");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("config failed: {error}");
            return ExitCode::FAILURE;
        }
    };

    let changed = match command {
        ConfigCommand::Show => {
            println!("url = {}", config.crucible.url);
            println!("token = <redacted>");
            println!("project = {}", config.crucible.project);
            println!(
                "repository = {}",
                config
                    .crucible
                    .repository
                    .as_deref()
                    .unwrap_or("(no anchor)")
            );
            println!("reviewers = {}", config.crucible.reviewers.join(", "));
            false
        }
        ConfigCommand::Set { key, value } => {
            if value.trim().is_empty() {
                eprintln!("config failed: value is required");
                return ExitCode::FAILURE;
            }
            match key {
                ConfigKey::Url => config.set_url(value),
                ConfigKey::Project => config.set_project(value),
                ConfigKey::Repository => config.set_repository(value),
            }
            true
        }
        ConfigCommand::Unset {
            key: UnsetConfigKey::Repository,
        } => {
            config.unset_repository();
            true
        }
        ConfigCommand::Reviewer { command } => match command {
            ReviewerCommand::List => {
                for reviewer in &config.crucible.reviewers {
                    println!("{reviewer}");
                }
                false
            }
            ReviewerCommand::Add { username } => {
                config.add_reviewer(username);
                true
            }
            ReviewerCommand::Remove { username } => {
                config.remove_reviewer(&username);
                true
            }
        },
    };

    if changed {
        match config.save() {
            Ok(path) => println!("Saved: {}", path.display()),
            Err(error) => {
                eprintln!("config failed: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

fn diff(base: Option<&str>, attach: Option<&str>) -> ExitCode {
    let repository = match git_repository::Repository::discover() {
        Ok(repository) => repository,
        Err(error) => {
            eprintln!("diff failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(review_id) = attach {
        return attach_review(&repository, review_id);
    }
    match repository.review_diff(base) {
        Ok(review_diff) => {
            println!("Base: {}", review_diff.base());
            println!("Commits: {}", review_diff.commit_count());
            println!("Patch bytes: {}", review_diff.patch_len());
            match crucible::submit_if_configured(&review_diff) {
                Ok(Some(submission)) => {
                    println!("Review: {}", submission.review_id());
                    if let Some((previous, current)) = submission.title_update() {
                        println!("Title updated: {previous} -> {current}");
                    }
                    if submission.was_created()
                        && let Err(error) = repository
                            .attach_review(submission.review_url(), submission.reviewers())
                    {
                        eprintln!(
                            "diff failed: review {} was created, but Git association failed: {error}",
                            submission.review_id()
                        );
                        return ExitCode::FAILURE;
                    }
                }
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

fn attach_review(repository: &git_repository::Repository, review_id: &str) -> ExitCode {
    if review_id.is_empty()
        || !review_id.contains('-')
        || !review_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        eprintln!("diff failed: invalid Crucible review id {review_id:?}");
        return ExitCode::FAILURE;
    }
    let config = match project_config::ProjectConfig::load() {
        Ok(Some(config)) => config,
        Ok(None) => {
            eprintln!("diff failed: project is not initialized; run `crsu init`");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("diff failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let review_url = format!(
        "{}/cru/{review_id}",
        config.crucible.url.trim_end_matches('/')
    );
    match repository.attach_review(&review_url, &config.crucible.reviewers) {
        Ok(()) => {
            println!("Attached: {review_id}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("diff failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn doctor() -> ExitCode {
    let repository = match git_repository::Repository::discover() {
        Ok(repository) => repository,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    println!("Git repository: {}", repository.work_tree().display());
    ExitCode::SUCCESS
}

fn not_implemented(command: &str, target: Option<&str>) -> ExitCode {
    let _ = target;
    eprintln!("{command} is not implemented");
    ExitCode::FAILURE
}

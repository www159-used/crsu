use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use std::process::ExitCode;

mod clipboard;
mod crucible;
mod crucible_conf;
mod git_repository;
mod init_model;
mod init_tui;
mod init_workflow;
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
#[command(name = "crsu", about = "Focused Git and Crucible review workflow")]
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
        /// 跳过提交 patch 前的确认提示。
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },
    /// 将当前评审摘要复制到剪贴板：`[base] title url`。
    Copy {
        /// 作为合入目标显示的 Git ref；默认使用当前 upstream。
        base: Option<String>,
    },
    /// 生成 shell 补全脚本。
    Completions {
        /// 目标 shell。
        shell: clap_complete::Shell,
    },
    /// 将当前分支合入可选的目标分支。
    Land {
        /// 本地目标分支；默认使用当前分支（首版仅支持同分支 push）。
        target: Option<String>,
        /// 跳过 push 前的确认提示。
        #[arg(short = 'y', long = "yes")]
        yes: bool,
        /// 允许推到与 review 记录目标不一致的分支。
        #[arg(long)]
        force: bool,
    },
    /// 读取或修改评审评论。
    Comments {
        #[command(subcommand)]
        command: Option<CommentsCommand>,
    },
    /// 列出或删除评审上的过往 patch。
    Patches {
        #[command(subcommand)]
        command: Option<PatchesCommand>,
    },
}

#[derive(Debug, Subcommand)]
enum CommentsCommand {
    /// 以稳定 JSON 输出评审评论。
    List {
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        review_id: Option<String>,
    },
    /// 回复一条评论。
    Reply {
        /// 评论 id，例如 `CMT:39844`。
        comment_id: String,
        /// 回复正文。
        #[arg(short, long)]
        message: String,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(long, value_name = "REVIEW_ID")]
        review: Option<String>,
    },
    /// 将评论标为已解决。
    #[command(visible_alias = "mark-resolved")]
    Resolve {
        /// 评论 id；可重复。与 `--all` 一起时忽略。
        comment_ids: Vec<String>,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(long, value_name = "REVIEW_ID")]
        review: Option<String>,
        /// 该 review 下全部顶层评论。
        #[arg(long)]
        all: bool,
    },
    /// 删除一条自己的评论或回复。
    #[command(visible_alias = "rm")]
    Delete {
        /// 评论 id，例如 `CMT:39844`。
        comment_id: String,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(long, value_name = "REVIEW_ID")]
        review: Option<String>,
    },
    /// 改写一条自己的评论或回复。
    #[command(visible_alias = "update")]
    Edit {
        /// 评论 id，例如 `CMT:39844`。
        comment_id: String,
        /// 新的评论正文。
        #[arg(short, long)]
        message: String,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(long, value_name = "REVIEW_ID")]
        review: Option<String>,
    },
    /// 将评论标为缺陷。
    #[command(visible_alias = "raise-defect")]
    Defect {
        /// 评论 id；可重复。与 `--all` 一起时忽略。
        comment_ids: Vec<String>,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(long, value_name = "REVIEW_ID")]
        review: Option<String>,
        /// 该 review 下全部顶层评论。
        #[arg(long)]
        all: bool,
    },
    /// 取消评论上的缺陷标记。
    #[command(visible_alias = "clear-defect")]
    Undefect {
        /// 评论 id；可重复。与 `--all` 一起时忽略。
        comment_ids: Vec<String>,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(long, value_name = "REVIEW_ID")]
        review: Option<String>,
        /// 该 review 下全部顶层评论。
        #[arg(long)]
        all: bool,
    },
    /// 将评论标为待解决（Needs resolution）。
    #[command(visible_alias = "needs-resolve")]
    Unresolve {
        /// 评论 id；可重复。与 `--all` 一起时忽略。
        comment_ids: Vec<String>,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(long, value_name = "REVIEW_ID")]
        review: Option<String>,
        /// 该 review 下全部顶层评论。
        #[arg(long)]
        all: bool,
    },
}

#[derive(Debug, Subcommand)]
enum PatchesCommand {
    /// 以稳定 JSON 输出评审上的 patch。
    List {
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        review_id: Option<String>,
    },
    /// 删除指定 patch；挂着未删行内评论的会跳过。
    Delete {
        /// patch id，例如 `37473` 或 `PATCH:37473`；可重复。
        patch_ids: Vec<String>,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(long, value_name = "REVIEW_ID")]
        review: Option<String>,
    },
    /// 只留最新一块，其余能删的删掉。
    Prune {
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(long, value_name = "REVIEW_ID")]
        review: Option<String>,
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
        Command::Diff { base, attach, yes } => diff(base.as_deref(), attach.as_deref(), yes),
        Command::Copy { base } => copy(base.as_deref()),
        Command::Completions { shell } => completions(shell),
        Command::Land { target, yes, force } => land(target.as_deref(), yes, force),
        Command::Comments { command } => comments_command(command),
        Command::Patches { command } => patches_command(command),
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

fn diff(base: Option<&str>, attach: Option<&str>, yes: bool) -> ExitCode {
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
            match crucible::submit_confirmation(&review_diff) {
                Ok(None) => {}
                Ok(Some(prompt)) => {
                    if !yes && !confirm_yes(&prompt) {
                        println!("Aborted");
                        return ExitCode::SUCCESS;
                    }
                    match crucible::submit_if_configured(&review_diff) {
                        Ok(Some(submission)) => {
                            println!("Review: {}", submission.review_id());
                            if let Some((previous, current)) = submission.title_update() {
                                println!("Title updated: {previous} -> {current}");
                            }
                            if submission.objectives_were_updated() {
                                println!("Objectives updated");
                            }
                            let clipboard = clipboard::summary(
                                review_diff.base(),
                                review_diff.title(),
                                submission.review_url(),
                            );
                            match clipboard::copy(&clipboard) {
                                Ok(()) => println!("Clipboard: {clipboard}"),
                                Err(error) => eprintln!("clipboard failed: {error}"),
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
                }
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

fn confirm_yes(prompt: &str) -> bool {
    eprint!("{prompt}");
    let _ = std::io::Write::flush(&mut std::io::stderr());
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    line.trim().eq_ignore_ascii_case("y")
}

fn copy(base: Option<&str>) -> ExitCode {
    let repository = match git_repository::Repository::discover() {
        Ok(repository) => repository,
        Err(error) => {
            eprintln!("copy failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (base, title, url) = match repository.share_summary(base) {
        Ok(parts) => parts,
        Err(error) => {
            eprintln!("copy failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let summary = clipboard::summary(&base, &title, &url);
    match clipboard::copy(&summary) {
        Ok(()) => {
            println!("Clipboard: {summary}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("clipboard failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn completions(shell: clap_complete::Shell) -> ExitCode {
    let mut command = Cli::command();
    clap_complete::generate(shell, &mut command, "crsu", &mut std::io::stdout());
    ExitCode::SUCCESS
}

fn comments_command(command: Option<CommentsCommand>) -> ExitCode {
    match command.unwrap_or(CommentsCommand::List { review_id: None }) {
        CommentsCommand::List { review_id } => {
            comments(review_id.as_deref(), crucible::review_comments)
        }
        CommentsCommand::Reply {
            comment_id,
            message,
            review,
        } => comments(review.as_deref(), |review_id| {
            crucible::reply_comment(review_id, &comment_id, &message)
        }),
        CommentsCommand::Resolve {
            comment_ids,
            review,
            all,
        } => comments(review.as_deref(), |review_id| {
            crucible::set_comment_resolutions(
                review_id,
                &comment_ids,
                crucible::ResolutionStatus::Resolved,
                all,
            )
        }),
        CommentsCommand::Unresolve {
            comment_ids,
            review,
            all,
        } => comments(review.as_deref(), |review_id| {
            crucible::set_comment_resolutions(
                review_id,
                &comment_ids,
                crucible::ResolutionStatus::Unresolved,
                all,
            )
        }),
        CommentsCommand::Delete { comment_id, review } => {
            comments(review.as_deref(), |review_id| {
                crucible::delete_comment(review_id, &comment_id)
            })
        }
        CommentsCommand::Edit {
            comment_id,
            message,
            review,
        } => comments(review.as_deref(), |review_id| {
            crucible::edit_comment(review_id, &comment_id, &message)
        }),
        CommentsCommand::Defect {
            comment_ids,
            review,
            all,
        } => comments(review.as_deref(), |review_id| {
            crucible::set_comment_defects(review_id, &comment_ids, true, all)
        }),
        CommentsCommand::Undefect {
            comment_ids,
            review,
            all,
        } => comments(review.as_deref(), |review_id| {
            crucible::set_comment_defects(review_id, &comment_ids, false, all)
        }),
    }
}

fn patches_command(command: Option<PatchesCommand>) -> ExitCode {
    match command.unwrap_or(PatchesCommand::List { review_id: None }) {
        PatchesCommand::List { review_id } => {
            review_json(review_id.as_deref(), crucible::review_patches, "patches")
        }
        PatchesCommand::Delete { patch_ids, review } => review_json(
            review.as_deref(),
            |review_id| crucible::delete_patches(review_id, &patch_ids),
            "patches",
        ),
        PatchesCommand::Prune { review } => {
            review_json(review.as_deref(), crucible::prune_patches, "patches")
        }
    }
}

/// Resolves the review id, runs one comment operation, and prints its JSON result.
fn comments<T: serde::Serialize>(
    review_id: Option<&str>,
    operation: impl FnOnce(&str) -> Result<T, crucible::CrucibleError>,
) -> ExitCode {
    review_json(review_id, operation, "comments")
}

fn review_json<T: serde::Serialize>(
    review_id: Option<&str>,
    operation: impl FnOnce(&str) -> Result<T, crucible::CrucibleError>,
    what: &str,
) -> ExitCode {
    let review_id = match review_id_or_head(review_id, what) {
        Ok(review_id) => review_id,
        Err(code) => return code,
    };
    match operation(&review_id) {
        Ok(result) => {
            let json = serde_json::to_string_pretty(&result).expect("review json serialize");
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => review_failed(what, error),
    }
}

fn review_id_or_head(review_id: Option<&str>, what: &str) -> Result<String, ExitCode> {
    if let Some(review_id) = review_id {
        return Ok(review_id.to_owned());
    }
    let repository =
        git_repository::Repository::discover().map_err(|error| review_failed(what, error))?;
    repository
        .review_id_from_head()
        .map_err(|error| review_failed(what, error))
}

fn review_failed(what: &str, error: impl std::fmt::Display) -> ExitCode {
    eprintln!("{what} failed: {error}");
    ExitCode::FAILURE
}

fn land(target: Option<&str>, yes: bool, force: bool) -> ExitCode {
    let repository = match git_repository::Repository::discover() {
        Ok(repository) => repository,
        Err(error) => return land_failed(error),
    };
    if let Err(error) = repository.ensure_clean_worktree() {
        return land_failed(error);
    }
    let current = match repository.current_branch() {
        Ok(branch) => branch,
        Err(error) => return land_failed(error),
    };
    let land_branch = target.unwrap_or(&current).to_owned();
    if land_branch != current {
        return land_failed(git_repository::Error::CrossBranchLand {
            current,
            target: land_branch,
        });
    }
    let upstream = match repository.upstream_of(&current) {
        Ok(upstream) => upstream,
        Err(error) => return land_failed(error),
    };
    let config = match crucible::configured() {
        Ok(config) => config,
        Err(error) => return land_failed(error),
    };
    let acceptance = match land_acceptance(&repository, &upstream, &config) {
        Ok(acceptance) => acceptance,
        Err(code) => return code,
    };
    let review_target = git_repository::target_from_objectives(&acceptance.objectives);
    print_land_plan(&repository, &current, &upstream, &acceptance, review_target);
    let target_error = match review_target {
        Some(target) => repository.land_target_error(target, &upstream),
        None => Some(git_repository::Error::MissingReviewTarget),
    };
    if let Some(error) = target_error {
        if force {
            eprintln!("warning: {error}; continuing because --force");
        } else {
            return land_failed(error);
        }
    }
    if let Err(error) = repository.prepare_land_commit(
        &acceptance.review_url,
        &acceptance.reviewers,
        &acceptance.reviewed_by,
    ) {
        return land_failed(error);
    }
    let remote_branch = git_repository::land_ref_key(&upstream);
    println!("Rebasing onto {upstream}");
    if let Err(error) = repository.pull_rebase_origin(remote_branch) {
        return land_failed(error);
    }
    match repository.head_oneline() {
        Ok(line) => println!("{line}"),
        Err(error) => return land_failed(error),
    }
    let prompt = format!("push branch '{current}' to '{upstream}'? [y/N] ");
    if !yes && !confirm_yes(&prompt) {
        println!("Aborted");
        return ExitCode::SUCCESS;
    }
    if let Err(error) = repository.push_to_origin(&current, remote_branch) {
        return land_failed(error);
    }
    println!("Pushed: {current} -> {upstream}");
    if let Err(error) = crucible::close_review(&config, &acceptance.review_id, &acceptance.state) {
        eprintln!("land failed: review was pushed, but closing failed: {error}");
        return ExitCode::FAILURE;
    }
    println!("Closed: {}", acceptance.review_id);
    ExitCode::SUCCESS
}

fn print_land_plan(
    repository: &git_repository::Repository,
    current: &str,
    upstream: &str,
    acceptance: &crucible::LandReview,
    review_target: Option<&str>,
) {
    println!("Review: {} ({})", acceptance.review_id, acceptance.state);
    println!("Review target: {}", review_target.unwrap_or("(none)"));
    println!("Push: {current} -> {upstream}");
    // shortstat is decorative; a failed git call should not hide the rest of the plan
    if let Ok(stat) = repository.diff_shortstat(upstream)
        && !stat.is_empty()
    {
        println!("Diff: {stat}");
    }
    println!("Reviewed By: {}", acceptance.reviewed_by.join(", "));
}

fn land_acceptance(
    repository: &git_repository::Repository,
    upstream: &str,
    config: &crucible::Config,
) -> Result<crucible::LandReview, ExitCode> {
    let commits = repository.commits_ahead_of(upstream).map_err(land_failed)?;
    match commits.as_slice() {
        [] => return Err(land_failed(git_repository::Error::NothingToLand)),
        [_] => {}
        _ => {
            return Err(land_failed(git_repository::Error::MultipleCommits {
                count: commits.len(),
            }));
        }
    }
    let first_message = repository
        .commit_message(&commits[0])
        .map_err(land_failed)?;
    let Some(review_id) = git_repository::review_id_from_message(&first_message) else {
        return Err(land_failed(git_repository::Error::NoReviewUrl));
    };
    crucible::land_review(config, &review_id).map_err(land_failed)
}

fn land_failed(error: impl std::fmt::Display) -> ExitCode {
    eprintln!("land failed: {error}");
    ExitCode::FAILURE
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
    let review_url = crucible::review_url(&config.crucible.url, review_id);
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

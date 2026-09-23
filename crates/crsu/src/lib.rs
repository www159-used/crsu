use clap::{Parser, Subcommand, ValueEnum};
use std::process::ExitCode;

mod clipboard;
mod complete;
mod crucible;
mod git_repository;
mod hooks;
mod init_model;
mod init_tui;
mod init_workflow;
mod log;
mod project_config;

/// Narrow test-only interface for exercising init behavior from a separate crate.
/// This feature is intentionally opt-in and is not part of the default CLI surface.
#[cfg(feature = "test-support")]
pub mod init_test_support {
    pub use crate::init_model::{FormFlow, InputMode, Screen, Step};
    pub use crate::init_tui::{
        OverflowScreen, RenderedInsert, TextInsertField, render_exit_confirmation,
        render_login_dialog, render_overflow_screen, render_required_field_error,
        render_search_results, render_text_insert,
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
#[command(
    name = "crsu",
    version,
    about = "Focused Git and Crucible review workflow"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 以交互式向导配置当前仓库。
    Init {
        /// 写入用户级配置目录的 `config.toml`，不写当前仓库。
        #[arg(short, long)]
        global: bool,
    },
    /// 查看或精确修改配置。
    #[command(visible_alias = "cfg")]
    Config {
        /// 读写用户级配置目录，不需要当前仓库。
        #[arg(short, long)]
        global: bool,
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// 检查本地 Git 与评审工具环境。
    #[command(visible_alias = "doc")]
    Doctor,
    /// 从 Crucible 读取评审状态，stdout 为稳定 JSON。
    #[command(visible_alias = "st")]
    Status {
        /// Crucible review id；可重复。省略时从 HEAD 的 `Url:` 读取。
        review_ids: Vec<String>,
    },
    /// 基于可选的基线分支创建或更新评审。
    #[command(visible_alias = "df")]
    Diff {
        /// 作为比较基线的 Git ref；默认使用当前 upstream。
        base: Option<String>,
        /// 将当前 HEAD 关联到已经存在的 Crucible review。
        #[arg(short, long, value_name = "REVIEW_ID", conflicts_with = "base")]
        attach: Option<String>,
        /// 忽略 HEAD 中的旧 Url，新建评审并替换关联；适用于 cherry-pick 后出评审。
        #[arg(short, long, conflicts_with = "attach")]
        new: bool,
        /// 跳过提交 patch 前的确认提示。
        #[arg(short = 'y', long = "yes")]
        yes: bool,
        /// 允许提交超过 1000 行的 patch。
        #[arg(short, long)]
        force: bool,
    },
    /// 输出评审摘要：`[base] title url`。默认复制当前 HEAD；`--jira` 按提交里的 `Url:` 聚合各分支。
    Copy {
        /// 作为合入目标显示的 Git ref；默认使用当前 upstream。
        base: Option<String>,
        /// 按 JIRA 编号收集各分支已出评审的摘要；不 checkout。合入目标来自 upstream / 分支名；仍像功能分支时才读评审 description。
        #[arg(short, long, conflicts_with = "base")]
        jira: Option<String>,
        /// 只看这些 ref；可与 `--jira` 合用。逗号分隔。
        #[arg(short, long, value_delimiter = ',', num_args = 1.., conflicts_with = "base")]
        branches: Vec<String>,
    },
    /// 打印 bash / zsh / fish 补全脚本；脚本会再调用 `crsu complete` 做动态候选。
    #[command(visible_alias = "comp")]
    Completions {
        /// 目标 shell。
        shell: complete::Shell,
        /// 写入该 shell 的常规补全目录，不要 source / eval。
        #[arg(long)]
        install: bool,
        /// 覆盖安装目录（文件名仍按 shell 约定）。
        #[arg(long, value_name = "DIR")]
        dir: Option<std::path::PathBuf>,
    },
    /// 给补全脚本提供动态候选，一行一个；失败时输出为空。
    #[command(hide = true)]
    Complete {
        kind: complete::Kind,
        #[arg(default_value = "")]
        prefix: String,
    },
    /// 将当前分支合入可选的目标分支。
    #[command(visible_alias = "ld")]
    Land {
        /// 本地目标分支；默认使用当前分支（首版仅支持同分支 push）。
        target: Option<String>,
        /// 跳过 push 前的确认提示。
        #[arg(short = 'y', long = "yes")]
        yes: bool,
        /// 允许推到与 review 记录目标不一致的分支。
        #[arg(short, long)]
        force: bool,
    },
    /// 读取或修改评审评论。
    #[command(visible_alias = "cmt")]
    Comments {
        #[command(subcommand)]
        command: Option<CommentsCommand>,
    },
    /// 列出或删除评审上的过往 patch。
    #[command(visible_alias = "pt")]
    Patches {
        #[command(subcommand)]
        command: Option<PatchesCommand>,
    },
}

#[derive(Debug, Subcommand)]
enum CommentsCommand {
    /// 以稳定 JSON 输出评审评论。
    #[command(visible_alias = "ls")]
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
        #[arg(short, long, value_name = "REVIEW_ID")]
        review: Option<String>,
    },
    /// 将评论标为已解决。
    Resolve {
        /// 评论 id；可重复。与 `--all` 一起时忽略。
        comment_ids: Vec<String>,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(short, long, value_name = "REVIEW_ID")]
        review: Option<String>,
        /// 该 review 下全部顶层评论。
        #[arg(short, long)]
        all: bool,
    },
    /// 删除一条自己的评论或回复。
    #[command(visible_alias = "rm")]
    Delete {
        /// 评论 id，例如 `CMT:39844`。
        comment_id: String,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(short, long, value_name = "REVIEW_ID")]
        review: Option<String>,
    },
    /// 改写一条自己的评论或回复。
    Edit {
        /// 评论 id，例如 `CMT:39844`。
        comment_id: String,
        /// 新的评论正文。
        #[arg(short, long)]
        message: String,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(short, long, value_name = "REVIEW_ID")]
        review: Option<String>,
    },
    /// 将评论标为缺陷。
    Defect {
        /// 评论 id；可重复。与 `--all` 一起时忽略。
        comment_ids: Vec<String>,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(short, long, value_name = "REVIEW_ID")]
        review: Option<String>,
        /// 该 review 下全部顶层评论。
        #[arg(short, long)]
        all: bool,
    },
    /// 取消评论上的缺陷标记。
    Undefect {
        /// 评论 id；可重复。与 `--all` 一起时忽略。
        comment_ids: Vec<String>,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(short, long, value_name = "REVIEW_ID")]
        review: Option<String>,
        /// 该 review 下全部顶层评论。
        #[arg(short, long)]
        all: bool,
    },
    /// 将评论标为待解决（Needs resolution）。
    Unresolve {
        /// 评论 id；可重复。与 `--all` 一起时忽略。
        comment_ids: Vec<String>,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(short, long, value_name = "REVIEW_ID")]
        review: Option<String>,
        /// 该 review 下全部顶层评论。
        #[arg(short, long)]
        all: bool,
    },
}

#[derive(Debug, Subcommand)]
enum PatchesCommand {
    /// 以稳定 JSON 输出评审上的 patch。
    #[command(visible_alias = "ls")]
    List {
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        review_id: Option<String>,
    },
    /// 删除指定 patch；挂着未删行内评论的会跳过。
    Delete {
        /// patch id，例如 `37473` 或 `PATCH:37473`；可重复。
        patch_ids: Vec<String>,
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(short, long, value_name = "REVIEW_ID")]
        review: Option<String>,
    },
    /// 只留最新一块，其余能删的删掉。
    Prune {
        /// Crucible review id；默认从 HEAD 提交的 `Url:` 读取。
        #[arg(short, long, value_name = "REVIEW_ID")]
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
        Command::Init { global } => init_tui::run(global),
        Command::Config { global, command } => config(global, command),
        Command::Doctor => doctor(),
        Command::Status { review_ids } => status(&review_ids),
        Command::Diff {
            base,
            attach,
            new,
            yes,
            force,
        } => diff(base.as_deref(), attach.as_deref(), new, yes, force),
        Command::Copy {
            base,
            jira,
            branches,
        } => copy(base.as_deref(), jira.as_deref(), &branches),
        Command::Completions {
            shell,
            install,
            dir,
        } => {
            if install {
                complete::install(shell, dir.as_deref())
            } else {
                complete::script(shell)
            }
        }
        Command::Complete { kind, prefix } => complete::candidates(kind, &prefix),
        Command::Land { target, yes, force } => land(target.as_deref(), yes, force),
        Command::Comments { command } => comments_command(command),
        Command::Patches { command } => patches_command(command),
    }
}

fn config(global: bool, command: ConfigCommand) -> ExitCode {
    if global
        && matches!(
            command,
            ConfigCommand::Set {
                key: ConfigKey::Repository,
                ..
            } | ConfigCommand::Unset {
                key: UnsetConfigKey::Repository
            }
        )
    {
        eprintln!("config failed: repository is project-only; omit --global");
        return ExitCode::FAILURE;
    }

    let mutating = matches!(
        command,
        ConfigCommand::Set { .. }
            | ConfigCommand::Unset { .. }
            | ConfigCommand::Reviewer {
                command: ReviewerCommand::Add { .. } | ReviewerCommand::Remove { .. }
            }
    );
    let mut log = mutating.then(|| crate::log::CommandLog::start("config"));
    let mut config = match load_editable_config(global, mutating) {
        Ok(config) => config,
        Err(code) => return code,
    };

    let changed = match command {
        ConfigCommand::Show => {
            println!("scope = {}", if global { "global" } else { "project" });
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
        let saved = if let Some(log) = &log {
            log.time("save", || {
                if global {
                    config.save_global()
                } else {
                    config.save()
                }
            })
        } else if global {
            config.save_global()
        } else {
            config.save()
        };
        match saved {
            Ok(path) => {
                println!("Saved: {}", path.display());
                if let Some(log) = &mut log {
                    log.finish("ok");
                }
            }
            Err(error) => {
                eprintln!("config failed: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

fn load_editable_config(
    global: bool,
    mutating: bool,
) -> Result<project_config::ProjectConfig, ExitCode> {
    let loaded = if global {
        project_config::ProjectConfig::load_global()
    } else {
        project_config::ProjectConfig::load()
    };
    match loaded {
        Ok(Some(config)) => Ok(config),
        Ok(None) if mutating && global => Ok(project_config::ProjectConfig::blank()),
        Ok(None) => {
            let hint = if global {
                "global config is missing; run `crsu init --global` or `crsu config --global set`"
            } else {
                "project is not initialized; run `crsu init`"
            };
            eprintln!("config failed: {hint}");
            Err(ExitCode::FAILURE)
        }
        Err(error) => {
            eprintln!("config failed: {error}");
            Err(ExitCode::FAILURE)
        }
    }
}

fn diff(base: Option<&str>, attach: Option<&str>, new: bool, yes: bool, force: bool) -> ExitCode {
    let mut log = crate::log::CommandLog::start("diff");
    let repository = match git_repository::Repository::discover() {
        Ok(repository) => repository,
        Err(error) => {
            eprintln!("diff failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(review_id) = attach {
        let code = attach_review(&repository, review_id);
        if code == ExitCode::SUCCESS {
            log.finish(format!("ok attach {review_id}"));
        }
        return code;
    }
    let review_diff = match log.time("git", || repository.review_diff(base)) {
        Ok(review_diff) => review_diff,
        Err(error) => {
            eprintln!("diff failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let review_diff = if new {
        review_diff.without_review()
    } else {
        review_diff
    };
    if let Err(code) = print_diff_plan(&review_diff, force) {
        return code;
    }
    if let Err(error) = log.time("pre-hooks", || {
        hooks::run_pre(
            &repository,
            "pre-diff",
            &serde_json::json!({
                "event": "pre-diff",
                "command": "diff",
                "base": review_diff.base(),
                "title": review_diff.title(),
                "review_id": review_diff.review_id(),
            }),
        )
    }) {
        eprintln!("diff failed: {error}");
        return ExitCode::FAILURE;
    }
    match crucible::submit_confirmation(&review_diff) {
        Ok(None) => {}
        Ok(Some(prompt)) => {
            if !yes && !log.time("wait", || confirm_yes(&prompt)) {
                println!("Aborted");
                log.finish("aborted");
                return ExitCode::SUCCESS;
            }
            match log.time("crucible", || crucible::submit_if_configured(&review_diff)) {
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
                    let action = if submission.was_created() {
                        "created"
                    } else {
                        "updated"
                    };
                    log.time("post-hooks", || {
                        hooks::run_post(
                            &repository,
                            "post-diff",
                            &serde_json::json!({
                                "event": "post-diff",
                                "command": "diff",
                                "action": action,
                                "review_id": submission.review_id(),
                                "url": submission.review_url(),
                            }),
                        );
                    });
                    log.finish(format!("ok {} {action}", submission.review_id()));
                }
                Ok(None) => log.finish("ok"),
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
    if !log.finished() {
        log.finish("ok");
    }
    ExitCode::SUCCESS
}

fn print_diff_plan(review_diff: &git_repository::ReviewDiff, force: bool) -> Result<(), ExitCode> {
    println!("Base: {}", review_diff.base());
    println!("Commits: {}", review_diff.commit_count());
    let stats = review_diff.stats();
    println!("Files: {}", stats.files);
    println!("Lines: {}", stats.lines);
    match git_repository::patch_line_limit(stats.lines) {
        Some(error) if force => {
            eprintln!("warning: {error}; continuing because --force");
            Ok(())
        }
        Some(error) => {
            eprintln!("diff failed: {error}");
            Err(ExitCode::FAILURE)
        }
        None => Ok(()),
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

fn status(review_ids: &[String]) -> ExitCode {
    let mut log = crate::log::CommandLog::start("status");
    let ids = if review_ids.is_empty() {
        match log.time("git", || review_id_or_head(None, "status")) {
            Ok(review_id) => vec![review_id],
            Err(code) => return code,
        }
    } else {
        review_ids.to_vec()
    };
    let config = match crucible::configured() {
        Ok(config) => config,
        Err(error) => return review_failed("status", error),
    };
    let mut reviews = Vec::new();
    for review_id in &ids {
        match log.time("crucible", || crucible::review_status(&config, review_id)) {
            Ok(review) => reviews.push(review),
            Err(error) => return review_failed("status", error),
        }
    }
    let json =
        serde_json::to_string_pretty(&StatusReport { reviews }).expect("status json serialize");
    println!("{json}");
    log.finish(format!("ok {}", ids.join(" ")));
    ExitCode::SUCCESS
}

#[derive(serde::Serialize)]
struct StatusReport {
    reviews: Vec<crucible::ReviewStatus>,
}

fn copy(base: Option<&str>, jira: Option<&str>, branches: &[String]) -> ExitCode {
    let mut log = crate::log::CommandLog::start("copy");
    let repository = match git_repository::Repository::discover() {
        Ok(repository) => repository,
        Err(error) => {
            eprintln!("copy failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    if jira.is_some() || !branches.is_empty() {
        return copy_batch(&repository, jira, branches, &mut log);
    }
    let (base, title, url) = match log.time("git", || repository.share_summary(base)) {
        Ok(parts) => parts,
        Err(error) => {
            eprintln!("copy failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let summary = clipboard::summary(&base, &title, &url);
    match log.time("clipboard", || clipboard::copy(&summary)) {
        Ok(()) => {
            println!("Clipboard: {summary}");
            log.finish("ok");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("clipboard failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn copy_batch(
    repository: &git_repository::Repository,
    jira: Option<&str>,
    branches: &[String],
    log: &mut crate::log::CommandLog,
) -> ExitCode {
    let shares = match jira {
        Some(jira) => log.time("git", || repository.review_shares(jira, branches)),
        None => log.time("git", || repository.review_shares_at_refs(branches)),
    };
    let shares = match shares {
        Ok(mut shares) => {
            overlay_copy_targets(repository, &mut shares);
            shares
        }
        Err(error) => {
            eprintln!("copy failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let summary = shares
        .iter()
        .map(|share| clipboard::summary(&share.target, &share.title, &share.url))
        .collect::<Vec<_>>()
        .join("\n");
    println!("{summary}");
    match log.time("clipboard", || clipboard::copy(&summary)) {
        Ok(()) => {
            log.finish(format!("ok {} shares", shares.len()));
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("clipboard failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn overlay_copy_targets(
    repository: &git_repository::Repository,
    shares: &mut [git_repository::ReviewShare],
) {
    let Ok(config) = crucible::configured() else {
        return;
    };
    for share in shares {
        if !git_repository::land_ref_key(&share.target).contains('/') {
            continue;
        }
        let Some(review_id) = git_repository::review_id_from_url(&share.url) else {
            continue;
        };
        let Some(target) = crucible::review_copy_target(&config, &review_id) else {
            continue;
        };
        share.target = repository.prefer_origin_remote(&target);
    }
}

fn comments_command(command: Option<CommentsCommand>) -> ExitCode {
    match command.unwrap_or(CommentsCommand::List { review_id: None }) {
        CommentsCommand::List { review_id } => {
            comments(review_id.as_deref(), crucible::review_comments, "list")
        }
        CommentsCommand::Reply {
            comment_id,
            message,
            review,
        } => comments(
            review.as_deref(),
            |review_id| crucible::reply_comment(review_id, &comment_id, &message),
            "reply",
        ),
        CommentsCommand::Resolve {
            comment_ids,
            review,
            all,
        } => comments(
            review.as_deref(),
            |review_id| {
                crucible::set_comment_resolutions(
                    review_id,
                    &comment_ids,
                    crucible::ResolutionStatus::Resolved,
                    all,
                )
            },
            "resolve",
        ),
        CommentsCommand::Unresolve {
            comment_ids,
            review,
            all,
        } => comments(
            review.as_deref(),
            |review_id| {
                crucible::set_comment_resolutions(
                    review_id,
                    &comment_ids,
                    crucible::ResolutionStatus::Unresolved,
                    all,
                )
            },
            "unresolve",
        ),
        CommentsCommand::Delete { comment_id, review } => comments(
            review.as_deref(),
            |review_id| crucible::delete_comment(review_id, &comment_id),
            "delete",
        ),
        CommentsCommand::Edit {
            comment_id,
            message,
            review,
        } => comments(
            review.as_deref(),
            |review_id| crucible::edit_comment(review_id, &comment_id, &message),
            "edit",
        ),
        CommentsCommand::Defect {
            comment_ids,
            review,
            all,
        } => comments(
            review.as_deref(),
            |review_id| crucible::set_comment_defects(review_id, &comment_ids, true, all),
            "defect",
        ),
        CommentsCommand::Undefect {
            comment_ids,
            review,
            all,
        } => comments(
            review.as_deref(),
            |review_id| crucible::set_comment_defects(review_id, &comment_ids, false, all),
            "undefect",
        ),
    }
}

fn patches_command(command: Option<PatchesCommand>) -> ExitCode {
    match command.unwrap_or(PatchesCommand::List { review_id: None }) {
        PatchesCommand::List { review_id } => review_json(
            review_id.as_deref(),
            crucible::review_patches,
            "patches",
            "list",
        ),
        PatchesCommand::Delete { patch_ids, review } => review_json(
            review.as_deref(),
            |review_id| crucible::delete_patches(review_id, &patch_ids),
            "patches",
            "delete",
        ),
        PatchesCommand::Prune { review } => review_json(
            review.as_deref(),
            crucible::prune_patches,
            "patches",
            "prune",
        ),
    }
}

/// Resolves the review id, runs one comment operation, and prints its JSON result.
fn comments<T: serde::Serialize>(
    review_id: Option<&str>,
    operation: impl FnOnce(&str) -> Result<T, crucible::CrucibleError>,
    action: &str,
) -> ExitCode {
    review_json(review_id, operation, "comments", action)
}

fn review_json<T: serde::Serialize>(
    review_id: Option<&str>,
    operation: impl FnOnce(&str) -> Result<T, crucible::CrucibleError>,
    what: &str,
    action: &str,
) -> ExitCode {
    let mut log = crate::log::CommandLog::start(format!("{what} {action}"));
    let review_id = if review_id.is_some() {
        match review_id_or_head(review_id, what) {
            Ok(review_id) => review_id,
            Err(code) => return code,
        }
    } else {
        match log.time("git", || review_id_or_head(None, what)) {
            Ok(review_id) => review_id,
            Err(code) => return code,
        }
    };
    match log.time("crucible", || operation(&review_id)) {
        Ok(result) => {
            let json = serde_json::to_string_pretty(&result).expect("review json serialize");
            println!("{json}");
            log.finish(format!("ok {review_id}"));
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
    let mut log = crate::log::CommandLog::start("land");
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
    let acceptance = match log.time("crucible_check", || {
        land_acceptance(&repository, &upstream, &config)
    }) {
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
    if let Err(error) = log.time("pre-hooks", || {
        hooks::run_pre(
            &repository,
            "pre-land",
            &serde_json::json!({
                "event": "pre-land",
                "command": "land",
                "review_id": acceptance.review_id,
                "url": acceptance.review_url,
                "branch": current,
                "target": review_target,
            }),
        )
    }) {
        return land_failed(error);
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
    if let Err(error) = log.time("rebase", || repository.pull_rebase_origin(remote_branch)) {
        return land_failed(error);
    }
    match repository.head_oneline() {
        Ok(line) => println!("{line}"),
        Err(error) => return land_failed(error),
    }
    let prompt = format!("push branch '{current}' to '{upstream}'? [y/N] ");
    if !yes && !log.time("wait", || confirm_yes(&prompt)) {
        println!("Aborted");
        log.finish("aborted");
        return ExitCode::SUCCESS;
    }
    if let Err(error) = log.time("push", || {
        repository.push_to_origin(&current, remote_branch)
    }) {
        return land_failed(error);
    }
    println!("Pushed: {current} -> {upstream}");
    if let Err(error) = log.time("crucible_close", || {
        crucible::close_review(&config, &acceptance.review_id, &acceptance.state)
    }) {
        eprintln!("land failed: review was pushed, but closing failed: {error}");
        return ExitCode::FAILURE;
    }
    println!("Closed: {}", acceptance.review_id);
    log.time("post-hooks", || {
        hooks::run_post(
            &repository,
            "post-land",
            &serde_json::json!({
                "event": "post-land",
                "command": "land",
                "review_id": acceptance.review_id,
                "url": acceptance.review_url,
                "branch": current,
                "target": review_target,
            }),
        );
    });
    log.finish(format!("ok {}", acceptance.review_id));
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
    let mut log = crate::log::CommandLog::start("doctor");
    let repository = match log.time("git", || git_repository::Repository::discover()) {
        Ok(repository) => repository,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    println!("Git repository: {}", repository.work_tree().display());
    log.finish("ok");
    ExitCode::SUCCESS
}

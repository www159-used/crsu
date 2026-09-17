use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 当前 Git working tree，以及所有 crsu 工作流需要的仓库操作。
pub struct Repository {
    work_tree: PathBuf,
    common_dir: PathBuf,
}

/// 可提交给代码评审系统的 Git 差异。
pub struct ReviewDiff {
    base: String,
    commits: Vec<String>,
    patch: String,
    title: String,
    review_id: Option<String>,
    objectives: String,
}

impl Repository {
    /// 从当前目录发现仓库；linked worktree 会保留自己的 working tree，
    /// 同时解析到主仓库共享的 common directory。
    pub fn discover() -> Result<Self, Error> {
        let work_tree = discovery_output(["rev-parse", "--show-toplevel"])?;
        let common_dir =
            discovery_output(["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        Ok(Self {
            work_tree: PathBuf::from(work_tree),
            common_dir: PathBuf::from(common_dir),
        })
    }

    #[must_use]
    pub fn work_tree(&self) -> &Path {
        &self.work_tree
    }

    #[must_use]
    pub fn common_dir(&self) -> &Path {
        &self.common_dir
    }

    /// Returns the configured origin URL, when this repository has one.
    #[must_use]
    pub fn origin_url(&self) -> Option<String> {
        self.output(["config", "--get", "remote.origin.url"])
            .ok()
            .filter(|url| !url.is_empty())
    }

    /// 准备从基线到当前 HEAD 的完整评审差异。
    pub fn review_diff(&self, requested_base: Option<&str>) -> Result<ReviewDiff, Error> {
        self.ensure_clean_worktree()?;
        let base = self.resolve_base(requested_base)?;
        let commits = lines(&self.output(["rev-list", "--reverse", &format!("{base}..HEAD")])?);
        if commits.is_empty() {
            return Err(Error::NoChanges { base });
        }

        let patch = self.output_untrimmed(["diff", "--no-ext-diff", "--binary", &base, "HEAD"])?;
        if patch.is_empty() {
            return Err(Error::NoChanges { base });
        }

        let title = self.output(["show", "-s", "--format=%s", "HEAD"])?;
        let message = self.output(["show", "-s", "--format=%B", "HEAD"])?;
        let branch = self.output(["branch", "--show-current"])?;
        let last_tag = self
            .output(["describe", "--abbrev=0", "--tags"])
            .unwrap_or_else(|_| "None".to_owned());
        let objectives = objectives(&message, &branch, &base, &last_tag);
        Ok(ReviewDiff {
            base,
            commits,
            patch,
            title,
            review_id: review_id_from_message(&message),
            objectives,
        })
    }

    /// Associates HEAD with a Crucible review without changing its subject.
    pub fn attach_review(&self, review_url: &str, reviewers: &[String]) -> Result<(), Error> {
        let remote_refs = self.output([
            "for-each-ref",
            "--format=%(refname:short)",
            "--contains=HEAD",
            "refs/remotes",
        ])?;
        if !remote_refs.is_empty() {
            return Err(Error::PublishedCommit { remote_refs });
        }

        let message = self.output(["show", "-s", "--format=%B", "HEAD"])?;
        let amended = managed_commit_message(&message, review_url, reviewers);

        let mut child = Command::new("git")
            .args(["commit", "--amend", "-F", "-"])
            .current_dir(&self.work_tree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(Error::GitUnavailable)?;
        child
            .stdin
            .take()
            .ok_or_else(|| Error::GitCommand {
                message: "failed to open git commit message input".to_owned(),
            })?
            .write_all(amended.as_bytes())
            .map_err(Error::GitUnavailable)?;
        let output = child.wait_with_output().map_err(Error::GitUnavailable)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Error::GitCommand {
                message: text(&output.stderr),
            })
        }
    }

    fn ensure_clean_worktree(&self) -> Result<(), Error> {
        if self.output(["status", "--porcelain"])?.is_empty() {
            Ok(())
        } else {
            Err(Error::DirtyWorktree)
        }
    }

    fn resolve_base(&self, requested: Option<&str>) -> Result<String, Error> {
        let base = match requested {
            Some(base) => base.to_owned(),
            None => self.default_base()?,
        };
        self.output(["rev-parse", "--verify", &format!("{base}^{{commit}}")])?;
        Ok(base)
    }

    fn default_base(&self) -> Result<String, Error> {
        if let Ok(upstream) = self.output([
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ]) {
            return Ok(upstream);
        }

        if self
            .output(["rev-parse", "--verify", "origin/HEAD^{commit}"])
            .is_ok()
        {
            return Ok("origin/HEAD".to_owned());
        }

        Err(Error::NoDefaultBase)
    }

    fn output<const N: usize>(&self, arguments: [&str; N]) -> Result<String, Error> {
        command_output(&self.work_tree, arguments)
    }

    fn output_untrimmed<const N: usize>(&self, arguments: [&str; N]) -> Result<String, Error> {
        command_output_untrimmed(&self.work_tree, arguments)
    }
}

fn managed_commit_message(message: &str, review_url: &str, reviewers: &[String]) -> String {
    let mut paragraphs = message
        .trim()
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>();
    let subject = paragraphs.first().copied().unwrap_or_default();
    if !paragraphs.is_empty() {
        paragraphs.remove(0);
    }
    let summary = paragraphs
        .iter()
        .find(|paragraph| paragraph.starts_with("Summary:"))
        .map_or("Summary:", |paragraph| *paragraph);
    let preserved = paragraphs.into_iter().filter(|paragraph| {
        !["Summary:", "Reviewers:", "Reviewed By:", "Url:"]
            .iter()
            .any(|field| paragraph.starts_with(field))
    });
    let mut output = vec![subject.to_owned()];
    output.extend(preserved.map(ToOwned::to_owned));
    output.push(summary.to_owned());
    output.push(format!("Reviewers: {}", reviewers.join(", ")));
    output.push("Reviewed By:".to_owned());
    output.push(format!("Url: {}", review_url.trim()));
    format!("{}\n", output.join("\n\n"))
}

pub(crate) fn git_remotes_match(left: &str, right: &str) -> bool {
    match (
        normalized_git_location(left),
        normalized_git_location(right),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn normalized_git_location(value: &str) -> Option<String> {
    let mut location = value.trim().trim_end_matches('/').to_owned();
    if location.is_empty() {
        return None;
    }

    if let Some((_, remainder)) = location.split_once("://") {
        location = remainder.to_owned();
    } else if let Some((host, path)) = location.split_once(':')
        && !host.contains('/')
    {
        location = format!("{host}/{path}");
    }
    if let Some((_, remainder)) = location.rsplit_once('@') {
        location = remainder.to_owned();
    }

    let location = location.trim_end_matches('/').trim_end_matches(".git");
    (!location.is_empty()).then(|| location.to_ascii_lowercase())
}

impl ReviewDiff {
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    #[must_use]
    pub fn commit_count(&self) -> usize {
        self.commits.len()
    }

    #[must_use]
    pub fn patch_len(&self) -> usize {
        self.patch.len()
    }

    #[must_use]
    pub fn patch(&self) -> &str {
        &self.patch
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub fn review_id(&self) -> Option<&str> {
        self.review_id.as_deref()
    }

    #[must_use]
    pub fn objectives(&self) -> &str {
        &self.objectives
    }
}

fn objectives(message: &str, branch: &str, target: &str, last_tag: &str) -> String {
    let summary = message
        .trim()
        .split("\n\n")
        .map(str::trim)
        .find_map(|paragraph| paragraph.strip_prefix("Summary:").map(str::trim))
        .unwrap_or_default();
    let metadata = format!("[ branch: {branch} ]\n[ target: {target} ]\n[ last_tag: {last_tag} ]");
    if summary.is_empty() {
        metadata
    } else {
        format!("{summary}\n\n{metadata}")
    }
}

fn review_id_from_message(message: &str) -> Option<String> {
    message.lines().find_map(|line| {
        let url = line
            .trim()
            .strip_prefix("Url:")?
            .trim()
            .trim_end_matches('/');
        let review_id = url.rsplit('/').next()?;
        (!review_id.is_empty() && review_id.contains('-')).then(|| review_id.to_owned())
    })
}

fn discovery_output<const N: usize>(arguments: [&str; N]) -> Result<String, Error> {
    let output = Command::new("git")
        .args(arguments)
        .output()
        .map_err(Error::GitUnavailable)?;
    if output.status.success() {
        Ok(text(&output.stdout))
    } else {
        Err(Error::NotRepository)
    }
}

fn command_output<const N: usize>(
    working_directory: &Path,
    arguments: [&str; N],
) -> Result<String, Error> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(working_directory)
        .output()
        .map_err(Error::GitUnavailable)?;
    if output.status.success() {
        return Ok(text(&output.stdout));
    }

    Err(Error::GitCommand {
        message: text(&output.stderr),
    })
}

fn command_output_untrimmed<const N: usize>(
    working_directory: &Path,
    arguments: [&str; N],
) -> Result<String, Error> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(working_directory)
        .output()
        .map_err(Error::GitUnavailable)?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }

    Err(Error::GitCommand {
        message: text(&output.stderr),
    })
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_owned()
}

fn lines(value: &str) -> Vec<String> {
    value.lines().map(ToOwned::to_owned).collect()
}

#[derive(Debug)]
pub enum Error {
    NotRepository,
    DirtyWorktree,
    GitUnavailable(std::io::Error),
    GitCommand { message: String },
    NoDefaultBase,
    NoChanges { base: String },
    PublishedCommit { remote_refs: String },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRepository => write!(formatter, "not a Git repository"),
            Self::DirtyWorktree => write!(formatter, "working tree has uncommitted changes"),
            Self::GitUnavailable(error) => write!(formatter, "Git unavailable: {error}"),
            Self::GitCommand { message } => write!(formatter, "git command failed: {message}"),
            Self::NoDefaultBase => write!(
                formatter,
                "no upstream or origin default branch; pass a base ref, for example: crsu diff origin/main"
            ),
            Self::NoChanges { base } => write!(formatter, "no changes relative to {base}"),
            Self::PublishedCommit { remote_refs } => write!(
                formatter,
                "refusing to amend HEAD because it is already on remote branch(es): {}",
                remote_refs.replace('\n', ", ")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{managed_commit_message, review_id_from_message};

    #[test]
    fn reads_legacy_cru_review_url_from_commit_body() {
        let message = "fix: example\n\nSummary:\n\nUrl: http://crucible/cru/LP-1472\n";

        assert_eq!(review_id_from_message(message).as_deref(), Some("LP-1472"));
    }

    #[test]
    fn ignores_commit_messages_without_a_review_url() {
        assert_eq!(review_id_from_message("fix: example"), None);
    }

    #[test]
    fn writes_legacy_cru_metadata_idempotently() {
        let reviewers = vec!["alice".to_owned(), "bob".to_owned()];
        let first = managed_commit_message(
            "fix: example\n\nSummary: details\n\nUrl: http://old/cru/LP-1",
            "http://cru/cru/LP-2",
            &reviewers,
        );
        let second = managed_commit_message(&first, "http://cru/cru/LP-2", &reviewers);

        assert_eq!(first, second);
        assert_eq!(first.matches("Summary:").count(), 1);
        assert_eq!(first.matches("Reviewers:").count(), 1);
        assert!(first.contains("Summary: details"));
        assert!(first.contains("Reviewers: alice, bob"));
        assert!(first.contains("Reviewed By:"));
        assert!(first.contains("Url: http://cru/cru/LP-2"));
        assert!(!first.contains("http://old"));
    }
}

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

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
        Ok(ReviewDiff {
            base,
            commits,
            patch,
            title,
            review_id: review_id_from_message(&message),
        })
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::review_id_from_message;

    #[test]
    fn reads_legacy_cru_review_url_from_commit_body() {
        let message = "fix: example\n\nSummary:\n\nUrl: http://crucible/cru/LP-1472\n";

        assert_eq!(review_id_from_message(message).as_deref(), Some("LP-1472"));
    }

    #[test]
    fn ignores_commit_messages_without_a_review_url() {
        assert_eq!(review_id_from_message("fix: example"), None);
    }
}

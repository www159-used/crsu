use std::fmt;
use std::process::Command;

/// 可提交给代码评审系统的 Git 差异。
pub struct ReviewDiff {
    base: String,
    commits: Vec<String>,
    patch: String,
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
}

/// 从当前 Git 工作区准备待审差异。
pub fn from_current_repository(base: Option<&str>) -> Result<ReviewDiff, DiffError> {
    ensure_clean_worktree()?;
    let base = resolve_base(base)?;
    let commits = git_output(["rev-list", "--reverse", &format!("{base}..HEAD")])?;
    let commits = lines(&commits);
    if commits.is_empty() {
        return Err(DiffError::NoChanges { base });
    }

    let patch = git_output(["diff", "--no-ext-diff", "--binary", &base, "HEAD"])?;
    if patch.is_empty() {
        return Err(DiffError::NoChanges { base });
    }

    Ok(ReviewDiff {
        base,
        commits,
        patch,
    })
}

fn ensure_clean_worktree() -> Result<(), DiffError> {
    let status = git_output(["status", "--porcelain"])?;
    if status.is_empty() {
        Ok(())
    } else {
        Err(DiffError::DirtyWorktree)
    }
}

fn resolve_base(base: Option<&str>) -> Result<String, DiffError> {
    let base = match base {
        Some(base) => base.to_owned(),
        None => git_output([
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ])?,
    };
    git_output(["rev-parse", "--verify", &format!("{base}^{{commit}}")])?;
    Ok(base)
}

fn git_output<const N: usize>(arguments: [&str; N]) -> Result<String, DiffError> {
    let output = Command::new("git")
        .args(arguments)
        .output()
        .map_err(DiffError::GitUnavailable)?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned());
    }

    Err(DiffError::GitCommand {
        message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

fn lines(value: &str) -> Vec<String> {
    value.lines().map(ToOwned::to_owned).collect()
}

#[derive(Debug)]
pub enum DiffError {
    DirtyWorktree,
    GitUnavailable(std::io::Error),
    GitCommand { message: String },
    NoChanges { base: String },
}

impl fmt::Display for DiffError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DirtyWorktree => write!(formatter, "working tree has uncommitted changes"),
            Self::GitUnavailable(error) => write!(formatter, "git is unavailable: {error}"),
            Self::GitCommand { message } => write!(formatter, "git command failed: {message}"),
            Self::NoChanges { base } => write!(formatter, "no changes relative to {base}"),
        }
    }
}

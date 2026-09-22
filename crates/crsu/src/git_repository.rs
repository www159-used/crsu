use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 当前 Git working tree，以及所有 crsu 工作流需要的仓库操作。
pub struct Repository {
    work_tree: PathBuf,
    common_dir: PathBuf,
}

/// One review summary collected from a commit message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewShare {
    pub target: String,
    pub title: String,
    pub url: String,
}

/// Changed files and +/- lines in a review patch. Reviews stop at this many lines.
pub const MAX_REVIEW_LINES: usize = 1000;

/// File and line counts taken from the patch that would be uploaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchStats {
    pub files: usize,
    pub lines: usize,
}

/// Counts `diff --git` files and added/removed lines, ignoring `---` / `+++` headers.
#[must_use]
pub fn patch_stats(patch: &str) -> PatchStats {
    let mut files = 0;
    let mut lines = 0;
    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            files += 1;
        } else if is_changed_line(line) {
            lines += 1;
        }
    }
    PatchStats { files, lines }
}

fn is_changed_line(line: &str) -> bool {
    matches!(line.as_bytes().first(), Some(b'+' | b'-'))
        && !line.starts_with("+++")
        && !line.starts_with("---")
}

/// Returns why this patch must not be submitted, if it is over the line limit.
#[must_use]
pub fn patch_line_limit(lines: usize) -> Option<Error> {
    (lines > MAX_REVIEW_LINES).then_some(Error::PatchTooLarge { lines })
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
        let commits = self.commits_ahead_of(&base)?;
        if commits.is_empty() {
            return Err(Error::NoChanges { base });
        }

        let patch = self.output_untrimmed(["diff", "--no-ext-diff", "--binary", &base, "HEAD"])?;
        if patch.is_empty() {
            return Err(Error::NoChanges { base });
        }

        let title = self.output(["show", "-s", "--format=%s", "HEAD"])?;
        let message = self.commit_message("HEAD")?;
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

        let message = self.commit_message("HEAD")?;
        let amended = managed_commit_message(&message, review_url, reviewers, &[]);
        self.commit_with_message(&amended, true)
    }

    /// Collects `[target] title url` lines for a JIRA key without checking out branches.
    pub fn review_shares(
        &self,
        jira: &str,
        branches: &[String],
    ) -> Result<Vec<ReviewShare>, Error> {
        let refs = if branches.is_empty() {
            self.copy_scan_refs()?
        } else {
            branches.to_vec()
        };
        let mut by_url = std::collections::BTreeMap::new();
        for git_ref in refs {
            let Some(hash) = self.newest_commit_mentioning(&git_ref, jira)? else {
                continue;
            };
            let message = self.commit_message(&hash)?;
            let fallback = self.display_target(&git_ref);
            let Some(share) = share_from_message(&message, &fallback) else {
                continue;
            };
            by_url
                .entry(share.url.clone())
                .and_modify(|existing: &mut ReviewShare| {
                    if prefer_origin_target(&share.target, &existing.target) {
                        *existing = share.clone();
                    }
                })
                .or_insert(share);
        }
        let mut shares = by_url.into_values().collect::<Vec<_>>();
        shares.sort_by(|left, right| left.target.cmp(&right.target));
        if shares.is_empty() {
            Err(Error::NoJiraReviews {
                jira: jira.to_owned(),
            })
        } else {
            Ok(shares)
        }
    }

    /// Collects `[target] title url` from each ref tip without checking out.
    pub fn review_shares_at_refs(&self, refs: &[String]) -> Result<Vec<ReviewShare>, Error> {
        let mut shares = Vec::new();
        for git_ref in refs {
            let message = self.commit_message(git_ref)?;
            let fallback = self.display_target(git_ref);
            if let Some(share) = share_from_message(&message, &fallback) {
                shares.push(share);
            }
        }
        if shares.is_empty() {
            Err(Error::NoJiraReviews {
                jira: refs.join(","),
            })
        } else {
            Ok(shares)
        }
    }

    fn newest_commit_mentioning(&self, git_ref: &str, jira: &str) -> Result<Option<String>, Error> {
        let grep = format!("--grep={jira}");
        let hashes = self.git(&["log", git_ref, "-F", &grep, "--format=%H"])?;
        Ok(lines(&hashes).into_iter().next())
    }

    fn copy_scan_refs(&self) -> Result<Vec<String>, Error> {
        let remotes = self.git(&[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/remotes/origin",
        ])?;
        let heads = self.git(&["for-each-ref", "--format=%(refname:short)", "refs/heads"])?;
        Ok(lines(&remotes)
            .into_iter()
            .chain(lines(&heads))
            .filter(|git_ref| git_ref != "origin/HEAD")
            .collect())
    }

    fn display_target(&self, git_ref: &str) -> String {
        let name = land_ref_key(git_ref);
        let origin = format!("origin/{name}");
        if self
            .git(&["rev-parse", "--verify", &format!("{origin}^{{commit}}")])
            .is_ok()
        {
            origin
        } else {
            git_ref.to_owned()
        }
    }

    /// Collects the clipboard summary fields from HEAD without requiring a clean worktree.
    pub fn share_summary(
        &self,
        requested_base: Option<&str>,
    ) -> Result<(String, String, String), Error> {
        let base = self.resolve_base(requested_base)?;
        let title = self.output(["show", "-s", "--format=%s", "HEAD"])?;
        let message = self.commit_message("HEAD")?;
        let url = review_url_from_message(&message).ok_or(Error::NoReviewUrl)?;
        Ok((base, title, url))
    }

    pub fn current_branch(&self) -> Result<String, Error> {
        let branch = self.output(["branch", "--show-current"])?;
        if branch.is_empty() {
            Err(Error::GitCommand {
                message: "HEAD is not on a branch".to_owned(),
            })
        } else {
            Ok(branch)
        }
    }

    pub fn upstream_of(&self, branch: &str) -> Result<String, Error> {
        self.upstream_ref(&format!("{branch}@{{upstream}}"))
            .map_err(|_| Error::NoUpstream {
                branch: branch.to_owned(),
            })
    }

    /// Commit hashes reachable from HEAD but not `base`, oldest first.
    pub fn commits_ahead_of(&self, base: &str) -> Result<Vec<String>, Error> {
        Ok(lines(&self.output([
            "rev-list",
            "--reverse",
            &format!("{base}..HEAD"),
        ])?))
    }

    pub fn commit_message(&self, commit: &str) -> Result<String, Error> {
        self.output(["show", "-s", "--format=%B", commit])
    }

    /// Reads the Crucible review id from the HEAD commit message.
    pub fn review_id_from_head(&self) -> Result<String, Error> {
        let message = self.commit_message("HEAD")?;
        review_id_from_message(&message).ok_or(Error::NoReviewUrl)
    }

    /// Amends HEAD with land metadata. The caller must already have exactly one commit to land.
    pub fn prepare_land_commit(
        &self,
        review_url: &str,
        reviewers: &[String],
        reviewed_by: &[String],
    ) -> Result<(), Error> {
        let message = self.commit_message("HEAD")?;
        let amended = managed_commit_message(&message, review_url, reviewers, reviewed_by);
        self.commit_with_message(&amended, true)
    }

    pub fn pull_rebase_origin(&self, remote_branch: &str) -> Result<(), Error> {
        self.output(["pull", "--rebase", "origin", remote_branch])
            .map(|_| ())
    }

    pub fn push_to_origin(&self, local_branch: &str, remote_branch: &str) -> Result<(), Error> {
        self.output(["push", "origin", &format!("{local_branch}:{remote_branch}")])
            .map(|_| ())
    }

    pub fn head_oneline(&self) -> Result<String, Error> {
        self.output(["log", "-n", "1", "--oneline"])
    }

    pub fn diff_shortstat(&self, base: &str) -> Result<String, Error> {
        self.output(["diff", "--shortstat", &format!("{base}...HEAD")])
    }

    /// Returns why `upstream` must not receive this land, if the review target disagrees.
    #[must_use]
    pub fn land_target_error(&self, review_target: &str, upstream: &str) -> Option<Error> {
        if self.canonical_land_ref(review_target) == self.canonical_land_ref(upstream) {
            None
        } else {
            Some(Error::TargetMismatch {
                review: review_target.to_owned(),
                land: upstream.to_owned(),
            })
        }
    }

    fn canonical_land_ref(&self, name: &str) -> String {
        let key = land_ref_key(name);
        if key.eq_ignore_ascii_case("HEAD")
            && let Ok(resolved) =
                self.output(["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        {
            return land_ref_key(&resolved).to_owned();
        }
        key.to_owned()
    }

    fn commit_with_message(&self, message: &str, amend: bool) -> Result<(), Error> {
        let mut args = vec!["commit"];
        if amend {
            args.push("--amend");
        }
        args.extend(["-F", "-"]);
        let mut child = Command::new("git")
            .args(&args)
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
            .write_all(message.as_bytes())
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

    pub(crate) fn ensure_clean_worktree(&self) -> Result<(), Error> {
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
        if let Ok(upstream) = self.upstream_ref("@{upstream}") {
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

    /// Resolves a rev-parse upstream specifier such as `main@{upstream}` to a full ref name.
    fn upstream_ref(&self, specifier: &str) -> Result<String, Error> {
        self.output([
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            specifier,
        ])
    }

    fn output<const N: usize>(&self, arguments: [&str; N]) -> Result<String, Error> {
        command_output(&self.work_tree, arguments)
    }

    fn git(&self, arguments: &[&str]) -> Result<String, Error> {
        command_output_slice(&self.work_tree, arguments)
    }

    fn output_untrimmed<const N: usize>(&self, arguments: [&str; N]) -> Result<String, Error> {
        command_output_untrimmed(&self.work_tree, arguments)
    }

    pub(crate) fn complete_refs(&self) -> Vec<String> {
        self.git(&[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes",
        ])
        .map(|text| {
            lines(&text)
                .into_iter()
                .filter(|git_ref| !git_ref.is_empty())
                .collect()
        })
        .unwrap_or_default()
    }

    pub(crate) fn complete_review_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        if let Ok(id) = self.review_id_from_head() {
            seen.insert(id.clone());
            ids.push(id);
        }
        if let Ok(body) = self.git(&["log", "-n", "80", "--format=%B"]) {
            for line in body.lines() {
                if let Some(id) = review_id_from_message(line) {
                    if seen.insert(id.clone()) {
                        ids.push(id);
                    }
                }
            }
        }
        ids
    }

    pub(crate) fn complete_log_text(&self) -> String {
        self.git(&["log", "-n", "80", "--format=%s%n%b"])
            .unwrap_or_default()
    }
}

fn managed_commit_message(
    message: &str,
    review_url: &str,
    reviewers: &[String],
    reviewed_by: &[String],
) -> String {
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
    output.push(if reviewed_by.is_empty() {
        "Reviewed By:".to_owned()
    } else {
        format!("Reviewed By: {}", reviewed_by.join(", "))
    });
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
    /// Treats the diff as a new review without modifying HEAD before submission.
    #[must_use]
    pub(crate) fn without_review(mut self) -> Self {
        self.review_id = None;
        self
    }

    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    #[must_use]
    pub fn commit_count(&self) -> usize {
        self.commits.len()
    }

    #[must_use]
    pub fn stats(&self) -> PatchStats {
        patch_stats(&self.patch)
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

fn share_from_message(message: &str, fallback_target: &str) -> Option<ReviewShare> {
    let url = review_url_from_message(message)?;
    let title = message.lines().next()?.trim();
    if title.is_empty() {
        return None;
    }
    Some(ReviewShare {
        target: target_from_objectives(message)
            .unwrap_or(fallback_target)
            .to_owned(),
        title: title.to_owned(),
        url,
    })
}

fn prefer_origin_target(new: &str, old: &str) -> bool {
    new.starts_with("origin/") && !old.starts_with("origin/")
}

pub(crate) fn target_from_objectives(objectives: &str) -> Option<&str> {
    objectives.lines().find_map(|line| {
        let target = line
            .trim()
            .strip_prefix("[ target:")?
            .strip_suffix(']')?
            .trim();
        (!target.is_empty()).then_some(target)
    })
}

pub(crate) fn land_ref_key(name: &str) -> &str {
    let name = name.trim().trim_end_matches('/');
    name.strip_prefix("refs/remotes/origin/")
        .or_else(|| name.strip_prefix("refs/remotes/"))
        .or_else(|| name.strip_prefix("refs/heads/"))
        .or_else(|| name.strip_prefix("origin/"))
        .unwrap_or(name)
}

pub(crate) fn review_url_from_message(message: &str) -> Option<String> {
    message.lines().find_map(|line| {
        let url = line.trim().strip_prefix("Url:")?.trim();
        let url = url.trim_end_matches('/');
        (!url.is_empty()).then(|| url.to_owned())
    })
}

pub(crate) fn review_id_from_message(message: &str) -> Option<String> {
    let url = review_url_from_message(message)?;
    let review_id = url.rsplit('/').next()?;
    (!review_id.is_empty() && review_id.contains('-')).then(|| review_id.to_owned())
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
    command_output_slice(working_directory, &arguments)
}

fn command_output_slice(working_directory: &Path, arguments: &[&str]) -> Result<String, Error> {
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
    NoUpstream { branch: String },
    NoReviewUrl,
    NoJiraReviews { jira: String },
    NothingToLand,
    MultipleCommits { count: usize },
    MissingReviewTarget,
    TargetMismatch { review: String, land: String },
    CrossBranchLand { current: String, target: String },
    NoChanges { base: String },
    PublishedCommit { remote_refs: String },
    PatchTooLarge { lines: usize },
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
            Self::NoUpstream { branch } => write!(
                formatter,
                "no upstream found on branch {branch}; run git branch --set-upstream-to=origin/{branch}"
            ),
            Self::NoReviewUrl => write!(
                formatter,
                "HEAD commit has no Crucible Url; run crsu diff first"
            ),
            Self::NoJiraReviews { jira } => {
                write!(formatter, "no reviews with a Url: trailer for {jira}")
            }
            Self::NothingToLand => write!(formatter, "nothing to land"),
            Self::MultipleCommits { count } => write!(
                formatter,
                "{count} commits ahead of upstream; squash to one commit before landing"
            ),
            Self::MissingReviewTarget => write!(
                formatter,
                "review has no recorded target; refuse to land without --force"
            ),
            Self::TargetMismatch { review, land } => write!(
                formatter,
                "review targets {review}, but land would push to {land}; use --force to override"
            ),
            Self::CrossBranchLand { current, target } => write!(
                formatter,
                "cross-branch land from {current} to {target} is not implemented; checkout {target} or omit the target"
            ),
            Self::NoChanges { base } => write!(formatter, "no changes relative to {base}"),
            Self::PatchTooLarge { lines } => write!(
                formatter,
                "patch has {lines} changed lines; reviews are limited to {MAX_REVIEW_LINES}; use --force to submit anyway"
            ),
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
    use super::{
        MAX_REVIEW_LINES, land_ref_key, managed_commit_message, patch_line_limit, patch_stats,
        review_id_from_message, review_url_from_message, share_from_message,
        target_from_objectives,
    };

    #[test]
    fn reads_legacy_cru_review_url_from_commit_body() {
        let message = "fix: example\n\nSummary:\n\nUrl: http://crucible/cru/LP-1472\n";

        assert_eq!(
            review_url_from_message(message).as_deref(),
            Some("http://crucible/cru/LP-1472")
        );
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
            &[],
        );
        let second = managed_commit_message(&first, "http://cru/cru/LP-2", &reviewers, &[]);

        assert_eq!(first, second);
        assert_eq!(first.matches("Summary:").count(), 1);
        assert_eq!(first.matches("Reviewers:").count(), 1);
        assert!(first.contains("Summary: details"));
        assert!(first.contains("Reviewers: alice, bob"));
        assert!(first.contains("Reviewed By:"));
        assert!(first.contains("Url: http://cru/cru/LP-2"));
        assert!(!first.contains("http://old"));
    }

    #[test]
    fn reads_share_line_fields_from_a_review_commit() {
        let message = "\
[TIC-10733] fix: fd leak

[ branch: four-six ]
[ target: origin/4.6 ]
[ last_tag: None ]

Url: http://crucible/cru/LP-1476
";
        let share = share_from_message(message, "origin/5.0").expect("share");
        assert_eq!(share.target, "origin/4.6");
        assert_eq!(share.title, "[TIC-10733] fix: fd leak");
        assert_eq!(share.url, "http://crucible/cru/LP-1476");
        let inferred = share_from_message(
            "[TIC-10733] fix: fd leak\n\nUrl: http://crucible/cru/LP-1476\n",
            "origin/4.6",
        )
        .expect("inferred");
        assert_eq!(inferred.target, "origin/4.6");
        assert_eq!(share_from_message("fix: no url", "origin/4.6"), None);
    }

    #[test]
    fn reads_review_target_from_objectives() {
        let objectives = "[ branch: feature ]\n[ target: origin/main ]\n[ last_tag: None ]";
        assert_eq!(target_from_objectives(objectives), Some("origin/main"));
        assert_eq!(target_from_objectives("no metadata"), None);
    }

    #[test]
    fn land_ref_keys_treat_origin_prefix_as_the_same_branch() {
        assert_eq!(land_ref_key("origin/feature"), "feature");
        assert_eq!(land_ref_key("refs/heads/feature"), "feature");
        assert_eq!(land_ref_key("feature"), "feature");
    }

    #[test]
    fn counts_files_and_changed_lines_not_headers() {
        let patch = "\
diff --git a/README.md b/README.md
index 111..222 100644
--- a/README.md
+++ b/README.md
@@ -1,2 +1,2 @@
 context
-old
+new
diff --git a/src/lib.rs b/src/lib.rs
new file mode 100644
--- /dev/null
+++ b/src/lib.rs
@@ -0,0 +1,2 @@
+one
+two
";
        let stats = patch_stats(patch);
        assert_eq!(stats.files, 2);
        assert_eq!(stats.lines, 4);
    }

    #[test]
    fn allows_exactly_the_line_limit() {
        assert_eq!(MAX_REVIEW_LINES, 1000);
        assert!(patch_line_limit(MAX_REVIEW_LINES).is_none());
        assert!(patch_line_limit(MAX_REVIEW_LINES + 1).is_some());
    }
}

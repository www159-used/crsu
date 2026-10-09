//! Executes the declarative scenarios in `e2e/land/*.yaml`.

use super::common::{
    CrucibleEnv, Expectation, assert_contains_all, assert_hooks, assert_no_token_leak,
    assert_scenario, create_origin, init_repository, install_crsu_hooks, install_global_hooks,
    load_yaml, run_crsu_with_config_home, run_git, write_files,
};
use crsu_testkit::{LandFixture, LandReviewer, MockCrucible};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub fn run(path: &Path) {
    run_scenario(load_yaml(path, "land scenario"));
}

fn run_scenario(mut scenario: Scenario) {
    let repository = ScenarioRepository::create(&scenario.repository);
    install_crsu_hooks(repository.root.path(), &scenario.hooks);
    let upstream_sha = repository.git_output([
        "rev-parse",
        &format!("origin/{}", scenario.repository.feature_branch),
    ]);
    let config_home = TempDir::new().expect("isolate CRSU_CONFIG_HOME");
    install_global_hooks(config_home.path(), &scenario.global_hooks);
    let server = MockCrucible::start_land(&LandFixture {
        token: scenario.crucible.token.clone(),
        review_id: scenario.crucible.review_id.clone(),
        title: scenario.crucible.title.clone(),
        state: scenario.crucible.state.clone(),
        objectives: scenario.crucible.objectives.clone(),
        reviewers: scenario.crucible.reviewers.clone(),
    });
    let env = CrucibleEnv {
        url: server.base_url(),
        project: &scenario.crucible.project,
        token: &scenario.crucible.token,
        reviewers: &[],
        repository: None,
    };
    let output = run_crsu_with_config_home(
        Some(&repository.working_directory),
        &scenario.command,
        Some(&env),
        config_home.path(),
    );
    if scenario.expect.success {
        server.assert_review_request();
    }

    assert_scenario(&scenario.name, &output, &scenario.expect);
    let body = repository.git_output(["show", "-s", "--format=%b", "HEAD"]);
    assert_contains_all(&scenario.name, &body, &scenario.expect.head_body_contains);
    if scenario.expect.success && !scenario.repository.upstream_files.is_empty() {
        assert_eq!(
            repository.git_output(["rev-parse", "HEAD^"]),
            upstream_sha,
            "scenario '{}' rebased onto the advanced upstream",
            scenario.name,
        );
    }
    if let Some(expected) = &mut scenario.expect.hook_json {
        if expected["landed_sha"] == "$pushed_commit" {
            expected["landed_sha"] = serde_json::json!(super::common::git_output(
                repository.origin.path(),
                [
                    "rev-parse",
                    &format!("refs/heads/{}", scenario.repository.feature_branch),
                ],
            ));
        }
        if expected["worktree_path"] == "$worktree_path" {
            expected["worktree_path"] =
                serde_json::json!(repository.git_output(["rev-parse", "--show-toplevel"]));
        }
    }
    assert_hooks(
        &scenario.name,
        repository.root.path(),
        &scenario.expect,
        Some((server.base_url().as_str(), "http://crucible")),
    );
    if !scenario.expect.success
        && (scenario.hooks.contains_key("post-land")
            || scenario.global_hooks.contains_key("post-land"))
    {
        assert!(
            !repository
                .root
                .path()
                .join(".git/crsu/hooks/captured.json")
                .exists(),
            "scenario '{}' must not run post-land after failure",
            scenario.name,
        );
    }
    assert_no_token_leak(&scenario.name, &output, &scenario.crucible.token);
}

struct ScenarioRepository {
    root: TempDir,
    _worktree: Option<TempDir>,
    origin: TempDir,
    working_directory: PathBuf,
}

impl ScenarioRepository {
    fn create(specification: &Repository) -> Self {
        let repository = init_repository(
            &specification.base_branch,
            &specification.base_files,
            "base",
        );
        let origin = create_origin(
            repository.path(),
            &specification.base_branch,
            &specification.origin.default_branch,
        );
        // Publish the feature branch tip at base so upstream exists, then commit locally.
        run_git(
            repository.path(),
            [
                "push",
                "origin",
                &format!(
                    "{}:{}",
                    specification.base_branch, specification.feature_branch
                ),
            ],
        );
        let (worktree, working_directory) = if specification.linked_worktree {
            let worktree = TempDir::new().expect("create linked worktree directory");
            run_git(
                repository.path(),
                [
                    "worktree",
                    "add",
                    "-b",
                    &specification.feature_branch,
                    worktree.path().to_str().expect("worktree path is UTF-8"),
                ],
            );
            let path = worktree.path().to_path_buf();
            (Some(worktree), path)
        } else {
            run_git(
                repository.path(),
                ["checkout", "-b", &specification.feature_branch],
            );
            (None, repository.path().to_path_buf())
        };
        run_git(
            &working_directory,
            [
                "branch",
                "--set-upstream-to",
                &format!("origin/{}", specification.feature_branch),
            ],
        );
        write_files(&working_directory, &specification.feature_files);
        run_git(&working_directory, ["add", "."]);
        run_git(
            &working_directory,
            ["commit", "-m", specification.feature_message.as_str()],
        );

        if !specification.upstream_files.is_empty() {
            run_git(repository.path(), ["checkout", &specification.base_branch]);
            write_files(repository.path(), &specification.upstream_files);
            run_git(repository.path(), ["add", "."]);
            run_git(repository.path(), ["commit", "-m", "upstream advances"]);
            run_git(
                repository.path(),
                [
                    "push",
                    "origin",
                    &format!(
                        "{}:{}",
                        specification.base_branch, specification.feature_branch
                    ),
                ],
            );
            if !specification.linked_worktree {
                run_git(
                    repository.path(),
                    ["checkout", &specification.feature_branch],
                );
            }
        }

        Self {
            working_directory,
            root: repository,
            _worktree: worktree,
            origin,
        }
    }

    fn git_output<const N: usize>(&self, arguments: [&str; N]) -> String {
        super::common::git_output(&self.working_directory, arguments)
    }
}

#[derive(Deserialize)]
struct Scenario {
    name: String,
    repository: Repository,
    command: Vec<String>,
    crucible: Crucible,
    #[serde(default)]
    hooks: BTreeMap<String, String>,
    #[serde(default)]
    global_hooks: BTreeMap<String, String>,
    expect: Expectation,
}

#[derive(Deserialize)]
struct Crucible {
    project: String,
    token: String,
    review_id: String,
    title: String,
    state: String,
    #[serde(default)]
    objectives: String,
    reviewers: Vec<LandReviewer>,
}

#[derive(Deserialize)]
struct Repository {
    base_branch: String,
    base_files: BTreeMap<String, String>,
    feature_branch: String,
    feature_files: BTreeMap<String, String>,
    feature_message: String,
    origin: Origin,
    #[serde(default)]
    linked_worktree: bool,
    #[serde(default)]
    upstream_files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Origin {
    default_branch: String,
}

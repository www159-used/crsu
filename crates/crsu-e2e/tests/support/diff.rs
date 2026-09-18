//! Executes the declarative scenarios in `e2e/diff/*.yaml`.

use super::common::{
    CrucibleEnv, Expectation, assert_contains_all, assert_hooks, assert_no_token_leak,
    assert_scenario, create_origin, init_repository, install_crsu_hooks, install_global_hooks,
    load_yaml, run_crsu_with_xdg, run_git, write_files,
};
use crsu_testkit::{MockCrucible, ReviewFixture, ReviewResponse};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Output;
use tempfile::TempDir;

pub fn run(path: &Path) {
    run_scenario(&load_yaml(path, "diff scenario"));
}

fn run_scenario(scenario: &Scenario) {
    let repository = ScenarioRepository::create(&scenario.repository);
    install_crsu_hooks(&repository.working_directory, &scenario.hooks);
    let xdg = TempDir::new().expect("isolate XDG_CONFIG_HOME");
    install_global_hooks(xdg.path(), &scenario.global_hooks);
    let output = run_with_optional_crucible(&repository, scenario, xdg.path());

    assert_scenario(&scenario.name, &output, &scenario.expect);
    let subject = repository.git_output(["show", "-s", "--format=%s", "HEAD"]);
    if let Some(expected) = &scenario.expect.head_subject {
        assert_eq!(
            &subject, expected,
            "scenario '{}' HEAD subject",
            scenario.name
        );
    }
    let body = repository.git_output(["show", "-s", "--format=%b", "HEAD"]);
    assert_contains_all(&scenario.name, &body, &scenario.expect.head_body_contains);
    if let Some(crucible) = &scenario.crucible {
        assert_no_token_leak(&scenario.name, &output, &crucible.token);
    }
}

fn run_with_optional_crucible(
    repository: &ScenarioRepository,
    scenario: &Scenario,
    xdg_config_home: &Path,
) -> Output {
    let Some(crucible) = &scenario.crucible else {
        let output = run_crsu_with_xdg(
            Some(&repository.working_directory),
            &scenario.command,
            None,
            xdg_config_home,
        );
        assert_hooks(
            &scenario.name,
            &repository.working_directory,
            &scenario.expect,
            None,
        );
        return output;
    };

    let server = MockCrucible::start_review(ReviewFixture {
        token: crucible.token.clone(),
        predecessor: match (
            crucible.previous_review_id.as_deref(),
            crucible.previous_state.as_deref(),
        ) {
            (Some(review_id), Some(state)) => Some(crsu_testkit::PredecessorReview {
                review_id: review_id.to_owned(),
                state: state.to_owned(),
            }),
            _ => None,
        },
        response: crucible.response.as_ref().map_or_else(
            || {
                let review_id = crucible.review_id.clone().expect("successful review id");
                match &crucible.current_title {
                    None => ReviewResponse::Created { review_id },
                    Some(current_title) => ReviewResponse::Updated {
                        review_id,
                        current_title: current_title.clone(),
                        current_objectives: crucible
                            .current_objectives
                            .clone()
                            .expect("updated review requires current_objectives"),
                        current_state: crucible
                            .current_state
                            .clone()
                            .unwrap_or_else(|| "Draft".to_owned()),
                    },
                }
            },
            |response| ReviewResponse::Rejected {
                status: response.status,
                body: response.body.clone(),
            },
        ),
    });

    let env = CrucibleEnv {
        url: server.base_url(),
        project: &crucible.project,
        token: &crucible.token,
        reviewers: &crucible.reviewers,
        repository: crucible.repository.as_deref(),
    };
    let output = run_crsu_with_xdg(
        Some(&repository.working_directory),
        &scenario.command,
        Some(&env),
        xdg_config_home,
    );
    server.assert_review_request();
    assert_hooks(
        &scenario.name,
        &repository.working_directory,
        &scenario.expect,
        Some((server.base_url().as_str(), "http://crucible")),
    );
    if let Some(expected) = &scenario.expect.review {
        let actual = server.review();
        assert_eq!(
            actual.title, expected.title,
            "scenario '{}' title",
            scenario.name
        );
        assert_eq!(
            actual.objectives, expected.objectives,
            "scenario '{}' objectives",
            scenario.name
        );
        assert_eq!(
            actual.state, expected.state,
            "scenario '{}' state",
            scenario.name
        );
    }
    output
}

struct ScenarioRepository {
    _repository: TempDir,
    _worktree: Option<TempDir>,
    _origin: Option<TempDir>,
    working_directory: std::path::PathBuf,
}

impl ScenarioRepository {
    fn create(specification: &Repository) -> Self {
        let repository = init_repository(
            &specification.base_branch,
            &specification.base_files,
            "base",
        );

        let origin = specification.origin.as_ref().map(|origin| {
            let remote = create_origin(
                repository.path(),
                &specification.base_branch,
                &origin.default_branch,
            );
            run_git(
                repository.path(),
                ["remote", "set-head", "origin", "--auto"],
            );
            remote
        });

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
        write_files(&working_directory, &specification.feature_files);
        run_git(&working_directory, ["add", "."]);
        let feature_message = specification
            .feature_message
            .as_deref()
            .unwrap_or("feature change");
        run_git(
            &working_directory,
            ["commit", "--allow-empty", "-m", feature_message],
        );
        write_files(&working_directory, &specification.uncommitted_files);

        Self {
            _repository: repository,
            _worktree: worktree,
            _origin: origin,
            working_directory,
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
    crucible: Option<Crucible>,
    #[serde(default)]
    hooks: BTreeMap<String, String>,
    #[serde(default)]
    global_hooks: BTreeMap<String, String>,
    expect: Expectation,
}

#[derive(Deserialize)]
struct Crucible {
    project: String,
    repository: Option<String>,
    token: String,
    #[serde(default)]
    reviewers: Vec<String>,
    review_id: Option<String>,
    current_title: Option<String>,
    current_objectives: Option<String>,
    current_state: Option<String>,
    previous_review_id: Option<String>,
    previous_state: Option<String>,
    response: Option<CrucibleResponse>,
}

#[derive(Deserialize)]
struct CrucibleResponse {
    status: u16,
    body: String,
}

#[derive(Deserialize)]
struct Repository {
    base_branch: String,
    base_files: BTreeMap<String, String>,
    feature_branch: String,
    feature_files: BTreeMap<String, String>,
    feature_message: Option<String>,
    #[serde(default)]
    linked_worktree: bool,
    origin: Option<Origin>,
    #[serde(default)]
    uncommitted_files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Origin {
    default_branch: String,
}

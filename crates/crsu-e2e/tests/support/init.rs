use super::common::load_yaml;
use crsu::init_test_support::load_candidates;
use crsu_testkit::{FixtureRepository, FixtureUser, InitFixture, MockCrucible};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct Expect {
    projects: Vec<String>,
    repositories: Vec<String>,
    reviewers: Vec<String>,
    detected_repository: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    credentials: Credentials,
    token: String,
    is_fisheye: bool,
    projects: Vec<String>,
    repositories: Vec<FixtureRepository>,
    origin: String,
    users: Vec<FixtureUser>,
    expect: Expect,
}

pub fn run(path: &Path) {
    let scenario: Scenario = load_yaml(path, "init fixture scenario");
    let crucible = MockCrucible::start(InitFixture {
        username: scenario.credentials.username.clone(),
        password: scenario.credentials.password.clone(),
        token: scenario.token.clone(),
        is_fisheye: scenario.is_fisheye,
        projects: scenario.projects,
        repositories: scenario.repositories,
        users: scenario.users,
    });
    let candidates = load_candidates(
        &crucible.base_url(),
        &scenario.credentials.username,
        &scenario.credentials.password,
    )
    .unwrap_or_else(|error| panic!("{} failed: {error}", scenario.name));

    assert_eq!(candidates.token, scenario.token, "{} token", scenario.name);
    assert_eq!(
        candidates.projects, scenario.expect.projects,
        "{} projects",
        scenario.name
    );
    assert_eq!(
        candidates
            .repositories
            .iter()
            .map(|repository| repository.name.clone())
            .collect::<Vec<_>>(),
        scenario.expect.repositories,
        "{} repositories",
        scenario.name
    );
    assert_eq!(
        candidates.detected_repository(&scenario.origin),
        scenario.expect.detected_repository.as_deref(),
        "{} detected repository",
        scenario.name
    );
    assert_eq!(
        candidates
            .reviewers
            .iter()
            .map(|reviewer| reviewer.username.clone())
            .collect::<Vec<_>>(),
        scenario.expect.reviewers,
        "{} reviewers",
        scenario.name
    );
}

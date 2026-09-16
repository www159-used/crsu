use crsu::init_test_support::load_candidates;
use crsu_testkit::{FixtureUser, InitFixture, MockCrucible};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct User {
    username: String,
    display_name: String,
}

#[derive(Debug, Deserialize)]
struct Expect {
    projects: Vec<String>,
    repositories: Vec<String>,
    reviewers: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    credentials: Credentials,
    token: String,
    projects: Vec<String>,
    repositories: Vec<String>,
    users: Vec<User>,
    expect: Expect,
}

#[test]
fn init_fixture_scenarios_are_executable() {
    for path in scenario_paths() {
        let scenario: Scenario = serde_yaml::from_str(
            &std::fs::read_to_string(&path).expect("read init fixture scenario"),
        )
        .expect("parse init fixture scenario");
        let crucible = MockCrucible::start(InitFixture {
            username: scenario.credentials.username.clone(),
            password: scenario.credentials.password.clone(),
            token: scenario.token.clone(),
            projects: scenario.projects,
            repositories: scenario.repositories,
            users: scenario
                .users
                .into_iter()
                .map(|user| FixtureUser::new(user.username, user.display_name))
                .collect(),
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
            candidates.repositories, scenario.expect.repositories,
            "{} repositories",
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
}

fn scenario_paths() -> Vec<std::path::PathBuf> {
    let mut paths = std::fs::read_dir("scenarios/init")
        .expect("read init fixture scenarios")
        .map(|entry| entry.expect("read scenario entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "yaml")
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

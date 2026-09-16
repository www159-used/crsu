use crsu::init_test_support::{OverflowScreen, render_overflow_screen};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Screen {
    Project,
    Repository,
    ReviewerCandidates,
    SelectedReviewers,
}

#[derive(Debug, Deserialize)]
struct Terminal {
    width: u16,
    height: u16,
}

#[derive(Debug, Deserialize)]
struct Expect {
    visible_contains: Vec<String>,
    visible_not_contains: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    screen: Screen,
    items: usize,
    cursor: usize,
    #[serde(default)]
    selected_reviewers: usize,
    terminal: Terminal,
    expect: Expect,
}

#[test]
fn overflow_scenarios_are_executable() {
    for path in scenario_paths() {
        let scenario: Scenario =
            serde_yaml::from_str(&std::fs::read_to_string(&path).expect("read overflow scenario"))
                .expect("parse overflow scenario");
        let screen = match scenario.screen {
            Screen::Project => OverflowScreen::Project,
            Screen::Repository => OverflowScreen::Repository,
            Screen::ReviewerCandidates => OverflowScreen::ReviewerCandidates,
            Screen::SelectedReviewers => OverflowScreen::SelectedReviewers,
        };
        let frame = render_overflow_screen(
            screen,
            scenario.items,
            scenario.cursor,
            scenario.selected_reviewers,
            scenario.terminal.width,
            scenario.terminal.height,
        );
        for expected in scenario.expect.visible_contains {
            assert!(
                frame.contains(&expected),
                "{} should show {expected:?}\n{frame}",
                scenario.name
            );
        }
        for unexpected in scenario.expect.visible_not_contains {
            assert!(
                !frame.contains(&unexpected),
                "{} should hide {unexpected:?}\n{frame}",
                scenario.name
            );
        }
    }
}

fn scenario_paths() -> Vec<std::path::PathBuf> {
    let mut paths = std::fs::read_dir("scenarios/overflow")
        .expect("read overflow scenarios")
        .map(|entry| entry.expect("read scenario entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "yaml")
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

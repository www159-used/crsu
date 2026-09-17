use crsu::init_test_support::OverflowScreen;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::path::Path;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Screen {
    Project,
    Repository,
    ReviewerCandidates,
    SelectedReviewers,
}

#[derive(Debug, Deserialize)]
pub struct Terminal {
    pub width: u16,
    pub height: u16,
}

pub fn load_yaml<T: DeserializeOwned>(path: &Path, what: &str) -> T {
    serde_yaml::from_str(&std::fs::read_to_string(path).unwrap_or_else(|error| {
        panic!("read {what}: {error}");
    }))
    .unwrap_or_else(|error| panic!("parse {what}: {error}"))
}

pub fn overflow_screen(screen: Screen) -> OverflowScreen {
    match screen {
        Screen::Project => OverflowScreen::Project,
        Screen::Repository => OverflowScreen::Repository,
        Screen::ReviewerCandidates => OverflowScreen::ReviewerCandidates,
        Screen::SelectedReviewers => OverflowScreen::SelectedReviewers,
    }
}

pub fn assert_visible(name: &str, frame: &str, contains: &[String], not_contains: &[String]) {
    for expected in contains {
        assert!(
            frame.contains(expected),
            "{name} should show {expected:?}\n{frame}"
        );
    }
    for unexpected in not_contains {
        assert!(
            !frame.contains(unexpected),
            "{name} should hide {unexpected:?}\n{frame}"
        );
    }
}

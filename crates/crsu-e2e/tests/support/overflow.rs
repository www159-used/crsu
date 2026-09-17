use super::common::{Screen, Terminal, assert_visible, load_yaml, overflow_screen};
use serde::Deserialize;
use std::path::Path;

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

pub fn run(path: &Path) {
    let scenario: Scenario = load_yaml(path, "overflow scenario");
    let frame = crsu::init_test_support::render_overflow_screen(
        overflow_screen(scenario.screen),
        scenario.items,
        scenario.cursor,
        scenario.selected_reviewers,
        scenario.terminal.width,
        scenario.terminal.height,
    );
    assert_visible(
        &scenario.name,
        &frame,
        &scenario.expect.visible_contains,
        &scenario.expect.visible_not_contains,
    );
}

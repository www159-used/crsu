use super::common::{Screen, Terminal, assert_visible, load_yaml, overflow_screen};
use crsu::init_test_support::{
    FormFlow, InputMode, Step, TextInsertField, render_exit_confirmation, render_login_dialog,
    render_required_field_error, render_search_results, render_text_insert,
};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    render: Render,
    expect: Expect,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Render {
    LoginDialog {
        terminal: Terminal,
    },
    ExitConfirmation {
        terminal: Terminal,
    },
    RequiredFieldError {
        terminal: Terminal,
    },
    SearchResults {
        screen: Screen,
        query: String,
        items: usize,
        terminal: Terminal,
    },
    FormFlow {
        actions: Vec<Action>,
    },
    TextInsert {
        field: InsertField,
        value: String,
        terminal: Terminal,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Action {
    EnterInsert,
    ExitInsert,
    Advance,
    Back,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum InsertField {
    Url,
    Username,
    Password,
}

#[derive(Debug, Default, Deserialize)]
struct Expect {
    #[serde(default)]
    visible_contains: Vec<String>,
    #[serde(default)]
    visible_not_contains: Vec<String>,
    mode: Option<String>,
    step: Option<String>,
    cursor: Option<Cursor>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct Cursor {
    x: u16,
    y: u16,
}

pub fn run(path: &Path) {
    let scenario: Scenario = load_yaml(path, "UI scenario");
    let Scenario {
        name,
        render,
        expect,
    } = scenario;
    match render {
        Render::LoginDialog { terminal } => assert_visible(
            &name,
            &render_login_dialog(terminal.width, terminal.height),
            &expect.visible_contains,
            &expect.visible_not_contains,
        ),
        Render::ExitConfirmation { terminal } => assert_visible(
            &name,
            &render_exit_confirmation(terminal.width, terminal.height),
            &expect.visible_contains,
            &expect.visible_not_contains,
        ),
        Render::RequiredFieldError { terminal } => assert_visible(
            &name,
            &render_required_field_error(terminal.width, terminal.height),
            &expect.visible_contains,
            &expect.visible_not_contains,
        ),
        Render::SearchResults {
            screen,
            query,
            items,
            terminal,
        } => assert_visible(
            &name,
            &render_search_results(
                overflow_screen(screen),
                &query,
                items,
                terminal.width,
                terminal.height,
            ),
            &expect.visible_contains,
            &expect.visible_not_contains,
        ),
        Render::TextInsert {
            field,
            value,
            terminal,
        } => {
            let rendered = render_text_insert(
                text_insert_field(field),
                &value,
                terminal.width,
                terminal.height,
            );
            assert_visible(
                &name,
                &rendered.frame,
                &expect.visible_contains,
                &expect.visible_not_contains,
            );
            if let Some(cursor) = expect.cursor {
                assert_eq!(rendered.cursor, (cursor.x, cursor.y), "{name} cursor");
            }
        }
        Render::FormFlow { actions } => {
            let mut form = FormFlow::new();
            for action in actions {
                match action {
                    Action::EnterInsert => form.enter_insert(),
                    Action::ExitInsert => form.exit_insert(),
                    Action::Advance => form.advance(),
                    Action::Back => form.back(),
                }
            }
            if let Some(mode) = &expect.mode {
                assert_eq!(input_mode_name(form.input_mode()), mode, "{name} mode");
            }
            if let Some(step) = &expect.step {
                assert_eq!(step_name(form.step()), step, "{name} step");
            }
        }
    }
}

fn text_insert_field(field: InsertField) -> TextInsertField {
    match field {
        InsertField::Url => TextInsertField::Url,
        InsertField::Username => TextInsertField::Username,
        InsertField::Password => TextInsertField::Password,
    }
}

fn input_mode_name(mode: InputMode) -> &'static str {
    match mode {
        InputMode::Normal => "normal",
        InputMode::Insert => "insert",
    }
}

fn step_name(step: Step) -> &'static str {
    match step {
        Step::Connection => "connection",
        Step::Authentication => "authentication",
        Step::Project => "project",
        Step::Repository => "repository",
        Step::Reviewers => "reviewers",
        Step::Confirm => "confirm",
    }
}

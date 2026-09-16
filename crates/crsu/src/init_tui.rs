use crate::crucible::{RepositoryCandidate, User};
use crate::init_model::{FormFlow, InputMode};
use crate::init_workflow::{detected_repository, load_candidates};
use crate::project_config::{CrucibleConfig, ProjectConfig};
use ratatui::crossterm::event::{self, Event, KeyCode};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};
use std::process::ExitCode;

#[cfg(feature = "test-support")]
#[derive(Clone, Copy, Debug)]
pub enum OverflowScreen {
    Project,
    Repository,
    ReviewerCandidates,
    SelectedReviewers,
}

#[cfg(feature = "test-support")]
/// Renders a deterministic init screen into text for scenario assertions.
///
/// # Panics
///
/// Panics if Ratatui cannot create or draw the in-memory test terminal.
#[must_use]
pub fn render_overflow_screen(
    screen: OverflowScreen,
    item_count: usize,
    cursor: usize,
    selected_reviewer_count: usize,
    width: u16,
    height: u16,
) -> String {
    let app = App::for_overflow_test(screen, item_count, cursor, selected_reviewer_count);
    render_test_app(&app, width, height)
}

#[cfg(feature = "test-support")]
/// Renders a filtered candidate list for integration-test assertions.
///
/// # Panics
///
/// Panics if Ratatui cannot create or draw the in-memory test terminal.
#[must_use]
pub fn render_search_results(
    screen: OverflowScreen,
    query: &str,
    item_count: usize,
    width: u16,
    height: u16,
) -> String {
    let mut app = App::for_overflow_test(screen, item_count, 0, 0);
    query.clone_into(&mut app.search_query);
    app.select_first_match();
    render_test_app(&app, width, height)
}

#[cfg(feature = "test-support")]
/// Renders the pending-login state for integration-test assertions.
///
/// # Panics
///
/// Panics if Ratatui cannot create or draw the in-memory test terminal.
#[must_use]
pub fn render_login_dialog(width: u16, height: u16) -> String {
    let mut app = App::new();
    app.flow.advance();
    app.pending_login = true;
    "Signing in and loading candidates...".clone_into(&mut app.message);
    render_test_app(&app, width, height)
}

#[cfg(feature = "test-support")]
/// Renders the exit-confirmation state for integration-test assertions.
///
/// # Panics
///
/// Panics if Ratatui cannot create or draw the in-memory test terminal.
#[must_use]
pub fn render_exit_confirmation(width: u16, height: u16) -> String {
    let mut app = App::new();
    app.confirm_exit = true;
    render_test_app(&app, width, height)
}

#[cfg(feature = "test-support")]
/// Renders a required-field error for integration-test assertions.
///
/// # Panics
///
/// Panics if Ratatui cannot create or draw the in-memory test terminal.
#[must_use]
pub fn render_required_field_error(width: u16, height: u16) -> String {
    let mut app = App::new();
    app.show_error("Crucible URL is required".to_owned());
    render_test_app(&app, width, height)
}

#[cfg(feature = "test-support")]
fn render_test_app(app: &App, width: u16, height: u16) -> String {
    use ratatui::{Terminal, backend::TestBackend};

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create test terminal");
    terminal
        .draw(|frame| app.render(frame))
        .expect("render test frame");
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn run() -> ExitCode {
    let mut terminal = ratatui::init();
    let result = run_app(&mut terminal);
    ratatui::restore();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("init failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_app(terminal: &mut ratatui::DefaultTerminal) -> Result<(), String> {
    let mut app = App::new();
    loop {
        terminal
            .draw(|frame| app.render(frame))
            .map_err(|error| error.to_string())?;
        if app.pending_login {
            if let Err(error) = app.finish_login() {
                app.show_error(error);
            }
            continue;
        }
        let Event::Key(key) = event::read().map_err(|error| error.to_string())? else {
            continue;
        };
        if app.confirm_exit {
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') => return Ok(()),
                KeyCode::Esc | KeyCode::Char('n') => app.confirm_exit = false,
                _ => {}
            }
            continue;
        }
        if app.error.is_some() {
            if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
                app.error = None;
            }
            continue;
        }
        match key.code {
            KeyCode::Char('q') if !app.is_editing() && !app.is_search_editing() => {
                app.confirm_exit = true;
            }
            KeyCode::Esc if app.is_search_editing() => app.cancel_search(),
            KeyCode::Esc if app.has_search() => app.clear_search(),
            KeyCode::Esc if app.is_editing() => app.exit_insert(),
            KeyCode::Esc => app.confirm_exit = true,
            KeyCode::Char('/') if app.is_list_step() && !app.is_search_editing() => {
                app.start_search();
            }
            KeyCode::Enter if app.is_search_editing() => app.finish_search(),
            KeyCode::Backspace if app.is_search_editing() => app.backspace_search(),
            KeyCode::Char(character) if app.is_search_editing() => app.push_search(character),
            KeyCode::Char('n') if app.is_list_step() && app.has_search() => app.next_match(),
            KeyCode::Char('N') if app.is_list_step() && app.has_search() => {
                app.previous_match();
            }
            KeyCode::Up | KeyCode::Char('k') if app.is_list_step() => app.up(),
            KeyCode::Down | KeyCode::Char('j') if app.is_list_step() => app.down(),
            KeyCode::Char('j' | 'k') if app.is_text_step() && !app.is_editing() => {
                app.toggle_field();
            }
            KeyCode::Char('g') if app.is_list_step() => app.first(),
            KeyCode::Char('G') if app.is_list_step() => app.last(),
            KeyCode::Char('u')
                if key.modifiers.contains(event::KeyModifiers::CONTROL) && app.is_list_step() =>
            {
                app.half_up();
            }
            KeyCode::Char('d')
                if key.modifiers.contains(event::KeyModifiers::CONTROL) && app.is_list_step() =>
            {
                app.half_down();
            }
            KeyCode::Char('i') if app.is_text_step() && !app.is_editing() => app.enter_insert(),
            KeyCode::Backspace if app.is_editing() => app.backspace(),
            KeyCode::Backspace => app.back(),
            KeyCode::Char(' ') => app.toggle_reviewer(),
            KeyCode::Left | KeyCode::Char('h') if app.is_reviewer_step() => {
                app.focus_reviewer_candidates();
            }
            KeyCode::Right | KeyCode::Char('l') if app.is_reviewer_step() => {
                app.focus_selected_reviewers();
            }
            KeyCode::Enter if !app.is_editing() => match app.next() {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(error) => app.show_error(error),
            },
            KeyCode::Char(character) if app.is_editing() => app.push(character),
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ReviewerPane {
    Candidates,
    Selected,
}

struct App {
    flow: FormFlow,
    url: String,
    username: String,
    password: String,
    token: String,
    projects: Vec<String>,
    repositories: Vec<String>,
    repository_candidates: Vec<RepositoryCandidate>,
    users: Vec<User>,
    project: usize,
    repository: usize,
    user: usize,
    reviewers: Vec<String>,
    reviewer_pane: ReviewerPane,
    selected_reviewer: usize,
    search_query: String,
    search_editing: bool,
    message: String,
    pending_login: bool,
    confirm_exit: bool,
    error: Option<String>,
}
impl App {
    fn new() -> Self {
        let defaults = crate::legacy_cru::defaults();
        Self {
            flow: FormFlow::new(),
            url: defaults.url,
            username: defaults.user,
            password: String::new(),
            token: String::new(),
            projects: vec![],
            repositories: vec!["(no anchor)".to_owned()],
            repository_candidates: vec![],
            users: vec![],
            project: 0,
            repository: 0,
            user: 0,
            reviewers: vec![],
            reviewer_pane: ReviewerPane::Candidates,
            selected_reviewer: 0,
            search_query: String::new(),
            search_editing: false,
            message: String::new(),
            pending_login: false,
            confirm_exit: false,
            error: None,
        }
    }
    #[cfg(feature = "test-support")]
    fn for_overflow_test(
        screen: OverflowScreen,
        item_count: usize,
        cursor: usize,
        selected_reviewer_count: usize,
    ) -> Self {
        let mut app = Self::new();
        let names = |prefix: &str| {
            (1..=item_count)
                .map(|index| format!("{prefix}-{index:02}"))
                .collect::<Vec<_>>()
        };
        app.projects = names("PROJECT");
        app.repositories = names("repo");
        app.users = (1..=item_count)
            .map(|index| User {
                username: format!("user-{index:02}"),
                display_name: format!("User {index:02}"),
            })
            .collect();
        app.reviewers = app
            .users
            .iter()
            .take(selected_reviewer_count)
            .map(|user| user.username.clone())
            .collect();
        match screen {
            OverflowScreen::Project => {
                app.flow.advance();
                app.flow.advance();
                app.project = cursor.min(item_count.saturating_sub(1));
            }
            OverflowScreen::Repository => {
                for _ in 0..3 {
                    app.flow.advance();
                }
                app.repository = cursor.min(item_count.saturating_sub(1));
            }
            OverflowScreen::ReviewerCandidates | OverflowScreen::SelectedReviewers => {
                for _ in 0..4 {
                    app.flow.advance();
                }
                app.user = cursor.min(item_count.saturating_sub(1));
                if matches!(screen, OverflowScreen::SelectedReviewers) {
                    app.users.clear();
                    app.focus_selected_reviewers();
                }
            }
        }
        app
    }
    fn next(&mut self) -> Result<bool, String> {
        if self.flow.step().index() == 0 {
            if self.url.trim().is_empty() {
                return Err("Crucible URL is required".to_owned());
            }
            self.flow.advance();
        } else if self.flow.step().index() == 1 {
            self.begin_login()?;
        } else if self.flow.step().index() <= 4 {
            if self.flow.step().index() == 2 && self.projects.is_empty() {
                return Err("A Crucible project is required".to_owned());
            }
            self.clear_search();
            self.flow.advance();
        } else {
            self.save()?;
            return Ok(true);
        }
        Ok(false)
    }
    fn begin_login(&mut self) -> Result<(), String> {
        if self.username.trim().is_empty() {
            return Err("Crucible username is required".to_owned());
        }
        if self.password.is_empty() {
            return Err("Crucible password is required".to_owned());
        }
        self.pending_login = true;
        "Signing in and loading candidates...".clone_into(&mut self.message);
        Ok(())
    }
    fn finish_login(&mut self) -> Result<(), String> {
        self.pending_login = false;
        self.login()?;
        self.flow.advance();
        Ok(())
    }
    fn login(&mut self) -> Result<(), String> {
        let candidates = load_candidates(&self.url, &self.username, &self.password)?;
        let origin_url = crate::git_repository::Repository::discover()
            .ok()
            .and_then(|repository| repository.origin_url());
        let detected = origin_url
            .as_deref()
            .and_then(|origin| detected_repository(origin, &candidates.repositories))
            .map(|repository| repository.name.clone());
        self.token = candidates.token;
        self.projects = candidates.projects;
        self.repositories = std::iter::once("(no anchor)".to_owned())
            .chain(
                candidates
                    .repositories
                    .iter()
                    .map(|repository| repository.name.clone()),
            )
            .collect();
        self.repository = detected
            .as_ref()
            .and_then(|name| {
                self.repositories
                    .iter()
                    .position(|candidate| candidate == name)
            })
            .unwrap_or(0);
        self.repository_candidates = candidates.repositories;
        self.users = candidates.reviewers;
        Ok(())
    }
    fn save(&self) -> Result<(), String> {
        ProjectConfig {
            schema_version: crate::project_config::CURRENT_SCHEMA_VERSION,
            crucible: CrucibleConfig {
                url: self.url.clone(),
                token: self.token.clone(),
                project: self.projects[self.project].clone(),
                repository: (self.repository > 0)
                    .then(|| self.repositories[self.repository].clone()),
                repository_location: self
                    .repository
                    .checked_sub(1)
                    .and_then(|index| self.repository_candidates.get(index))
                    .map(|repository| repository.location.clone()),
                reviewers: self.reviewers.clone(),
            },
        }
        .save()
        .map(|_| ())
    }
    fn back(&mut self) {
        self.clear_search();
        self.flow.back();
    }
    fn show_error(&mut self, error: String) {
        self.error = Some(error);
    }
    fn up(&mut self) {
        let indices = self.visible_indices();
        let current = self.selected();
        let position = indices
            .iter()
            .position(|index| *index == current)
            .unwrap_or(0);
        if let Some(previous) = indices.get(position.saturating_sub(1)) {
            self.set_selected(*previous);
        }
    }
    fn first(&mut self) {
        self.select_first_visible();
    }
    fn last(&mut self) {
        if let Some(last) = self.visible_indices().last() {
            self.set_selected(*last);
        }
    }
    fn half_up(&mut self) {
        for _ in 0..5 {
            self.up();
        }
    }
    fn half_down(&mut self) {
        for _ in 0..5 {
            self.down();
        }
    }
    fn is_list_step(&self) -> bool {
        matches!(self.flow.step().index(), 2..=4)
    }
    fn is_reviewer_step(&self) -> bool {
        self.flow.step().index() == 4
    }
    fn is_text_step(&self) -> bool {
        matches!(self.flow.step().index(), 0..=1)
    }
    fn is_editing(&self) -> bool {
        self.flow.input_mode() == InputMode::Insert
    }
    fn enter_insert(&mut self) {
        self.flow.enter_insert();
    }
    fn exit_insert(&mut self) {
        self.flow.exit_insert();
    }
    fn down(&mut self) {
        let indices = self.visible_indices();
        let current = self.selected();
        let position = indices
            .iter()
            .position(|index| *index == current)
            .unwrap_or(0);
        if let Some(next) = indices.get((position + 1).min(indices.len().saturating_sub(1))) {
            self.set_selected(*next);
        }
    }
    fn selected(&self) -> usize {
        match self.flow.step().index() {
            2 => self.project,
            3 => self.repository,
            4 if self.reviewer_pane == ReviewerPane::Selected => self.selected_reviewer,
            _ => self.user,
        }
    }
    fn set_selected(&mut self, selected: usize) {
        match self.flow.step().index() {
            2 => self.project = selected,
            3 => self.repository = selected,
            4 if self.reviewer_pane == ReviewerPane::Selected => self.selected_reviewer = selected,
            _ => self.user = selected,
        }
    }
    fn visible_indices(&self) -> Vec<usize> {
        match self.flow.step().index() {
            2 => (0..self.projects.len()).collect(),
            3 => (0..self.repositories.len()).collect(),
            4 if self.reviewer_pane == ReviewerPane::Selected => {
                (0..self.reviewers.len()).collect()
            }
            4 => (0..self.users.len()).collect(),
            _ => Vec::new(),
        }
    }
    fn search_matches(&self) -> Vec<usize> {
        if !self.has_search() {
            return Vec::new();
        }
        match self.flow.step().index() {
            2 => self
                .projects
                .iter()
                .enumerate()
                .filter_map(|(index, value)| self.matches_search(value).then_some(index))
                .collect(),
            3 => self
                .repositories
                .iter()
                .enumerate()
                .filter_map(|(index, value)| self.matches_search(value).then_some(index))
                .collect(),
            4 if self.reviewer_pane == ReviewerPane::Selected => self
                .reviewers
                .iter()
                .enumerate()
                .filter_map(|(index, reviewer)| self.matches_search(reviewer).then_some(index))
                .collect(),
            4 => self
                .users
                .iter()
                .enumerate()
                .filter_map(|(index, user)| {
                    self.matches_search(&format!("{} {}", user.display_name, user.username))
                        .then_some(index)
                })
                .collect(),
            _ => Vec::new(),
        }
    }
    fn next_match(&mut self) {
        let matches = self.search_matches();
        let current = self.selected();
        let position = matches
            .iter()
            .position(|index| *index == current)
            .unwrap_or(0);
        if let Some(next) = matches.get((position + 1) % matches.len().max(1)) {
            self.set_selected(*next);
        }
    }
    fn previous_match(&mut self) {
        let matches = self.search_matches();
        let current = self.selected();
        let position = matches
            .iter()
            .position(|index| *index == current)
            .unwrap_or(0);
        if let Some(previous) =
            matches.get((position + matches.len().saturating_sub(1)) % matches.len().max(1))
        {
            self.set_selected(*previous);
        }
    }
    fn reviewer_candidate_indices(&self) -> Vec<usize> {
        self.users
            .iter()
            .enumerate()
            .map(|(index, _)| index)
            .collect()
    }
    fn selected_reviewer_indices(&self) -> Vec<usize> {
        self.reviewers
            .iter()
            .enumerate()
            .map(|(index, _)| index)
            .collect()
    }
    fn matches_search(&self, value: &str) -> bool {
        let query = self.search_query.to_lowercase();
        query.is_empty() || value.to_lowercase().contains(&query)
    }
    fn is_search_editing(&self) -> bool {
        self.search_editing
    }
    fn has_search(&self) -> bool {
        !self.search_query.is_empty()
    }
    fn start_search(&mut self) {
        self.search_query.clear();
        self.search_editing = true;
    }
    fn finish_search(&mut self) {
        self.search_editing = false;
        self.select_first_match();
    }
    fn cancel_search(&mut self) {
        self.search_editing = false;
        self.search_query.clear();
    }
    fn push_search(&mut self, character: char) {
        self.search_query.push(character);
    }
    fn backspace_search(&mut self) {
        self.search_query.pop();
    }
    fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_editing = false;
    }
    fn select_first_visible(&mut self) {
        if let Some(first) = self.visible_indices().first() {
            self.set_selected(*first);
        }
    }
    fn select_first_match(&mut self) {
        if let Some(first) = self.search_matches().first() {
            self.set_selected(*first);
        }
    }
    fn toggle_field(&mut self) {
        if self.flow.step().index() == 1 {
            self.flow.toggle_authentication_field();
        }
    }
    fn push(&mut self, character: char) {
        match self.flow.step().index() {
            0 => self.url.push(character),
            1 if self.flow.authentication_field() => self.password.push(character),
            1 => self.username.push(character),
            _ => {}
        }
    }
    fn backspace(&mut self) {
        match self.flow.step().index() {
            0 => {
                self.url.pop();
            }
            1 if self.flow.authentication_field() => {
                self.password.pop();
            }
            1 => {
                self.username.pop();
            }
            _ => {}
        }
    }
    fn toggle_reviewer(&mut self) {
        if self.reviewer_pane == ReviewerPane::Selected {
            if self.selected_reviewer < self.reviewers.len() {
                self.reviewers.remove(self.selected_reviewer);
                self.selected_reviewer = self
                    .selected_reviewer
                    .min(self.reviewers.len().saturating_sub(1));
            }
        } else if self.flow.step().index() == 4
            && let Some(user) = self.users.get(self.user)
        {
            if let Some(index) = self
                .reviewers
                .iter()
                .position(|name| name == &user.username)
            {
                self.reviewers.remove(index);
            } else {
                self.reviewers.push(user.username.clone());
            }
        }
    }
    fn focus_reviewer_candidates(&mut self) {
        self.reviewer_pane = ReviewerPane::Candidates;
        self.select_first_visible();
    }
    fn focus_selected_reviewers(&mut self) {
        self.reviewer_pane = ReviewerPane::Selected;
        if let Some(last) = self.selected_reviewer_indices().last() {
            self.selected_reviewer = *last;
        }
    }
    fn render(&self, frame: &mut Frame) {
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(frame.area());
        let names = [
            "Connection",
            "Authentication",
            "Project",
            "Repository",
            "Reviewers",
            "Confirm",
        ];
        frame.render_widget(
            Paragraph::new(format!(
                "crsu init · Step {} of 6",
                self.flow.step().index() + 1
            ))
            .block(Block::bordered().title("Repository setup")),
            areas[0],
        );
        let body =
            Layout::horizontal([Constraint::Length(22), Constraint::Min(30)]).split(areas[1]);
        let steps = names.iter().enumerate().map(|(index, name)| {
            ListItem::new(format!(
                "{} {name}",
                match index.cmp(&self.flow.step().index()) {
                    std::cmp::Ordering::Less => "✓",
                    std::cmp::Ordering::Equal => "›",
                    std::cmp::Ordering::Greater => "○",
                }
            ))
        });
        frame.render_widget(
            List::new(steps).block(Block::bordered().title("Setup")),
            body[0],
        );
        if self.flow.step().index() == 4 {
            self.render_reviewers(frame, body[1]);
        } else if let Some((title, values, selected)) = self.selection_page() {
            render_scrollable_list(frame, body[1], title, values, Some(selected));
        } else {
            frame.render_widget(
                Paragraph::new(self.page())
                    .block(Block::bordered().title(names[self.flow.step().index()])),
                body[1],
            );
        }
        frame.render_widget(
            Paragraph::new(format!(
                "-- {} --  {}",
                self.input_mode_label(),
                self.help()
            ))
            .block(Block::bordered()),
            areas[2],
        );
        if self.pending_login {
            self.render_login_dialog(frame);
        }
        if self.confirm_exit {
            Self::render_exit_dialog(frame);
        }
        if let Some(error) = &self.error {
            Self::render_error_dialog(frame, error);
        }
    }
    fn render_login_dialog(&self, frame: &mut Frame) {
        let popup = centered_rect(48, 7, frame.area());
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(self.message.as_str())
                .alignment(Alignment::Center)
                .block(Block::bordered().title("Loading")),
            popup,
        );
    }
    fn render_exit_dialog(frame: &mut Frame) {
        let popup = centered_rect(48, 7, frame.area());
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new("Discard this setup?\n\nEnter/y: discard  ·  Esc/n: continue")
                .alignment(Alignment::Center)
                .block(Block::bordered().title("Discard setup?")),
            popup,
        );
    }
    fn render_error_dialog(frame: &mut Frame, error: &str) {
        let popup = centered_rect(56, 7, frame.area());
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(format!("{error}\n\nEnter/Esc: continue editing"))
                .alignment(Alignment::Center)
                .block(Block::bordered().title("Required field")),
            popup,
        );
    }
    fn render_reviewers(&self, frame: &mut Frame, area: Rect) {
        let panes = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(area);
        let visible = self.reviewer_candidate_indices();
        let candidates = self
            .users
            .iter()
            .enumerate()
            .filter(|(index, _)| visible.contains(index))
            .map(|(_, user)| {
                ListItem::new(format!(
                    "{} {} ({})",
                    if self.reviewers.contains(&user.username) {
                        "[x]"
                    } else {
                        "[ ]"
                    },
                    user.display_name,
                    user.username
                ))
            })
            .collect::<Vec<_>>();
        render_scrollable_list(
            frame,
            panes[0],
            "Candidates · Space toggle",
            candidates,
            (self.reviewer_pane == ReviewerPane::Candidates && !self.users.is_empty())
                .then_some(self.user),
        );
        let visible = self.selected_reviewer_indices();
        let selected = self
            .reviewers
            .iter()
            .enumerate()
            .filter(|(index, _)| visible.contains(index))
            .map(|(_, reviewer)| ListItem::new(reviewer.clone()))
            .collect::<Vec<_>>();
        render_scrollable_list(
            frame,
            panes[1],
            format!("Selected ({})", self.reviewers.len()),
            selected,
            (self.reviewer_pane == ReviewerPane::Selected).then_some(self.selected_reviewer),
        );
    }
    fn selection_page(&self) -> Option<(String, Vec<ListItem<'static>>, usize)> {
        match self.flow.step().index() {
            2 => {
                let visible = self.visible_indices();
                Some((
                    self.list_title("Projects"),
                    visible
                        .iter()
                        .map(|index| ListItem::new(self.projects[*index].clone()))
                        .collect(),
                    visible
                        .iter()
                        .position(|index| *index == self.project)
                        .unwrap_or(0),
                ))
            }
            3 => {
                let visible = self.visible_indices();
                Some((
                    self.list_title("Repositories"),
                    visible
                        .iter()
                        .map(|index| ListItem::new(self.repositories[*index].clone()))
                        .collect(),
                    visible
                        .iter()
                        .position(|index| *index == self.repository)
                        .unwrap_or(0),
                ))
            }
            _ => None,
        }
    }
    fn list_title(&self, title: &str) -> String {
        if self.has_search() || self.is_search_editing() {
            format!("{title} · /{}", self.search_query)
        } else {
            title.to_owned()
        }
    }
    fn page(&self) -> String {
        match self.flow.step().index() {
            0 => format!("Crucible URL\n\n> {}", self.url),
            1 => format!(
                "Username{}\n> {}\n\nPassword{}\n> {}",
                if self.flow.authentication_field() {
                    ""
                } else {
                    "  ←"
                },
                self.username,
                if self.flow.authentication_field() {
                    "  ←"
                } else {
                    ""
                },
                "•".repeat(self.password.chars().count())
            ),
            2 => Self::list(&self.projects, self.project),
            3 => Self::list(&self.repositories, self.repository),
            4 => {
                let candidates = self
                    .users
                    .iter()
                    .map(|user| {
                        format!(
                            "{} {} ({})",
                            if self.reviewers.contains(&user.username) {
                                "[x]"
                            } else {
                                "[ ]"
                            },
                            user.display_name,
                            user.username
                        )
                    })
                    .collect::<Vec<_>>();
                format!(
                    "Candidates (your own account is excluded)\n{}\n\nSelected: {}",
                    Self::list(&candidates, self.user),
                    if self.reviewers.is_empty() {
                        "(none)".to_owned()
                    } else {
                        self.reviewers.join(", ")
                    }
                )
            }
            _ => format!(
                "URL: {}\nProject: {}\nRepository: {}\nReviewers: {}",
                self.url,
                self.projects[self.project],
                self.repositories[self.repository],
                self.reviewers.join(", ")
            ),
        }
    }
    fn list(values: &[String], selected: usize) -> String {
        values
            .iter()
            .enumerate()
            .map(|(index, value)| format!("{} {value}", if index == selected { ">" } else { " " }))
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn help(&self) -> String {
        if self.is_search_editing() {
            return format!(
                "/{}  [Enter] confirm  [Esc] cancel  [Backspace] erase",
                self.search_query
            );
        }
        if self.has_search() {
            return format!(
                "/{}  [n/N] next/previous  [Esc] clear  [Enter] select",
                self.search_query
            );
        }
        if self.is_text_step() && self.is_editing() {
            return "[Esc] normal mode  [Backspace] delete  [Enter] submit".to_owned();
        }
        match self.flow.step().index() {
            4 => "[/] filter  [h/l] pane  [j/k] move  [Space] toggle  [Backspace] back",
            1 => "[j/k] field  [i] edit  [Enter] sign in  [Backspace] back",
            0 => "[i] edit  [Enter] next  [Backspace] back",
            _ => "[/] filter  [j/k] move  [g/G] first/last  [Enter] next  [Backspace] back",
        }
        .to_owned()
    }
    fn input_mode_label(&self) -> &'static str {
        match self.flow.input_mode() {
            InputMode::Normal => "NORMAL",
            InputMode::Insert => "INSERT",
        }
    }
}

fn centered_rect(width_percent: u16, height: u16, area: Rect) -> Rect {
    let width = area.width.saturating_mul(width_percent).saturating_div(100);
    let height = height.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn render_scrollable_list(
    frame: &mut Frame,
    area: Rect,
    title: impl Into<ratatui::text::Line<'static>>,
    items: Vec<ListItem<'static>>,
    selected: Option<usize>,
) {
    let mut state = ListState::default();
    state.select(selected.filter(|index| *index < items.len()));
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::bordered().title(title))
            .highlight_symbol("› ")
            .highlight_style(Style::new().reversed()),
        area,
        &mut state,
    );
}

#[cfg(test)]
mod tests {
    use super::App;

    #[test]
    fn confirmed_search_stays_on_the_page_and_navigates_matching_candidates() {
        let mut app = App::new();
        app.flow.advance();
        app.flow.advance();
        app.projects = vec![
            "PROJECT-00".to_owned(),
            "PROJECT-11".to_owned(),
            "PROJECT-21".to_owned(),
        ];

        app.start_search();
        app.push_search('1');
        app.finish_search();
        assert_eq!(app.flow.step().index(), 2);
        assert_eq!(app.project, 1);

        app.next_match();
        assert_eq!(app.project, 2);
        app.previous_match();
        assert_eq!(app.project, 1);
    }
}

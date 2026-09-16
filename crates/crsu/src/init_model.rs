/// The form-flow seam: navigation and validation are independent of terminal rendering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    Connection,
    Authentication,
    Project,
    Repository,
    Reviewers,
    Confirm,
}

/// Whether a text form interprets keys as navigation or text input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputMode {
    Normal,
    Insert,
}

impl Step {
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Connection => 0,
            Self::Authentication => 1,
            Self::Project => 2,
            Self::Repository => 3,
            Self::Reviewers => 4,
            Self::Confirm => 5,
        }
    }

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Connection => Self::Authentication,
            Self::Authentication => Self::Project,
            Self::Project => Self::Repository,
            Self::Repository => Self::Reviewers,
            Self::Reviewers | Self::Confirm => Self::Confirm,
        }
    }

    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Connection | Self::Authentication => Self::Connection,
            Self::Project => Self::Authentication,
            Self::Repository => Self::Project,
            Self::Reviewers => Self::Repository,
            Self::Confirm => Self::Reviewers,
        }
    }
}

#[derive(Debug)]
pub struct FormFlow {
    step: Step,
    authentication_field: bool,
    input_mode: InputMode,
}

/// Stable, terminal-independent description of the current form screen.
#[cfg(feature = "test-support")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Screen {
    pub title: &'static str,
    pub is_multi_select: bool,
    pub help: &'static str,
}

impl Step {
    #[cfg(feature = "test-support")]
    #[must_use]
    pub const fn screen(self) -> Screen {
        match self {
            Self::Connection => Screen {
                title: "Connection",
                is_multi_select: false,
                help: "Enter: next",
            },
            Self::Authentication => Screen {
                title: "Authentication",
                is_multi_select: false,
                help: "Tab: switch field · Enter: sign in",
            },
            Self::Project => Screen {
                title: "Project",
                is_multi_select: false,
                help: "j/k: move · Enter: next",
            },
            Self::Repository => Screen {
                title: "Repository",
                is_multi_select: false,
                help: "j/k: move · Enter: next",
            },
            Self::Reviewers => Screen {
                title: "Reviewers",
                is_multi_select: true,
                help: "j/k: move · Space: toggle · Enter: next",
            },
            Self::Confirm => Screen {
                title: "Confirm",
                is_multi_select: false,
                help: "Enter: save",
            },
        }
    }
}

impl Default for FormFlow {
    fn default() -> Self {
        Self::new()
    }
}

impl FormFlow {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            step: Step::Connection,
            authentication_field: false,
            input_mode: InputMode::Normal,
        }
    }
    #[must_use]
    pub const fn step(&self) -> Step {
        self.step
    }
    #[must_use]
    pub const fn authentication_field(&self) -> bool {
        self.authentication_field
    }
    #[must_use]
    pub const fn input_mode(&self) -> InputMode {
        self.input_mode
    }
    pub fn enter_insert(&mut self) {
        self.input_mode = InputMode::Insert;
    }
    pub fn exit_insert(&mut self) {
        self.input_mode = InputMode::Normal;
    }
    pub fn toggle_authentication_field(&mut self) {
        self.authentication_field = !self.authentication_field;
    }
    pub fn advance(&mut self) {
        self.step = self.step.next();
        self.exit_insert();
    }
    pub fn back(&mut self) {
        self.step = self.step.previous();
        self.exit_insert();
    }
    #[cfg(feature = "test-support")]
    #[must_use]
    pub const fn screen(&self) -> Screen {
        self.step.screen()
    }
}

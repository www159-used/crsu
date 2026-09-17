use std::fmt;

/// Builds the clipboard line shared after a successful review submit.
#[must_use]
pub fn summary(target: &str, title: &str, url: &str) -> String {
    format!("[{target}] {title} {url}")
}

/// Copies text to the system clipboard.
///
/// # Errors
///
/// Returns [`Error`] when the clipboard backend is unavailable or rejects the write.
pub fn copy(text: &str) -> Result<(), Error> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text.to_owned()))
        .map_err(Error)
}

#[derive(Debug)]
pub struct Error(arboard::Error);

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "clipboard unavailable: {}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::summary;

    #[test]
    fn formats_target_title_and_url() {
        assert_eq!(
            summary("main", "fix: title", "http://cru/cru/COMMON-1"),
            "[main] fix: title http://cru/cru/COMMON-1"
        );
    }
}

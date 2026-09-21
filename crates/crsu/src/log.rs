use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LOG_ENV: &str = "CRSU_LOG";

pub struct CommandLog {
    command: String,
    start: Instant,
    outcome: Option<String>,
}

impl CommandLog {
    pub fn start(command: impl Into<String>) -> Self {
        let command = command.into();
        write(&format!("{command} start {}", working_dir()));
        Self {
            command,
            start: Instant::now(),
            outcome: None,
        }
    }

    pub fn time<T>(&self, phase: &str, body: impl FnOnce() -> T) -> T {
        time(&self.command, phase, body)
    }

    pub fn finish(&mut self, outcome: impl Into<String>) {
        self.outcome = Some(outcome.into());
    }

    pub const fn finished(&self) -> bool {
        self.outcome.is_some()
    }
}

impl Drop for CommandLog {
    fn drop(&mut self) {
        let outcome = self.outcome.as_deref().unwrap_or("failed");
        write(&format!(
            "{} end {outcome} {}",
            self.command,
            format_ms(self.start.elapsed())
        ));
    }
}

pub fn time<T>(command: &str, phase: &str, body: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let value = body();
    write(&format!(
        "{command} {phase} {}",
        format_ms(started.elapsed())
    ));
    value
}

fn working_dir() -> String {
    std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "-".to_owned())
}

fn write(message: &str) {
    let Some(path) = log_path() else {
        return;
    };
    append(&path, message);
}

fn log_path() -> Option<PathBuf> {
    match std::env::var(LOG_ENV) {
        Ok(value) if matches!(value.as_str(), "off" | "0" | "false") => None,
        Ok(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => crate::project_config::global_crsu_dir()
            .map(|directory| directory.join("logs/crsu.log")),
    }
}

fn append(path: &Path, message: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    let _ = writeln!(file, "{} {message}", utc_stamp());
}

fn format_ms(elapsed: Duration) -> String {
    format!("{}ms", elapsed.as_millis())
}

fn utc_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    utc_stamp_at(secs)
}

fn utc_stamp_at(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let clock = u32::try_from(secs % 86_400).unwrap_or(0);
    let hour = clock / 3_600;
    let minute = (clock % 3_600) / 60;
    let second = clock % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = u32::try_from(z.rem_euclid(146_097)).unwrap_or(0);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = i32::try_from(yoe).unwrap_or(0) + i32::try_from(era).unwrap_or(0) * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::{append, utc_stamp_at};
    use tempfile::TempDir;

    #[test]
    fn working_dir_is_an_absolute_path() {
        let directory = super::working_dir();
        assert!(directory.starts_with('/'), "{directory}");
    }

    #[test]
    fn formats_unix_epoch_as_utc() {
        assert_eq!(utc_stamp_at(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_stamp_at(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn appends_plain_text_not_json() {
        let directory = TempDir::new().expect("temp log dir");
        let path = directory.path().join("crsu.log");
        append(&path, "diff start /tmp/repo");
        append(&path, "diff git 12ms");
        append(&path, "diff end ok 40ms");
        let text = std::fs::read_to_string(&path).expect("read log");
        assert!(text.contains("diff start /tmp/repo"), "{text}");
        assert!(text.contains("diff git 12ms"), "{text}");
        assert!(text.contains("diff end ok 40ms"), "{text}");
        assert!(!text.contains('{'), "{text}");
    }
}

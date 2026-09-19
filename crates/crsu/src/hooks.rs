use crate::git_repository::Repository;
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Hook stdin contract. Bump only when existing fields change meaning or vanish.
pub const PROTOCOL_VERSION: u32 = 1;

/// Runs global then project `pre-*` hooks before a write. Missing hook is a no-op.
///
/// # Errors
///
/// Returns the hook's stderr (or a short status) when the executable exits non-zero.
pub fn run_pre(repository: &Repository, event: &str, payload: &Value) -> Result<(), String> {
    for scope in [Scope::Global, Scope::Project] {
        match invoke(repository, event, payload, scope) {
            HookOutcome::Missing | HookOutcome::Ok => {}
            HookOutcome::Failed { status, stderr } => {
                return Err(format_failure(event, status, &stderr));
            }
            HookOutcome::Spawn(error) => return Err(format!("{event} hook failed: {error}")),
        }
    }
    Ok(())
}

/// Runs project then global `post-*` hooks after success. Failure is reported, not returned.
pub fn run_post(repository: &Repository, event: &str, payload: &Value) {
    for scope in [Scope::Project, Scope::Global] {
        match invoke(repository, event, payload, scope) {
            HookOutcome::Missing | HookOutcome::Ok => {}
            HookOutcome::Failed { status, stderr } => {
                eprintln!("warning: {}", format_failure(event, status, &stderr));
            }
            HookOutcome::Spawn(error) => eprintln!("warning: {event} hook failed: {error}"),
        }
    }
}

#[derive(Clone, Copy)]
enum Scope {
    Global,
    Project,
}

impl Scope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
        }
    }
}

enum HookOutcome {
    Missing,
    Ok,
    Failed { status: i32, stderr: String },
    Spawn(String),
}

fn envelope(payload: &Value, scope: Scope) -> Value {
    let mut message = payload.clone();
    if let Some(object) = message.as_object_mut() {
        object.insert("version".to_owned(), json!(PROTOCOL_VERSION));
        object.insert("scope".to_owned(), json!(scope.as_str()));
    }
    message
}

fn invoke(repository: &Repository, event: &str, payload: &Value, scope: Scope) -> HookOutcome {
    let Some(path) = hook_path(repository, event, scope) else {
        return HookOutcome::Missing;
    };
    if !is_runnable(&path) {
        return HookOutcome::Missing;
    }
    let mut child = match Command::new(&path)
        .current_dir(repository.work_tree())
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return HookOutcome::Spawn(error.to_string()),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let body = format!("{}\n", envelope(payload, scope));
        if let Err(error) = stdin.write_all(body.as_bytes()) {
            return HookOutcome::Spawn(error.to_string());
        }
    }
    match child.wait_with_output() {
        Ok(output) if output.status.success() => HookOutcome::Ok,
        Ok(output) => HookOutcome::Failed {
            status: output.status.code().unwrap_or(1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        },
        Err(error) => HookOutcome::Spawn(error.to_string()),
    }
}

fn hook_path(repository: &Repository, event: &str, scope: Scope) -> Option<PathBuf> {
    match scope {
        Scope::Global => global_hooks_dir().map(|directory| directory.join(event)),
        Scope::Project => Some(repository.common_dir().join("crsu/hooks").join(event)),
    }
}

fn global_hooks_dir() -> Option<PathBuf> {
    crate::project_config::global_crsu_dir().map(|directory| directory.join("hooks"))
}

fn format_failure(event: &str, status: i32, stderr: &str) -> String {
    if stderr.is_empty() {
        format!("{event} hook exited {status}")
    } else {
        format!("{event} hook exited {status}: {stderr}")
    }
}

fn is_runnable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

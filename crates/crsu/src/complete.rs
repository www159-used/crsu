use crate::crucible::{ReviewComment, review_comments, review_patches};
use crate::git_repository::Repository;
use clap::ValueEnum;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Clone, Copy, Debug, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum Kind {
    Refs,
    ReviewIds,
    CommentIds,
    PatchIds,
    ConfigKeys,
    Shells,
    Jira,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

pub fn script(shell: Shell) -> ExitCode {
    print!("{}", script_body(shell));
    ExitCode::SUCCESS
}

pub fn install(shell: Shell, directory: Option<&Path>) -> ExitCode {
    let path = match install_path(shell, directory) {
        Some(path) => path,
        None => {
            eprintln!("completions failed: cannot determine install directory");
            return ExitCode::FAILURE;
        }
    };
    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        eprintln!("completions failed: {error}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = fs::write(&path, script_body(shell)) {
        eprintln!("completions failed: {error}");
        return ExitCode::FAILURE;
    }
    println!("Wrote {}", path.display());
    match shell {
        Shell::Zsh => {
            if let Some(parent) = path.parent() {
                println!(
                    "Put this directory on fpath in ~/.zshrc, then reload:\n\nfpath=(\"{}\" $fpath)\nautoload -Uz compinit && compinit\n\nrm -f ~/.zcompdump && exec zsh",
                    parent.display()
                );
            }
        }
        Shell::Bash => println!("Reload bash, or: source {}", path.display()),
        Shell::Fish => println!("Fish loads this file on the next start."),
    }
    ExitCode::SUCCESS
}

fn script_body(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => include_str!("../completions/crsu.bash"),
        Shell::Zsh => include_str!("../completions/crsu.zsh"),
        Shell::Fish => include_str!("../completions/crsu.fish"),
    }
}

fn install_path(shell: Shell, directory: Option<&Path>) -> Option<PathBuf> {
    if let Some(directory) = directory {
        return Some(directory.join(file_name(shell)));
    }
    let home = directories::BaseDirs::new()?;
    Some(match shell {
        Shell::Zsh => home.home_dir().join(".zfunc").join("_crsu"),
        Shell::Bash => home
            .home_dir()
            .join(".local/share/bash-completion/completions/crsu"),
        Shell::Fish => home.home_dir().join(".config/fish/completions/crsu.fish"),
    })
}

fn file_name(shell: Shell) -> &'static str {
    match shell {
        Shell::Zsh => "_crsu",
        Shell::Bash => "crsu",
        Shell::Fish => "crsu.fish",
    }
}

pub fn candidates(kind: Kind, prefix: &str) -> ExitCode {
    for value in values(kind) {
        if prefix.is_empty() || value.starts_with(prefix) {
            println!("{value}");
        }
    }
    ExitCode::SUCCESS
}

fn values(kind: Kind) -> Vec<String> {
    match kind {
        Kind::Refs => Repository::discover()
            .map(|repository| repository.complete_refs())
            .unwrap_or_default(),
        Kind::ReviewIds => Repository::discover()
            .map(|repository| repository.complete_review_ids())
            .unwrap_or_default(),
        Kind::CommentIds => comment_ids(),
        Kind::PatchIds => patch_ids(),
        Kind::ConfigKeys => vec![
            "url".to_owned(),
            "project".to_owned(),
            "repository".to_owned(),
        ],
        Kind::Shells => vec!["bash".to_owned(), "zsh".to_owned(), "fish".to_owned()],
        Kind::Jira => Repository::discover()
            .map(|repository| jira_keys(&repository.complete_log_text()))
            .unwrap_or_default(),
    }
}

fn comment_ids() -> Vec<String> {
    let Ok(repository) = Repository::discover() else {
        return Vec::new();
    };
    let Ok(review_id) = repository.review_id_from_head() else {
        return Vec::new();
    };
    let Ok(comments) = review_comments(&review_id) else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    collect_comment_ids(&comments.comments, &mut ids);
    ids
}

fn collect_comment_ids(comments: &[ReviewComment], ids: &mut Vec<String>) {
    for comment in comments {
        if !comment.deleted {
            ids.push(comment.id.clone());
            collect_comment_ids(&comment.replies, ids);
        }
    }
}

fn patch_ids() -> Vec<String> {
    let Ok(repository) = Repository::discover() else {
        return Vec::new();
    };
    let Ok(review_id) = repository.review_id_from_head() else {
        return Vec::new();
    };
    let Ok(patches) = review_patches(&review_id) else {
        return Vec::new();
    };
    patches.patches.into_iter().map(|patch| patch.id).collect()
}

fn jira_keys(text: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_uppercase() {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_uppercase() {
                index += 1;
            }
            if index - start >= 2
                && index < bytes.len()
                && bytes[index] == b'-'
                && index + 1 < bytes.len()
                && bytes[index + 1].is_ascii_digit()
            {
                index += 1;
                while index < bytes.len() && bytes[index].is_ascii_digit() {
                    index += 1;
                }
                let key = text[start..index].to_owned();
                if seen.insert(key.clone()) {
                    keys.push(key);
                }
                continue;
            }
        }
        index += 1;
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::jira_keys;

    #[test]
    fn extracts_jira_keys_from_commit_text() {
        assert_eq!(
            jira_keys("fix: TIC-10733 and COMMON-1 in body"),
            vec!["TIC-10733".to_owned(), "COMMON-1".to_owned()]
        );
        assert_eq!(jira_keys("A-1 is too short"), Vec::<String>::new());
        assert_eq!(jira_keys("not a ticket"), Vec::<String>::new());
    }
}

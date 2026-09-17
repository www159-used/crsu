//! Registers every YAML file as an independently reported test.

#[path = "support/comments.rs"]
mod comments;
#[path = "support/common.rs"]
mod common;
#[path = "support/copy.rs"]
mod copy;
#[path = "support/diff.rs"]
mod diff;
#[path = "support/init.rs"]
mod init;
#[path = "support/land.rs"]
mod land;
#[path = "support/overflow.rs"]
mod overflow;
#[path = "support/patches.rs"]
mod patches;
#[path = "support/ui.rs"]
mod ui;

use libtest_mimic::{Arguments, Trial};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Header {
    name: String,
}

fn main() {
    let arguments = Arguments::from_args();
    let trials = scenario_paths()
        .into_iter()
        .map(|path| {
            let category = category(&path).to_owned();
            let header: Header = serde_yaml::from_str(
                &std::fs::read_to_string(&path).expect("read scenario header"),
            )
            .expect("parse scenario header");
            Trial::test(format!("{category}::{}", header.name), move || {
                run(&category, &path);
                Ok(())
            })
        })
        .collect();
    libtest_mimic::run(&arguments, trials).exit();
}

fn run(category: &str, path: &Path) {
    match category {
        "comments" => comments::run(path),
        "copy" => copy::run(path),
        "diff" => diff::run(path),
        "init" => init::run(path),
        "land" => land::run(path),
        "overflow" => overflow::run(path),
        "patches" => patches::run(path),
        "ui" => ui::run(path),
        _ => panic!("unknown scenario category: {category}"),
    }
}

fn scenario_paths() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios");
    let mut paths = Vec::new();
    for category in std::fs::read_dir(root).expect("read scenario categories") {
        let category = category.expect("read scenario category").path();
        for entry in std::fs::read_dir(category).expect("read scenario directory") {
            let path = entry.expect("read scenario entry").path();
            if path
                .extension()
                .is_some_and(|extension| extension == "yaml")
            {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
}

fn category(path: &Path) -> &str {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .expect("scenario category")
}

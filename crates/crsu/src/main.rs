use clap::Parser;
use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    crsu::run(crsu::Cli::parse_from(compatibility_args(
        std::env::args_os(),
    )))
}

// Clap interprets -na as -n followed by -a; preserve the legacy diff spelling.
fn compatibility_args(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut args: Vec<_> = args.into_iter().collect();
    if !args.get(1).is_some_and(|arg| arg == "diff" || arg == "df") {
        return args;
    }
    let mut attach_value = false;
    for arg in args.iter_mut().skip(2) {
        if attach_value {
            attach_value = false;
            continue;
        }
        if arg == "--" {
            break;
        }
        if arg == "-a" || arg == "--attach" {
            attach_value = true;
        } else if arg == "-na" {
            *arg = "--no-anchor".into();
        }
    }
    args
}

#[cfg(test)]
mod tests {
    use super::compatibility_args;
    use std::ffi::OsString;

    #[test]
    fn legacy_flag_only_rewrites_diff_options() {
        for (input, expected) in [
            (
                vec!["crsu", "diff", "-na", "main"],
                vec!["crsu", "diff", "--no-anchor", "main"],
            ),
            (vec!["crsu", "df", "-na"], vec!["crsu", "df", "--no-anchor"]),
            (
                vec!["crsu", "diff", "--", "-na"],
                vec!["crsu", "diff", "--", "-na"],
            ),
            (
                vec!["crsu", "diff", "--attach", "-na"],
                vec!["crsu", "diff", "--attach", "-na"],
            ),
            (
                vec!["crsu", "comments", "reply", "1", "-m", "-na"],
                vec!["crsu", "comments", "reply", "1", "-m", "-na"],
            ),
        ] {
            let actual = compatibility_args(input.into_iter().map(OsString::from));
            assert_eq!(
                actual,
                expected.into_iter().map(OsString::from).collect::<Vec<_>>()
            );
        }
    }
}

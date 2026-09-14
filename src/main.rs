use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    crsu::run(crsu::Cli::parse())
}

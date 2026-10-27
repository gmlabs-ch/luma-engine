//! `luma`: command-line tools for Luma Engine.

use std::process::ExitCode;

const USAGE: &str = "\
luma - command-line tools for Luma Engine

USAGE:
    luma <COMMAND> [ARGS]

COMMANDS:
    list <game>            List the files contained in a game's packages
    unpack <game> <dir>    Extract every file from a game's packages
    help                   Show this message
    version                Show the version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("help" | "-h" | "--help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("version" | "-V" | "--version") => {
            println!("luma {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(command @ ("list" | "unpack")) => {
            eprintln!("luma: `{command}` is not implemented yet");
            ExitCode::from(2)
        }
        Some(other) => {
            eprintln!("luma: unknown command `{other}`\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

//! ostracon: Ostracon's projects and tasks from the shell, through the same tools as ostracon-mcp.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;

use clap::Parser;
use ostracon_cli::{Cli, Runner};
use ostracon_core::Repository;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let repo = match Repository::open_default() {
        Ok(repo) => repo,
        Err(e) => {
            eprintln!("ostracon: couldn't open the Ostracon database: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Deletions ask first when someone is at the keyboard; scripts pass --yes.
    let mut ask = |question: &str| {
        eprint!("{question} [y/N] ");
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        let _ = std::io::stdin().lock().read_line(&mut answer);
        matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
    };
    let confirm: Option<&mut dyn FnMut(&str) -> bool> = if std::io::stdin().is_terminal() {
        Some(&mut ask)
    } else {
        None
    };
    match Runner::new(&repo).run(cli, confirm) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ostracon: {}", e.message);
            ExitCode::from(e.status)
        }
    }
}

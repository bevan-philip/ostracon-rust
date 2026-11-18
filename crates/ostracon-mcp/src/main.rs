//! ostracon-mcp: Ostracon's tasks over MCP.
//!
//!   ostracon-mcp                 stdio, for clients that launch the server themselves
//!   ostracon-mcp --http [port]   HTTP on 127.0.0.1 (default port 8765), served at /mcp

use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::sync::Arc;

use ostracon_core::Repository;
use ostracon_mcp::{Server, http};

const USAGE: &str = "Usage: ostracon-mcp [--http [port]]

Serves Ostracon's projects and tasks over the Model Context Protocol, without authentication.
By default it speaks MCP over stdio. With --http it listens on http://127.0.0.1:<port>/mcp (default 8765).
Set OSTRACON_DATABASE=/path.sqlite to use a database other than the app's.";

fn log(message: &str) {
    eprintln!("ostracon-mcp: {message}");
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let http_port = match args.iter().position(|a| a == "--http") {
        None => None,
        Some(i) => match args.get(i + 1).map(|p| p.parse::<u16>()) {
            None => Some(8765),
            Some(Ok(port)) => Some(port),
            Some(Err(_)) => {
                log(&format!("invalid port {}", args[i + 1]));
                return ExitCode::from(64);
            }
        },
    };
    if let Some(unknown) = args.iter().find(|a| a.starts_with('-') && *a != "--http") {
        log(&format!("unknown option {unknown}\n\n{USAGE}"));
        return ExitCode::from(64);
    }

    let repo = match Repository::open_default() {
        Ok(repo) => repo,
        Err(e) => {
            log(&format!("couldn't open the Ostracon database: {e}"));
            return ExitCode::FAILURE;
        }
    };
    let server = Arc::new(Server::new(repo));

    if let Some(port) = http_port {
        let ready = |addr| {
            log(&format!(
                "listening on http://{addr}/mcp (no authentication)"
            ))
        };
        if let Err(e) = http::listen(server, port, ready) {
            log(&format!("couldn't listen on port {port}: {e}"));
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }

    // stdio: one JSON-RPC message per line on stdin, replies on stdout, logs on stderr.
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle_bytes(line.as_bytes()) {
            let _ = writeln!(stdout, "{reply}");
            let _ = stdout.flush();
        }
    }
    ExitCode::SUCCESS
}

//! An MCP server for Ostracon's tasks, sharing the app's SQLite database. There is no authentication: anything
//! that can reach a transport can read and change every task.

pub mod http;
pub mod server;

pub use server::Server;

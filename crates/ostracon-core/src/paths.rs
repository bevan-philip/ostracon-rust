//! Where Ostracon keeps its files.

use std::path::PathBuf;

/// The environment variable that points the app, CLI and MCP server at another database.
pub const DATABASE_OVERRIDE: &str = "OSTRACON_DATABASE";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseLocation {
    pub path: PathBuf,
    /// Set from [`DATABASE_OVERRIDE`]. Backups are off while it is, so a test database never overwrites the
    /// real one's backups.
    pub overridden: bool,
}

/// The platform's app-data folder for Ostracon, such as `~/Library/Application Support/Ostracon`.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Ostracon")
}

pub fn database_location() -> DatabaseLocation {
    match std::env::var_os(DATABASE_OVERRIDE).filter(|value| !value.is_empty()) {
        Some(path) => DatabaseLocation {
            path: PathBuf::from(path),
            overridden: true,
        },
        None => DatabaseLocation {
            path: data_dir().join("ostracon.sqlite"),
            overridden: false,
        },
    }
}

/// The app's settings file, beside the database.
pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

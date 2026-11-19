//! The app's own settings, kept in a small JSON file beside the database.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Where backups go, or `None` when they're off.
    pub backup_folder: Option<PathBuf>,
    pub appearance: Appearance,
}

/// Light or dark, or whichever the system uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Settings {
    /// Reads `path`, or the defaults if it is missing or unreadable.
    pub fn load(path: &Path) -> Settings {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Writes atomically, through a temporary file beside `path`.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let scratch = path.with_extension("json.tmp");
        std::fs::write(
            &scratch,
            serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(scratch, path)
    }
}

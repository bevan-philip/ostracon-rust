//! Daily copies of the database in a folder, typically one that OneDrive, iCloud Drive or Dropbox syncs.
//!
//! The database itself stays on local disk, where the app, the CLI and the MCP server can share it safely. Sync
//! services upload a file whenever they notice it change, so a live SQLite database in a synced folder could be
//! uploaded halfway through a write. Each backup is instead written in full outside the folder and then moved
//! in, so all a sync service ever sees is a complete file.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jiff::{SignedDuration, Timestamp, ZonedDateTime};

use crate::repo::{Repository, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    pub path: PathBuf,
    /// When the file was written.
    pub modified: SystemTime,
}

impl Backup {
    /// The file name without `.sqlite`, such as "Ostracon 2026-10-30".
    pub fn title(&self) -> String {
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFolder {
    pub path: PathBuf,
    /// How many daily backups to keep; older ones are deleted after each backup.
    pub keep: usize,
}

impl BackupFolder {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        BackupFolder {
            path: path.into(),
            keep: 2,
        }
    }

    /// The daily backups, newest first.
    pub fn backups(&self) -> Vec<Backup> {
        let Ok(entries) = std::fs::read_dir(&self.path) else {
            return Vec::new();
        };
        let mut backups: Vec<Backup> = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| is_daily_name(&entry.file_name().to_string_lossy()))
            .map(|entry| Backup {
                path: entry.path(),
                modified: entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH),
            })
            .collect();
        backups.sort_by(|a, b| b.path.file_name().cmp(&a.path.file_name()));
        backups
    }

    /// Every restorable file in the folder (daily and one-off backups), newest first.
    pub fn restorable(&self) -> Vec<Backup> {
        let Ok(entries) = std::fs::read_dir(&self.path) else {
            return Vec::new();
        };
        let mut backups: Vec<Backup> = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.starts_with("Ostracon ") && name.ends_with(".sqlite")
            })
            .map(|entry| Backup {
                path: entry.path(),
                modified: entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH),
            })
            .collect();
        backups.sort_by(|a, b| b.modified.cmp(&a.modified));
        backups
    }

    /// Writes today's backup, replacing one from earlier today, then deletes all but the newest `keep`.
    pub fn back_up(&self, repo: &Repository, now: &ZonedDateTime) -> Result<Backup> {
        let backup = self.write(
            repo,
            &format!("Ostracon {}.sqlite", now.strftime("%Y-%m-%d")),
        )?;
        for old in self.backups().into_iter().skip(self.keep) {
            let _ = std::fs::remove_file(old.path);
        }
        Ok(backup)
    }

    /// Writes a one-off backup that is never deleted, such as of the data a restore is about to replace.
    pub fn back_up_before(
        &self,
        repo: &Repository,
        reason: &str,
        now: &ZonedDateTime,
    ) -> Result<Backup> {
        self.write(
            repo,
            &format!(
                "Ostracon before {reason} {}.sqlite",
                now.strftime("%Y-%m-%d %H.%M.%S")
            ),
        )
    }

    fn write(&self, repo: &Repository, name: &str) -> Result<Backup> {
        if !self.path.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("the backup folder {} doesn't exist", self.path.display()),
            )
            .into());
        }
        let destination = self.path.join(name);
        let scratch_dir = scratch_dir()?;
        let written = scratch_dir.join(name);
        let result = (|| -> Result<()> {
            repo.copy_to(&written)?;
            move_into_place(&written, &destination)?;
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&scratch_dir);
        result?;
        let modified = std::fs::metadata(&destination)?.modified()?;
        Ok(Backup {
            path: destination,
            modified,
        })
    }
}

/// `Ostracon YYYY-MM-DD.sqlite`.
fn is_daily_name(name: &str) -> bool {
    let Some(day) = name
        .strip_prefix("Ostracon ")
        .and_then(|rest| rest.strip_suffix(".sqlite"))
    else {
        return false;
    };
    day.len() == 10 && jiff::civil::Date::strptime("%Y-%m-%d", day).is_ok()
}

fn scratch_dir() -> std::io::Result<PathBuf> {
    let base = std::env::temp_dir();
    for attempt in 0..1000u32 {
        let dir = base.join(format!("ostracon-backup-{}-{attempt}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("couldn't make a scratch folder"))
}

/// Renames `from` onto `to`, replacing it. Across volumes, copies to a hidden file beside `to` and renames
/// that, so `to` still only ever appears complete.
fn move_into_place(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let name = to
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let partial = to.with_file_name(format!(".{name}.partial"));
    std::fs::copy(from, &partial)?;
    std::fs::rename(&partial, to).inspect_err(|_| {
        let _ = std::fs::remove_file(&partial);
    })
}

/// When to back up: at most once a [`BackupPolicy::INTERVAL`] while things change, and whenever asked to
/// without throttling (on quit).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackupPolicy {
    pub last_attempt: Option<Timestamp>,
    /// When the newest backup was taken.
    pub last_backup: Option<Timestamp>,
}

impl BackupPolicy {
    pub const INTERVAL: SignedDuration = SignedDuration::from_secs(60);

    /// Whether to back up now, given when the database last changed.
    pub fn is_due(
        &self,
        now: Timestamp,
        last_modified: Option<Timestamp>,
        throttled: bool,
    ) -> bool {
        if throttled
            && self
                .last_attempt
                .is_some_and(|at| now.duration_since(at) < Self::INTERVAL)
        {
            return false;
        }
        match (self.last_backup, last_modified) {
            (Some(backup), Some(modified)) => modified > backup,
            _ => true,
        }
    }
}

pub fn timestamp(time: SystemTime) -> Option<Timestamp> {
    Timestamp::try_from(time).ok()
}

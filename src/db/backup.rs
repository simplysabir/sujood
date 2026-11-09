use anyhow::{Context, Result, anyhow};
use rusqlite::Connection;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use super::migrations::{CURRENT_SCHEMA_VERSION, run_migrations};

fn validate_backup(conn: &Connection) -> Result<()> {
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .context("Checking SQLite integrity")?;
    if integrity != "ok" {
        return Err(anyhow!("Backup failed SQLite integrity check: {integrity}"));
    }

    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(anyhow!(
            "Backup schema version {version} is newer than this Sujood build supports ({CURRENT_SCHEMA_VERSION})"
        ));
    }
    if version == 0 {
        let table_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'
             AND name IN ('prayers', 'dhikr_definitions', 'dhikr_log', 'quran_log')",
            [],
            |row| row.get(0),
        )?;
        if table_count != 4 {
            return Err(anyhow!("Database is not a recognized Sujood schema"));
        }
    }
    Ok(())
}

fn lock_path(live: &Path) -> PathBuf {
    live.with_extension("lock")
}

/// Exclude concurrent application and restore processes from a database.
pub fn acquire_database_lock(live: &Path) -> Result<DatabaseLock> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_path(live))
        .context("Another Sujood process is using the database")?;
    Ok(DatabaseLock {
        path: lock_path(live),
        _file: file,
    })
}

pub struct DatabaseLock {
    path: PathBuf,
    _file: File,
}

impl Drop for DatabaseLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Recover a restore interrupted between staging and replacement.
pub fn recover_interrupted_restore(live: &Path) -> Result<()> {
    let staged = live.with_extension("restore.tmp");
    let previous = live.with_extension("pre-restore.bak");
    if live.exists() {
        if staged.exists() {
            fs::remove_file(staged)?;
        }
    } else if staged.exists() {
        fs::rename(staged, live)?;
    } else if previous.exists() {
        fs::rename(previous, live)?;
    }
    Ok(())
}

/// Create a consistent SQLite backup without modifying the source database.
fn create_backup_unlocked(source: &Path, destination: &Path) -> Result<()> {
    let source = fs::canonicalize(source)
        .with_context(|| format!("Opening source database {:?}", source))?;
    let destination = if destination.exists() {
        fs::canonicalize(destination)?
    } else {
        destination.to_path_buf()
    };
    if source == destination {
        return Err(anyhow!(
            "Backup destination must differ from the live database"
        ));
    }

    let destination_str = destination
        .to_str()
        .ok_or_else(|| anyhow!("Backup destination must be valid UTF-8"))?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Creating backup directory {:?}", parent))?;
    }
    let conn = Connection::open(&source)?;
    validate_backup(&conn)?;
    conn.execute_batch("PRAGMA wal_checkpoint(FULL);")?;
    let escaped = destination_str.replace('\'', "''");
    conn.execute_batch(&format!("VACUUM INTO '{escaped}'"))
        .context("Creating SQLite backup")?;
    let backup = Connection::open(&destination)?;
    validate_backup(&backup)?;
    Ok(())
}

/// Create a consistent SQLite backup while excluding concurrent writers.
pub fn create_backup(source: &Path, destination: &Path) -> Result<()> {
    let _lock = acquire_database_lock(source)?;
    create_backup_unlocked(source, destination)
}

/// Validate and atomically install a staged SQLite backup.
pub fn restore_backup(live: &Path, backup: &Path) -> Result<PathBuf> {
    if !backup.is_file() {
        return Err(anyhow!("Backup file does not exist: {:?}", backup));
    }
    let backup = fs::canonicalize(backup)?;
    let _lock = acquire_database_lock(live)?;
    let validation = Connection::open(&backup)
        .with_context(|| format!("Opening backup database {:?}", backup))?;
    validate_backup(&validation)?;
    drop(validation);

    let parent = live
        .parent()
        .ok_or_else(|| anyhow!("Live database path has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let staged = live.with_extension("restore.tmp");
    if staged == backup {
        return Err(anyhow!(
            "Restore staging path must differ from the backup path"
        ));
    }
    if staged.exists() {
        fs::remove_file(&staged)?;
    }
    fs::copy(&backup, &staged).context("Staging database restore")?;
    let staged_conn = Connection::open(&staged)?;
    validate_backup(&staged_conn)?;
    run_migrations(&staged_conn).context("Migrating staged restore")?;
    drop(staged_conn);

    let previous = live.with_extension("pre-restore.bak");
    if previous.exists() {
        fs::remove_file(&previous)?;
    }
    if live.exists() {
        create_backup_unlocked(live, &previous).context("Preserving current database")?;
        for suffix in ["-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{}", live.display(), suffix));
        }
    }
    if let Err(error) = fs::rename(&staged, live) {
        return Err(error).context("Installing restored database");
    }
    let restored = Connection::open(live)?;
    validate_backup(&restored).context("Verifying restored database")?;
    drop(restored);
    Ok(previous)
}

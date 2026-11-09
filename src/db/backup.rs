use anyhow::{anyhow, Context, Result};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

use super::migrations::{run_migrations, CURRENT_SCHEMA_VERSION};

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
    Ok(())
}

pub fn create_backup(source: &Path, destination: &Path) -> Result<()> {
    let source = fs::canonicalize(source)
        .with_context(|| format!("Opening source database {:?}", source))?;
    let destination = if destination.exists() {
        fs::canonicalize(destination)?
    } else {
        destination.to_path_buf()
    };
    if source == destination {
        return Err(anyhow!("Backup destination must differ from the live database"));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Creating backup directory {:?}", parent))?;
    }
    let conn = Connection::open(&source)?;
    validate_backup(&conn)?;
    let escaped = destination.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!("VACUUM INTO '{escaped}'"))
        .context("Creating SQLite backup")?;
    let backup = Connection::open(&destination)?;
    validate_backup(&backup)?;
    Ok(())
}

pub fn restore_backup(live: &Path, backup: &Path) -> Result<PathBuf> {
    if !backup.is_file() {
        return Err(anyhow!("Backup file does not exist: {:?}", backup));
    }
    let validation = Connection::open(backup)
        .with_context(|| format!("Opening backup database {:?}", backup))?;
    validate_backup(&validation)?;
    run_migrations(&validation).context("Checking backup schema compatibility")?;
    drop(validation);

    let parent = live
        .parent()
        .ok_or_else(|| anyhow!("Live database path has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let staged = live.with_extension("restore.tmp");
    if staged.exists() {
        fs::remove_file(&staged)?;
    }
    fs::copy(backup, &staged).context("Staging database restore")?;
    let staged_conn = Connection::open(&staged)?;
    validate_backup(&staged_conn).context("Verifying staged restore")?;
    drop(staged_conn);

    let previous = live.with_extension("pre-restore.bak");
    if previous.exists() {
        fs::remove_file(&previous)?;
    }
    if live.exists() {
        fs::rename(live, &previous).context("Preserving current database")?;
    }
    if let Err(error) = fs::rename(&staged, live) {
        if live.exists() {
            fs::remove_file(live)?;
        }
        if previous.exists() {
            fs::rename(&previous, live)?;
        }
        return Err(error).context("Installing restored database");
    }
    let restored = Connection::open(live)?;
    validate_backup(&restored).context("Verifying restored database")?;
    drop(restored);
    Ok(previous)
}

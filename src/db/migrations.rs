use anyhow::Result;
use rusqlite::Connection;

pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS prayers (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            prayer_type  TEXT NOT NULL CHECK(prayer_type IN ('fajr','zuhr','asr','maghrib','isha')),
            date         TEXT NOT NULL,
            status       TEXT NOT NULL DEFAULT 'pending'
                         CHECK(status IN ('pending','done','missed')),
            is_qada      INTEGER DEFAULT 0,
            note         TEXT,
            created_at   TEXT DEFAULT (datetime('now')),
            UNIQUE(prayer_type, date, is_qada)
        );

        CREATE TABLE IF NOT EXISTS prayer_times_cache (
            date     TEXT PRIMARY KEY,
            fajr     TEXT,
            sunrise  TEXT,
            zuhr     TEXT,
            asr      TEXT,
            maghrib  TEXT,
            isha     TEXT
        );

        CREATE TABLE IF NOT EXISTS qada_queue (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            prayer_type   TEXT NOT NULL,
            original_date TEXT NOT NULL,
            completed     INTEGER DEFAULT 0,
            completed_at  TEXT
        );

        CREATE TABLE IF NOT EXISTS dhikr_definitions (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            name          TEXT NOT NULL UNIQUE,
            dhikr_type    TEXT NOT NULL CHECK(dhikr_type IN ('checkbox','counter')),
            frequency     TEXT NOT NULL CHECK(frequency IN ('daily','weekly')),
            target_count  INTEGER DEFAULT 1,
            category      TEXT NOT NULL CHECK(category IN ('builtin','custom')),
            sort_order    INTEGER DEFAULT 0,
            active        INTEGER DEFAULT 1
        );

        CREATE TABLE IF NOT EXISTS dhikr_log (
            id        INTEGER PRIMARY KEY AUTOINCREMENT,
            dhikr_id  INTEGER NOT NULL REFERENCES dhikr_definitions(id),
            date      TEXT NOT NULL,
            count     INTEGER DEFAULT 0,
            completed INTEGER DEFAULT 0,
            UNIQUE(dhikr_id, date)
        );

        CREATE TABLE IF NOT EXISTS quran_log (
            id    INTEGER PRIMARY KEY AUTOINCREMENT,
            date  TEXT NOT NULL UNIQUE,
            pages REAL DEFAULT 0,
            note  TEXT
        );

        CREATE TABLE IF NOT EXISTS app_meta (
            key   TEXT PRIMARY KEY,
            value TEXT
        );
    ")?;

    seed_builtins(conn)?;
    Ok(())
}

fn seed_builtins(conn: &Connection) -> Result<()> {
    let builtins = [
        ("Morning Adhkar", "checkbox", "daily", 1, 0),
        ("Evening Adhkar", "checkbox", "daily", 1, 1),
        ("Post-Salah Tasbih", "counter", "daily", 99, 2),
    ];

    for (name, dhikr_type, freq, target, order) in &builtins {
        conn.execute(
            "INSERT OR IGNORE INTO dhikr_definitions
                (name, dhikr_type, frequency, target_count, category, sort_order, active)
             VALUES (?1, ?2, ?3, ?4, 'builtin', ?5, 1)",
            rusqlite::params![name, dhikr_type, freq, target, order],
        )?;
    }
    Ok(())
}

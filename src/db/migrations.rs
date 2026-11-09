use anyhow::{anyhow, bail, Result};
use rusqlite::{Connection, OptionalExtension};

/// SQLite `user_version` used by Sujood's versioned schema migrations.
///
/// Version 0 is the original unversioned schema shipped by the first release.
/// Version 1 adds configuration-aware prayer caching, weekly Adhkar summaries,
/// integrity guards, and supporting indexes without deleting existing records.
/// Version 2 enforces normalized, case-insensitive Dhikr names.
pub const CURRENT_SCHEMA_VERSION: i64 = 2;

const LEGACY_SCHEMA: &str = r#"
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
"#;

const MIGRATION_V1: &str = r#"
    ALTER TABLE prayer_times_cache
        ADD COLUMN config_fingerprint TEXT NOT NULL DEFAULT '';

    CREATE TABLE dhikr_weekly_log (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        dhikr_id   INTEGER NOT NULL REFERENCES dhikr_definitions(id) ON DELETE RESTRICT,
        week_start TEXT NOT NULL
                   CHECK(date(week_start) IS NOT NULL)
                   CHECK(date(week_start) = week_start)
                   CHECK(strftime('%w', week_start) = '1'),
        count      INTEGER NOT NULL DEFAULT 0
                   CHECK(typeof(count) = 'integer' AND count BETWEEN 0 AND 2147483647),
        completed  INTEGER NOT NULL DEFAULT 0
                   CHECK(typeof(completed) = 'integer' AND completed IN (0, 1)),
        UNIQUE(dhikr_id, week_start)
    );

    CREATE INDEX idx_prayers_date_non_qada
        ON prayers(date, is_qada, status);
    CREATE INDEX idx_qada_pending_order
        ON qada_queue(completed, original_date, id);
    CREATE INDEX idx_dhikr_log_date
        ON dhikr_log(date, dhikr_id);
    CREATE INDEX idx_dhikr_weekly_log_period
        ON dhikr_weekly_log(week_start, dhikr_id);

    -- Enforce invariants on legacy tables without rebuilding them or dropping
    -- user records. The original table-level CHECK constraints remain in place.
    CREATE TRIGGER sujood_validate_qada_insert
    BEFORE INSERT ON qada_queue
    WHEN NEW.prayer_type NOT IN ('fajr','zuhr','asr','maghrib','isha')
      OR date(NEW.original_date) IS NULL
      OR date(NEW.original_date) <> NEW.original_date
      OR NEW.completed IS NULL
      OR NEW.completed NOT IN (0, 1)
      OR EXISTS (
          SELECT 1 FROM qada_queue
          WHERE prayer_type = NEW.prayer_type
            AND original_date = NEW.original_date
      )
    BEGIN
        SELECT RAISE(ABORT, 'invalid or duplicate Qada entry');
    END;

    CREATE TRIGGER sujood_validate_qada_update
    BEFORE UPDATE ON qada_queue
    WHEN NEW.prayer_type NOT IN ('fajr','zuhr','asr','maghrib','isha')
      OR date(NEW.original_date) IS NULL
      OR date(NEW.original_date) <> NEW.original_date
      OR NEW.completed IS NULL
      OR NEW.completed NOT IN (0, 1)
      OR (
          (NEW.prayer_type <> OLD.prayer_type OR NEW.original_date <> OLD.original_date)
          AND EXISTS (
              SELECT 1 FROM qada_queue
              WHERE prayer_type = NEW.prayer_type
                AND original_date = NEW.original_date
                AND id <> OLD.id
          )
      )
    BEGIN
        SELECT RAISE(ABORT, 'invalid or duplicate Qada entry');
    END;

    CREATE TRIGGER sujood_validate_prayer_insert
    BEFORE INSERT ON prayers
    WHEN date(NEW.date) IS NULL
      OR date(NEW.date) <> NEW.date
      OR NEW.is_qada IS NULL
      OR NEW.is_qada NOT IN (0, 1)
    BEGIN
        SELECT RAISE(ABORT, 'invalid prayer record');
    END;

    CREATE TRIGGER sujood_validate_prayer_update
    BEFORE UPDATE ON prayers
    WHEN date(NEW.date) IS NULL
      OR date(NEW.date) <> NEW.date
      OR NEW.is_qada IS NULL
      OR NEW.is_qada NOT IN (0, 1)
    BEGIN
        SELECT RAISE(ABORT, 'invalid prayer record');
    END;

    CREATE TRIGGER sujood_validate_dhikr_definition_insert
    BEFORE INSERT ON dhikr_definitions
    WHEN length(trim(NEW.name)) = 0
      OR NEW.target_count IS NULL
      OR typeof(NEW.target_count) <> 'integer'
      OR NEW.target_count <= 0
      OR NEW.target_count > 2147483647
      OR NEW.active IS NULL
      OR NEW.active NOT IN (0, 1)
    BEGIN
        SELECT RAISE(ABORT, 'invalid Dhikr definition');
    END;

    CREATE TRIGGER sujood_validate_dhikr_definition_update
    BEFORE UPDATE ON dhikr_definitions
    WHEN length(trim(NEW.name)) = 0
      OR NEW.target_count IS NULL
      OR typeof(NEW.target_count) <> 'integer'
      OR NEW.target_count <= 0
      OR NEW.target_count > 2147483647
      OR NEW.active IS NULL
      OR NEW.active NOT IN (0, 1)
    BEGIN
        SELECT RAISE(ABORT, 'invalid Dhikr definition');
    END;

    CREATE TRIGGER sujood_validate_dhikr_log_insert
    BEFORE INSERT ON dhikr_log
    WHEN date(NEW.date) IS NULL
      OR date(NEW.date) <> NEW.date
      OR NEW.count IS NULL
      OR typeof(NEW.count) <> 'integer'
      OR NEW.count < 0
      OR NEW.count > 2147483647
      OR NOT EXISTS (
          SELECT 1 FROM dhikr_definitions
          WHERE id = NEW.dhikr_id AND frequency = 'daily'
      )
      OR NEW.completed IS NULL
      OR NEW.completed NOT IN (0, 1)
    BEGIN
        SELECT RAISE(ABORT, 'invalid daily Dhikr log');
    END;

    CREATE TRIGGER sujood_validate_dhikr_log_update
    BEFORE UPDATE ON dhikr_log
    WHEN date(NEW.date) IS NULL
      OR date(NEW.date) <> NEW.date
      OR NEW.count IS NULL
      OR typeof(NEW.count) <> 'integer'
      OR NEW.count < 0
      OR NEW.count > 2147483647
      OR NOT EXISTS (
          SELECT 1 FROM dhikr_definitions
          WHERE id = NEW.dhikr_id AND frequency = 'daily'
      )
      OR NEW.completed IS NULL
      OR NEW.completed NOT IN (0, 1)
    BEGIN
        SELECT RAISE(ABORT, 'invalid daily Dhikr log');
    END;

    CREATE TRIGGER sujood_validate_weekly_dhikr_insert
    BEFORE INSERT ON dhikr_weekly_log
    WHEN NOT EXISTS (
        SELECT 1 FROM dhikr_definitions
        WHERE id = NEW.dhikr_id AND frequency = 'weekly'
    )
    BEGIN
        SELECT RAISE(ABORT, 'weekly log requires a weekly Dhikr definition');
    END;

    CREATE TRIGGER sujood_validate_weekly_dhikr_update
    BEFORE UPDATE ON dhikr_weekly_log
    WHEN NOT EXISTS (
        SELECT 1 FROM dhikr_definitions
        WHERE id = NEW.dhikr_id AND frequency = 'weekly'
    )
    BEGIN
        SELECT RAISE(ABORT, 'weekly log requires a weekly Dhikr definition');
    END;

    CREATE TRIGGER sujood_validate_quran_log_insert
    BEFORE INSERT ON quran_log
    WHEN date(NEW.date) IS NULL
      OR date(NEW.date) <> NEW.date
      OR NEW.pages IS NULL
      OR typeof(NEW.pages) NOT IN ('integer', 'real')
      OR NEW.pages < 0
      OR NEW.pages > 1.7976931348623157e308
    BEGIN
        SELECT RAISE(ABORT, 'invalid Quran log');
    END;

    CREATE TRIGGER sujood_validate_quran_log_update
    BEFORE UPDATE ON quran_log
    WHEN date(NEW.date) IS NULL
      OR date(NEW.date) <> NEW.date
      OR NEW.pages IS NULL
      OR typeof(NEW.pages) NOT IN ('integer', 'real')
      OR NEW.pages < 0
      OR NEW.pages > 1.7976931348623157e308
    BEGIN
        SELECT RAISE(ABORT, 'invalid Quran log');
    END;
"#;

const MIGRATION_V2: &str = r#"
    CREATE UNIQUE INDEX uq_dhikr_definition_normalized_name
        ON dhikr_definitions(lower(trim(name)));
"#;

pub fn run_migrations(conn: &Connection) -> Result<()> {
    // Foreign keys are a per-connection SQLite setting, so enable and verify
    // them whenever the application initializes a connection.
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let foreign_keys_enabled: i64 =
        conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    if foreign_keys_enabled != 1 {
        bail!("SQLite foreign-key enforcement could not be enabled");
    }

    let mut version: i64 =
        conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 0 {
        bail!("Database has an invalid negative schema version: {version}");
    }
    if version > CURRENT_SCHEMA_VERSION {
        return Err(anyhow!(
            "Database schema version {version} is newer than this Sujood build supports ({}). Update Sujood before opening this database.",
            CURRENT_SCHEMA_VERSION
        ));
    }

    // Apply one migration per transaction. A failed migration leaves both its
    // schema/data changes and user_version rolled back, while prior versions
    // remain safely committed.
    while version < CURRENT_SCHEMA_VERSION {
        let next_version = version + 1;
        let tx = conn.unchecked_transaction()?;

        match next_version {
            1 => {
                // Version 0 is the original unversioned schema. CREATE IF NOT
                // EXISTS bootstraps a fresh database and leaves existing tables
                // and personal records untouched.
                tx.execute_batch(LEGACY_SCHEMA)?;
                apply_v1(&tx)?;
                seed_builtins(&tx)?;
            }
            2 => {
                tx.execute_batch(MIGRATION_V2)?;
            }
            _ => bail!("No database migration is available for schema version {next_version}"),
        }

        tx.pragma_update(None, "user_version", next_version)?;
        tx.commit()?;
        version = next_version;
    }

    // Keep startup initialization safe for already-current databases without
    // reactivating built-ins that users have deliberately deactivated.
    seed_missing_builtins(conn)?;
    Ok(())
}

fn apply_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(MIGRATION_V1)?;

    // The old implementation wrote one row per calendar day for weekly
    // definitions. Build Monday-to-Sunday summaries from those rows while
    // leaving every original daily row untouched for historical preservation.
    // Invalid legacy dates are excluded from the summary but are not deleted.
    // Check the existing i32 model boundary before inserting; the migration
    // transaction remains untouched if old activity cannot be represented.
    let oversized_week: Option<(i64, String, i64)> = conn
        .query_row(
            "SELECT d.id,
                    date(
                        l.date,
                        '-' || ((CAST(strftime('%w', l.date) AS INTEGER) + 6) % 7) || ' days'
                    ) AS week_start,
                    SUM(CASE WHEN l.count > 0 THEN l.count ELSE 0 END) AS weekly_count
             FROM dhikr_log AS l
             INNER JOIN dhikr_definitions AS d ON d.id = l.dhikr_id
             WHERE d.frequency = 'weekly'
               AND d.dhikr_type = 'counter'
               AND date(l.date) IS NOT NULL
               AND date(l.date) = l.date
             GROUP BY d.id, week_start
             HAVING SUM(CASE WHEN l.count > 0 THEN l.count ELSE 0 END) > ?1
             LIMIT 1",
            [i64::from(i32::MAX)],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    if let Some((dhikr_id, week_start, count)) = oversized_week {
        bail!(
            "Cannot migrate weekly Dhikr {dhikr_id} for week {week_start}: legacy total {count} exceeds the supported counter range"
        );
    }

    conn.execute_batch(
        r#"
        INSERT INTO dhikr_weekly_log (dhikr_id, week_start, count, completed)
        SELECT
            d.id,
            date(
                l.date,
                '-' || ((CAST(strftime('%w', l.date) AS INTEGER) + 6) % 7) || ' days'
            ) AS week_start,
            CASE
                WHEN d.dhikr_type = 'checkbox'
                    THEN MAX(CASE WHEN l.completed = 1 THEN 1 ELSE 0 END)
                ELSE SUM(CASE WHEN l.count > 0 THEN l.count ELSE 0 END)
            END AS count,
            CASE
                WHEN d.dhikr_type = 'checkbox'
                    THEN MAX(CASE WHEN l.completed = 1 THEN 1 ELSE 0 END)
                WHEN SUM(CASE WHEN l.count > 0 THEN l.count ELSE 0 END) >= d.target_count
                    THEN 1
                ELSE 0
            END AS completed
        FROM dhikr_log AS l
        INNER JOIN dhikr_definitions AS d ON d.id = l.dhikr_id
        WHERE d.frequency = 'weekly'
          AND date(l.date) IS NOT NULL
          AND date(l.date) = l.date
        GROUP BY d.id, week_start;
        "#,
    )?;

    Ok(())
}

fn seed_builtins(conn: &Connection) -> Result<()> {
    let builtins = [
        ("Morning Adhkar", "checkbox", "daily", 1, 0),
        ("Evening Adhkar", "checkbox", "daily", 1, 1),
        ("Post-Salah Tasbih", "counter", "daily", 99, 2),
    ];

    for (name, dhikr_type, frequency, target, order) in &builtins {
        conn.execute(
            "INSERT OR IGNORE INTO dhikr_definitions
                (name, dhikr_type, frequency, target_count, category, sort_order, active)
             VALUES (?1, ?2, ?3, ?4, 'builtin', ?5, 1)",
            rusqlite::params![name, dhikr_type, frequency, target, order],
        )?;
    }
    Ok(())
}

fn seed_missing_builtins(conn: &Connection) -> Result<()> {
    // Deliberately does not reactivate rows that the user has deactivated.
    // Keep the three startup inserts atomic if the database rejects one.
    let tx = conn.unchecked_transaction()?;
    seed_builtins(&tx)?;
    tx.commit()?;
    Ok(())
}

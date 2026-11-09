//! Regression tests for Phase 3: weekly Adhkar, database integrity/migrations,
//! configuration-aware prayer-time caching, and complete statistics ranges.

use chrono::NaiveDate;
use rusqlite::Connection;

use crate::{
    common::in_memory_database,
    db::{
        migrations::{CURRENT_SCHEMA_VERSION, run_migrations},
        repository::{CacheRepo, CachedTimes, DhikrRepo, PrayerRepo, QuranRepo, StatsRepo},
    },
    prayer_times::PrayerCalculator,
};

fn legacy_v0_database() -> Connection {
    let conn = Connection::open_in_memory().expect("open legacy database");
    conn.execute_batch(
        r#"
        CREATE TABLE prayers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prayer_type TEXT NOT NULL CHECK(prayer_type IN ('fajr','zuhr','asr','maghrib','isha')),
            date TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','done','missed')),
            is_qada INTEGER DEFAULT 0,
            note TEXT,
            created_at TEXT DEFAULT (datetime('now')),
            UNIQUE(prayer_type, date, is_qada)
        );

        CREATE TABLE prayer_times_cache (
            date TEXT PRIMARY KEY,
            fajr TEXT, sunrise TEXT, zuhr TEXT, asr TEXT, maghrib TEXT, isha TEXT
        );

        CREATE TABLE qada_queue (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prayer_type TEXT NOT NULL,
            original_date TEXT NOT NULL,
            completed INTEGER DEFAULT 0,
            completed_at TEXT
        );

        CREATE TABLE dhikr_definitions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            dhikr_type TEXT NOT NULL CHECK(dhikr_type IN ('checkbox','counter')),
            frequency TEXT NOT NULL CHECK(frequency IN ('daily','weekly')),
            target_count INTEGER DEFAULT 1,
            category TEXT NOT NULL CHECK(category IN ('builtin','custom')),
            sort_order INTEGER DEFAULT 0,
            active INTEGER DEFAULT 1
        );

        CREATE TABLE dhikr_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            dhikr_id INTEGER NOT NULL REFERENCES dhikr_definitions(id),
            date TEXT NOT NULL,
            count INTEGER DEFAULT 0,
            completed INTEGER DEFAULT 0,
            UNIQUE(dhikr_id, date)
        );

        CREATE TABLE quran_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL UNIQUE,
            pages REAL DEFAULT 0,
            note TEXT
        );

        CREATE TABLE app_meta (key TEXT PRIMARY KEY, value TEXT);
        "#,
    )
    .expect("create original unversioned schema");

    conn.execute(
        "INSERT INTO prayers (prayer_type, date, status, is_qada)
         VALUES ('fajr', '2026-11-06', 'done', 0)",
        [],
    )
    .expect("insert legacy prayer");
    conn.execute(
        "INSERT INTO prayer_times_cache
         (date, fajr, sunrise, zuhr, asr, maghrib, isha)
         VALUES ('2026-11-06', '05:00', '06:15', '12:00', '15:00', '17:45', '19:00')",
        [],
    )
    .expect("insert legacy cache row");
    conn.execute(
        "INSERT INTO qada_queue (prayer_type, original_date, completed)
         VALUES ('asr', '2026-11-05', 0)",
        [],
    )
    .expect("insert legacy Qada row");
    conn.execute(
        "INSERT INTO dhikr_definitions
         (name, dhikr_type, frequency, target_count, category, sort_order, active)
         VALUES ('Weekly Counter', 'counter', 'weekly', 50, 'custom', 10, 1)",
        [],
    )
    .expect("insert legacy weekly counter");
    let counter_id: i64 = conn
        .query_row(
            "SELECT id FROM dhikr_definitions WHERE name = 'Weekly Counter'",
            [],
            |row| row.get(0),
        )
        .expect("read legacy weekly counter id");
    conn.execute(
        "INSERT INTO dhikr_definitions
         (name, dhikr_type, frequency, target_count, category, sort_order, active)
         VALUES ('Weekly Checkbox', 'checkbox', 'weekly', 1, 'custom', 11, 1)",
        [],
    )
    .expect("insert legacy weekly checkbox");
    let checkbox_id: i64 = conn
        .query_row(
            "SELECT id FROM dhikr_definitions WHERE name = 'Weekly Checkbox'",
            [],
            |row| row.get(0),
        )
        .expect("read legacy weekly checkbox id");

    conn.execute(
        "INSERT INTO dhikr_log (dhikr_id, date, count, completed) VALUES (?1, '2026-11-02', 10, 0)",
        [counter_id],
    )
    .expect("insert Monday legacy weekly activity");
    conn.execute(
        "INSERT INTO dhikr_log (dhikr_id, date, count, completed) VALUES (?1, '2026-11-03', 15, 0)",
        [counter_id],
    )
    .expect("insert Tuesday legacy weekly activity");
    conn.execute(
        "INSERT INTO dhikr_log (dhikr_id, date, count, completed) VALUES (?1, '2026-11-04', 1, 1)",
        [checkbox_id],
    )
    .expect("insert legacy checkbox activity");
    conn.execute(
        "INSERT INTO quran_log (date, pages) VALUES ('2026-11-06', 2.5)",
        [],
    )
    .expect("insert legacy Quran total");
    conn.execute(
        "INSERT INTO app_meta (key, value) VALUES ('setup_done', '1')",
        [],
    )
    .expect("insert legacy setup flag");

    conn
}

#[test]
fn v0_migration_preserves_existing_data_and_builds_weekly_summaries() {
    let conn = legacy_v0_database();

    run_migrations(&conn).expect("upgrade unversioned database");

    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read schema version");
    assert_eq!(version, CURRENT_SCHEMA_VERSION);

    let old_cache_fingerprint: String = conn
        .query_row(
            "SELECT config_fingerprint FROM prayer_times_cache WHERE date = '2026-11-06'",
            [],
            |row| row.get(0),
        )
        .expect("legacy cache row receives a safe empty fingerprint");
    assert!(old_cache_fingerprint.is_empty());

    let configured_calculator = PrayerCalculator::new(19.0760, 72.8777, "Karachi", "Hanafi", 330)
        .expect("valid configured calculator");
    assert!(
        CacheRepo::get_times_for_date(
            &conn,
            "2026-11-06",
            &configured_calculator.cache_fingerprint(),
        )
        .expect("check legacy cache identity")
        .is_none(),
        "an old cache row without a configuration identity must be recomputed"
    );

    let old_prayer_status: String = conn
        .query_row(
            "SELECT status FROM prayers WHERE prayer_type = 'fajr' AND date = '2026-11-06'",
            [],
            |row| row.get(0),
        )
        .expect("legacy prayer remains");
    assert_eq!(old_prayer_status, "done");

    let quran_pages: f64 = conn
        .query_row(
            "SELECT pages FROM quran_log WHERE date = '2026-11-06'",
            [],
            |row| row.get(0),
        )
        .expect("legacy Quran row remains");
    assert_eq!(quran_pages, 2.5);

    let qada_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM qada_queue WHERE completed = 0",
            [],
            |row| row.get(0),
        )
        .expect("legacy Qada remains");
    assert_eq!(qada_count, 1);

    let weekly_counter: (String, i64, i64) = conn
        .query_row(
            "SELECT week_start, count, completed
             FROM dhikr_weekly_log
             WHERE dhikr_id = (SELECT id FROM dhikr_definitions WHERE name = 'Weekly Counter')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("weekly counter summary is migrated");
    assert_eq!(weekly_counter, ("2026-11-02".to_string(), 25, 0));

    let weekly_checkbox: (String, i64, i64) = conn
        .query_row(
            "SELECT week_start, count, completed
             FROM dhikr_weekly_log
             WHERE dhikr_id = (SELECT id FROM dhikr_definitions WHERE name = 'Weekly Checkbox')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("weekly checkbox summary is migrated");
    assert_eq!(weekly_checkbox, ("2026-11-02".to_string(), 1, 1));

    let old_daily_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM dhikr_log WHERE dhikr_id = (SELECT id FROM dhikr_definitions WHERE name = 'Weekly Counter')",
            [],
            |row| row.get(0),
        )
        .expect("original daily activity remains available");
    assert_eq!(old_daily_rows, 2);

    let setup_flag: String = conn
        .query_row(
            "SELECT value FROM app_meta WHERE key = 'setup_done'",
            [],
            |row| row.get(0),
        )
        .expect("setup flag remains");
    assert_eq!(setup_flag, "1");

    // Applying the same migration entry point again must not duplicate data.
    run_migrations(&conn).expect("re-running migrations is safe");
    let weekly_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM dhikr_weekly_log", [], |row| {
            row.get(0)
        })
        .expect("count weekly summaries");
    assert_eq!(weekly_rows, 2);
}

#[test]
fn migrations_enable_foreign_keys_and_reject_unknown_future_schema_versions() {
    let conn = in_memory_database();

    let foreign_keys: i64 = conn
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .expect("read foreign key pragma");
    assert_eq!(foreign_keys, 1);

    conn.execute_batch(
        "CREATE TABLE fk_probe (
            dhikr_id INTEGER NOT NULL REFERENCES dhikr_definitions(id)
        );",
    )
    .expect("create foreign-key probe table");
    let orphan_insert = conn.execute("INSERT INTO fk_probe (dhikr_id) VALUES (?1)", [999_999_i64]);
    assert!(matches!(
        orphan_insert,
        Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::ConstraintViolation,
                ..
            },
            _
        ))
    ));

    let future = Connection::open_in_memory().expect("open future-version database");
    future
        .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION + 1)
        .expect("set future schema version");
    assert!(
        run_migrations(&future).is_err(),
        "an older Sujood build must not downgrade an unknown future schema"
    );
    let table_count: i64 = future
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'",
            [],
            |row| row.get(0),
        )
        .expect("inspect untouched future database");
    assert_eq!(table_count, 0);
}

#[test]
fn failed_schema_migration_rolls_back_ddl_and_keeps_user_version_unchanged() {
    let conn = legacy_v0_database();
    // Simulate an unexpected pre-existing object that makes migration v1 fail
    // after its first ALTER TABLE statement. No half-applied schema should remain.
    conn.execute_batch(
        "CREATE TABLE dhikr_weekly_log (
            id INTEGER PRIMARY KEY,
            dhikr_id INTEGER,
            week_start TEXT,
            count INTEGER,
            completed INTEGER
        );",
    )
    .expect("create conflicting pre-existing table");

    assert!(run_migrations(&conn).is_err());

    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read unchanged schema version");
    assert_eq!(version, 0);

    let cache_columns: Vec<String> = {
        let mut statement = conn
            .prepare("PRAGMA table_info(prayer_times_cache)")
            .expect("prepare cache column inspection");
        statement
            .query_map([], |row| row.get(1))
            .expect("query cache columns")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect cache columns")
    };
    assert!(
        !cache_columns
            .iter()
            .any(|column| column == "config_fingerprint"),
        "the failed ALTER TABLE must be rolled back"
    );

    let prayer_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM prayers", [], |row| row.get(0))
        .expect("legacy user records remain");
    assert_eq!(prayer_count, 1);
}

#[test]
fn migration_refuses_weekly_aggregates_that_exceed_the_supported_counter_range() {
    let conn = Connection::open_in_memory().expect("open legacy database");
    conn.execute_batch(
        r#"
        CREATE TABLE dhikr_definitions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            dhikr_type TEXT NOT NULL CHECK(dhikr_type IN ('checkbox','counter')),
            frequency TEXT NOT NULL CHECK(frequency IN ('daily','weekly')),
            target_count INTEGER DEFAULT 1,
            category TEXT NOT NULL CHECK(category IN ('builtin','custom')),
            sort_order INTEGER DEFAULT 0,
            active INTEGER DEFAULT 1
        );
        CREATE TABLE dhikr_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            dhikr_id INTEGER NOT NULL REFERENCES dhikr_definitions(id),
            date TEXT NOT NULL,
            count INTEGER DEFAULT 0,
            completed INTEGER DEFAULT 0,
            UNIQUE(dhikr_id, date)
        );
        CREATE TABLE prayer_times_cache (
            date TEXT PRIMARY KEY,
            fajr TEXT, sunrise TEXT, zuhr TEXT, asr TEXT, maghrib TEXT, isha TEXT
        );
        CREATE TABLE prayers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prayer_type TEXT NOT NULL,
            date TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            is_qada INTEGER DEFAULT 0,
            note TEXT,
            created_at TEXT DEFAULT (datetime('now')),
            UNIQUE(prayer_type, date, is_qada)
        );
        CREATE TABLE qada_queue (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prayer_type TEXT NOT NULL,
            original_date TEXT NOT NULL,
            completed INTEGER DEFAULT 0,
            completed_at TEXT
        );
        CREATE TABLE quran_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL UNIQUE,
            pages REAL DEFAULT 0,
            note TEXT
        );
        CREATE TABLE app_meta (key TEXT PRIMARY KEY, value TEXT);
        INSERT INTO dhikr_definitions(name,dhikr_type,frequency,target_count,category)
        VALUES('Oversized Legacy Week','counter','weekly',100,'custom');
        INSERT INTO dhikr_log(dhikr_id,date,count,completed)
        VALUES(1,'2026-11-02',2000000000,0);
        INSERT INTO dhikr_log(dhikr_id,date,count,completed)
        VALUES(1,'2026-11-03',2000000000,0);
        "#,
    )
    .expect("create legacy schema and oversized weekly activity");

    assert!(run_migrations(&conn).is_err());
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read unchanged schema version");
    assert_eq!(version, 0);

    let cache_columns: Vec<String> = {
        let mut statement = conn
            .prepare("PRAGMA table_info(prayer_times_cache)")
            .expect("prepare cache schema inspection");
        statement
            .query_map([], |row| row.get(1))
            .expect("query cache schema")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect cache column names")
    };
    assert!(
        !cache_columns
            .iter()
            .any(|column| column == "config_fingerprint"),
        "failed migration must roll back DDL"
    );

    let legacy_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM dhikr_log", [], |row| row.get(0))
        .expect("legacy weekly rows remain untouched");
    assert_eq!(legacy_rows, 2);
}

#[test]
fn database_integrity_guards_reject_invalid_values_without_blocking_valid_updates() {
    let conn = in_memory_database();
    let definition = DhikrRepo::find_by_name(&conn, "Morning Adhkar")
        .expect("find built-in definition")
        .expect("built-in exists");

    assert!(DhikrRepo::get_log_for_date(&conn, "2026-11-9").is_err());

    let null_count = conn.execute(
        "INSERT INTO dhikr_log (dhikr_id, date, count, completed)
         VALUES (?1, '2026-11-09', NULL, 0)",
        [definition.id],
    );
    assert!(null_count.is_err(), "NULL counters must be rejected");

    let fractional_count = conn.execute(
        "INSERT INTO dhikr_log (dhikr_id, date, count, completed)
         VALUES (?1, '2026-11-09', 1.5, 0)",
        [definition.id],
    );
    assert!(
        fractional_count.is_err(),
        "fractional repetition counts must be rejected"
    );

    let zero_target = conn.execute(
        "INSERT INTO dhikr_definitions
         (name, dhikr_type, frequency, target_count, category, sort_order, active)
         VALUES ('Invalid zero target', 'counter', 'weekly', 0, 'custom', 50, 1)",
        [],
    );
    assert!(
        zero_target.is_err(),
        "non-positive targets must be rejected"
    );

    DhikrRepo::add_custom(&conn, "Weekly Integrity Probe", "counter", 10, "weekly")
        .expect("create weekly definition for period constraint test");
    let weekly_definition = DhikrRepo::find_by_name(&conn, "Weekly Integrity Probe")
        .expect("find weekly definition")
        .expect("weekly definition exists");
    let invalid_week_start = conn.execute(
        "INSERT INTO dhikr_weekly_log (dhikr_id, week_start, count, completed)
         VALUES (?1, '2026-11-03', 1, 0)",
        [weekly_definition.id],
    );
    assert!(
        invalid_week_start.is_err(),
        "weekly records must start on Monday"
    );

    let daily_definition_in_weekly_table = conn.execute(
        "INSERT INTO dhikr_weekly_log (dhikr_id, week_start, count, completed)
         VALUES (?1, '2026-11-02', 1, 0)",
        [definition.id],
    );
    assert!(
        daily_definition_in_weekly_table.is_err(),
        "daily definitions must not receive weekly summaries"
    );

    let invalid_quran = conn.execute(
        "INSERT INTO quran_log (date, pages) VALUES ('2026-11-09', -0.5)",
        [],
    );
    assert!(
        invalid_quran.is_err(),
        "negative Quran totals must be rejected"
    );

    conn.execute(
        "INSERT INTO qada_queue (prayer_type, original_date, completed)
         VALUES ('asr', '2026-11-07', 0)",
        [],
    )
    .expect("insert first Qada row");
    let duplicate_qada = conn.execute(
        "INSERT INTO qada_queue (prayer_type, original_date, completed)
         VALUES ('asr', '2026-11-07', 0)",
        [],
    );
    assert!(
        duplicate_qada.is_err(),
        "the database must reject duplicate Qada rows"
    );

    conn.execute(
        "INSERT INTO qada_queue (prayer_type, original_date, completed)
         VALUES ('fajr', '2026-11-07', 0)",
        [],
    )
    .expect("insert second Qada row");
    let duplicate_qada_update = conn.execute(
        "UPDATE qada_queue SET prayer_type = 'asr'
         WHERE prayer_type = 'fajr' AND original_date = '2026-11-07'",
        [],
    );
    assert!(
        duplicate_qada_update.is_err(),
        "updates must not create duplicate Qada rows"
    );

    assert!(
        DhikrRepo::upsert_log(&conn, definition.id, "2026-11-09", 1, true).is_ok(),
        "valid existing workflows must remain functional"
    );
}

#[test]
fn weekly_dhikr_progress_accumulates_across_monday_to_sunday_and_resets_next_week() {
    let conn = in_memory_database();
    let (week_start, week_end) =
        DhikrRepo::weekly_period_for_date("2026-11-08").expect("derive Sunday week");
    assert_eq!(week_start, "2026-11-02");
    assert_eq!(week_end, "2026-11-08");

    DhikrRepo::add_custom(&conn, "Weekly Istighfar", "counter", 50, "weekly")
        .expect("create weekly counter");
    let weekly_counter = DhikrRepo::find_by_name(&conn, "Weekly Istighfar")
        .expect("find weekly counter")
        .expect("weekly counter exists");

    DhikrRepo::upsert_log(&conn, weekly_counter.id, "2026-11-02", 10, false)
        .expect("record Monday activity");
    let tuesday_progress = DhikrRepo::get_log_for_date(&conn, "2026-11-03")
        .expect("load Tuesday period progress")
        .into_iter()
        .find(|log| log.dhikr_id == weekly_counter.id)
        .expect("weekly progress carries into Tuesday");
    assert_eq!(tuesday_progress.count, 10);
    assert_eq!(tuesday_progress.date, "2026-11-02");

    DhikrRepo::upsert_log(&conn, weekly_counter.id, "2026-11-03", 25, false)
        .expect("save accumulated weekly counter");
    let sunday_progress = DhikrRepo::get_log_for_date(&conn, "2026-11-08")
        .expect("load Sunday period progress")
        .into_iter()
        .find(|log| log.dhikr_id == weekly_counter.id)
        .expect("weekly progress remains through Sunday");
    assert_eq!(sunday_progress.count, 25);
    assert!(!sunday_progress.completed);

    DhikrRepo::upsert_log(&conn, weekly_counter.id, "2026-11-08", 50, true)
        .expect("complete weekly target");
    let completed_progress = DhikrRepo::get_log_for_date(&conn, "2026-11-04")
        .expect("load completed period")
        .into_iter()
        .find(|log| log.dhikr_id == weekly_counter.id)
        .expect("completed weekly progress is shared");
    assert_eq!(completed_progress.count, 50);
    assert!(completed_progress.completed);

    assert!(
        DhikrRepo::get_log_for_date(&conn, "2026-11-09")
            .expect("load next week")
            .iter()
            .all(|log| log.dhikr_id != weekly_counter.id),
        "a new Monday starts a new weekly period"
    );
}

#[test]
fn weekly_checkbox_is_shared_throughout_the_week_but_daily_dhikr_stays_date_scoped() {
    let conn = in_memory_database();
    DhikrRepo::add_custom(&conn, "Weekly Review", "checkbox", 1, "weekly")
        .expect("create weekly checkbox");
    let weekly = DhikrRepo::find_by_name(&conn, "Weekly Review")
        .expect("find weekly checkbox")
        .expect("weekly checkbox exists");

    let daily = DhikrRepo::find_by_name(&conn, "Morning Adhkar")
        .expect("find daily definition")
        .expect("daily definition exists");

    DhikrRepo::upsert_log(&conn, weekly.id, "2026-11-03", 1, true)
        .expect("complete weekly checkbox");
    DhikrRepo::upsert_log(&conn, daily.id, "2026-11-03", 1, true).expect("complete daily checkbox");

    let later_this_week =
        DhikrRepo::get_log_for_date(&conn, "2026-11-08").expect("load Sunday progress");
    let weekly_log = later_this_week
        .iter()
        .find(|log| log.dhikr_id == weekly.id)
        .expect("weekly checkbox remains completed");
    assert!(weekly_log.completed);

    assert!(
        later_this_week.iter().all(|log| log.dhikr_id != daily.id),
        "daily progress must not leak into another date"
    );

    let next_week =
        DhikrRepo::get_log_for_date(&conn, "2026-11-09").expect("load next week progress");
    assert!(next_week.iter().all(|log| log.dhikr_id != weekly.id));
}

#[test]
fn prayer_cache_is_reused_only_for_an_identical_calculation_configuration() {
    let conn = in_memory_database();
    let base = PrayerCalculator::new(19.0760, 72.8777, "Karachi", "Hanafi", 330)
        .expect("create base calculator");
    let different_location = PrayerCalculator::new(28.6139, 77.2090, "Karachi", "Hanafi", 330)
        .expect("create different-location calculator");
    let different_method = PrayerCalculator::new(19.0760, 72.8777, "Egyptian", "Hanafi", 330)
        .expect("create different-method calculator");
    let different_madhab = PrayerCalculator::new(19.0760, 72.8777, "Karachi", "Shafi", 330)
        .expect("create different-madhab calculator");
    let different_offset = PrayerCalculator::new(19.0760, 72.8777, "Karachi", "Hanafi", 300)
        .expect("create different-offset calculator");

    let base_fingerprint = base.cache_fingerprint();
    for calculator in [
        different_location,
        different_method,
        different_madhab,
        different_offset,
    ] {
        assert_ne!(base_fingerprint, calculator.cache_fingerprint());
    }

    let times = CachedTimes {
        fajr: chrono::NaiveTime::from_hms_opt(5, 0, 0).unwrap(),
        sunrise: chrono::NaiveTime::from_hms_opt(6, 15, 0).unwrap(),
        zuhr: chrono::NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
        asr: chrono::NaiveTime::from_hms_opt(15, 30, 0).unwrap(),
        maghrib: chrono::NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
        isha: chrono::NaiveTime::from_hms_opt(19, 15, 0).unwrap(),
    };
    CacheRepo::store_times(&conn, "2026-11-09", &base_fingerprint, &times)
        .expect("store cache for base configuration");

    assert!(
        CacheRepo::get_times_for_date(&conn, "2026-11-09", &base_fingerprint)
            .expect("read base cache")
            .is_some()
    );
    assert!(
        CacheRepo::get_times_for_date(
            &conn,
            "2026-11-09",
            &PrayerCalculator::new(28.6139, 77.2090, "Karachi", "Hanafi", 330)
                .unwrap()
                .cache_fingerprint(),
        )
        .expect("read cache for different configuration")
        .is_none(),
        "a different configuration must not reuse the cached schedule"
    );

    // The date primary key replaces the old schedule when the new configuration
    // computes that date, keeping one authoritative cache row per calendar date.
    let new_fingerprint = different_location_fingerprint();
    CacheRepo::store_times(&conn, "2026-11-09", &new_fingerprint, &times)
        .expect("replace cache for changed settings");
    assert!(
        CacheRepo::get_times_for_date(&conn, "2026-11-09", &base_fingerprint)
            .expect("read old config cache")
            .is_none()
    );
    assert!(
        CacheRepo::get_times_for_date(&conn, "2026-11-09", &new_fingerprint)
            .expect("read new config cache")
            .is_some()
    );
}

fn different_location_fingerprint() -> String {
    PrayerCalculator::new(28.6139, 77.2090, "Karachi", "Hanafi", 330)
        .expect("create changed calculator")
        .cache_fingerprint()
}

#[test]
fn weekly_quran_totals_reject_invalid_ranges_and_numeric_overflow() {
    let conn = in_memory_database();

    QuranRepo::log_pages(&conn, "2026-11-02", 1.25).expect("log Monday reading");
    QuranRepo::log_pages(&conn, "2026-11-03", 0.75).expect("log Tuesday reading");
    assert_eq!(
        QuranRepo::get_weekly_total(&conn, "2026-11-02", "2026-11-08")
            .expect("sum the rolling date range"),
        2.0
    );

    let invalid_range = QuranRepo::get_weekly_total(&conn, "2026-11-09", "2026-11-02");
    assert!(
        invalid_range.is_err(),
        "reversed Quran ranges must be rejected"
    );

    let overflow_db = in_memory_database();
    let largest_finite = f64::MAX;
    QuranRepo::log_pages(&overflow_db, "2026-11-02", largest_finite)
        .expect("the largest finite daily amount remains valid");
    QuranRepo::log_pages(&overflow_db, "2026-11-03", largest_finite)
        .expect("a second individually valid daily amount remains valid");
    assert!(
        QuranRepo::get_weekly_total(&overflow_db, "2026-11-02", "2026-11-08").is_err(),
        "weekly aggregation must not return infinity"
    );
}

#[test]
fn weekly_statistics_return_every_date_and_streak_semantics_remain_consistent() {
    let conn = in_memory_database();

    for date in ["2026-11-02", "2026-11-03", "2026-11-04"] {
        PrayerRepo::ensure_today_rows(&conn, date).expect("create day rows");
        for prayer in ["fajr", "zuhr", "asr", "maghrib", "isha"] {
            PrayerRepo::mark_status(&conn, prayer, date, "done").expect("complete prayer");
        }
    }

    let before_today_is_complete =
        StatsRepo::calculate_streak(&conn, NaiveDate::from_ymd_opt(2026, 11, 5).unwrap())
            .expect("calculate streak before today's prayers");
    assert_eq!(before_today_is_complete.current, 0);
    assert_eq!(before_today_is_complete.best, 3);

    PrayerRepo::ensure_today_rows(&conn, "2026-11-05").expect("create today's prayers");
    for prayer in ["fajr", "zuhr", "asr", "maghrib", "isha"] {
        PrayerRepo::mark_status(&conn, prayer, "2026-11-05", "done")
            .expect("complete today's prayer");
    }

    // A complete future date is deliberately ignored for a calculation as of Nov 5.
    PrayerRepo::ensure_today_rows(&conn, "2026-11-06").expect("create future rows");
    for prayer in ["fajr", "zuhr", "asr", "maghrib", "isha"] {
        PrayerRepo::mark_status(&conn, prayer, "2026-11-06", "done")
            .expect("complete future prayer");
    }

    let after_today_is_complete =
        StatsRepo::calculate_streak(&conn, NaiveDate::from_ymd_opt(2026, 11, 5).unwrap())
            .expect("calculate current streak");
    assert_eq!(after_today_is_complete.current, 4);
    assert_eq!(after_today_is_complete.best, 4);

    let grid = StatsRepo::get_weekly_grid(&conn, "2026-11-02", "2026-11-08")
        .expect("load complete seven-day range");
    assert_eq!(grid.len(), 7);
    assert_eq!(grid.first().unwrap().date, "2026-11-02");
    assert_eq!(grid.last().unwrap().date, "2026-11-08");

    let missing_day = grid
        .iter()
        .find(|day| day.date == "2026-11-07")
        .expect("missing activity date is represented");
    assert_eq!(missing_day.prayers_done, 0);
    assert_eq!(missing_day.prayers_total, 0);

    let invalid_range = StatsRepo::get_weekly_grid(&conn, "2026-11-09", "2026-11-02");
    assert!(invalid_range.is_err());
}

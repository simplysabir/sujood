use rusqlite::Connection;

use crate::{
    common::in_memory_database,
    db::{
        migrations::run_migrations,
        repository::{DhikrRepo, PrayerRepo, QadaRepo, QuranRepo},
    },
};

#[test]
fn migrations_can_be_run_repeatedly_without_duplicating_builtin_dhikr() {
    let conn = in_memory_database();

    run_migrations(&conn).expect("second migration run should be safe");

    let builtin_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM dhikr_definitions WHERE category = 'builtin'",
            [],
            |row| row.get(0),
        )
        .expect("count built-in dhikr definitions");

    assert_eq!(builtin_count, 3);
}

#[test]
fn prayer_rows_are_created_once_per_prayer_and_date() {
    let conn = in_memory_database();
    let date = "2026-11-09";

    PrayerRepo::ensure_today_rows(&conn, date).expect("create prayer rows");
    PrayerRepo::ensure_today_rows(&conn, date).expect("repeat row creation");

    let prayers = PrayerRepo::get_by_date(&conn, date).expect("load prayer rows");

    assert_eq!(prayers.len(), 5);
    assert!(prayers.iter().all(|prayer| {
        prayer.date == date && prayer.status.as_str() == "pending" && !prayer.is_qada
    }));
}

#[test]
fn quran_pages_accumulate_for_the_same_date_and_remain_separate_between_dates() {
    let conn = in_memory_database();

    QuranRepo::log_pages(&conn, "2026-11-08", 1.25).expect("log first reading");
    QuranRepo::log_pages(&conn, "2026-11-08", 0.5).expect("log second reading");
    QuranRepo::log_pages(&conn, "2026-11-09", 2.0).expect("log next day reading");

    let previous_day = QuranRepo::get_today(&conn, "2026-11-08").expect("read previous day");
    let today = QuranRepo::get_today(&conn, "2026-11-09").expect("read today");

    assert!((previous_day - 1.75).abs() < f64::EPSILON);
    assert!((today - 2.0).abs() < f64::EPSILON);
}

#[test]
fn dhikr_upsert_keeps_one_log_per_definition_and_date() {
    let conn = in_memory_database();
    let definition = DhikrRepo::find_by_name(&conn, "Morning Adhkar")
        .expect("find built-in dhikr")
        .expect("built-in definition exists");

    DhikrRepo::upsert_log(&conn, definition.id, "2026-11-09", 1, false)
        .expect("insert initial log");
    DhikrRepo::upsert_log(&conn, definition.id, "2026-11-09", 2, true)
        .expect("update existing log");

    let logs = DhikrRepo::get_log_for_date(&conn, "2026-11-09").expect("load daily dhikr logs");

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].count, 2);
    assert!(logs[0].completed);
}

#[test]
fn user_tracking_data_survives_database_reopening() {
    let directory = tempfile::tempdir().expect("create temporary directory");
    let database_path = directory.path().join("sujood-test.db");

    {
        let conn = Connection::open(&database_path).expect("open initial database");
        run_migrations(&conn).expect("initialize database");

        PrayerRepo::ensure_today_rows(&conn, "2026-11-09").expect("create prayer rows");
        PrayerRepo::mark_status(&conn, "fajr", "2026-11-09", "done").expect("mark Fajr complete");
        QuranRepo::log_pages(&conn, "2026-11-09", 2.5).expect("log Quran pages");
        QadaRepo::add_entry(&conn, "asr", "2026-11-08").expect("add a Qada record");

        let morning = DhikrRepo::find_by_name(&conn, "Morning Adhkar")
            .expect("find Morning Adhkar")
            .expect("built-in definition exists");
        DhikrRepo::upsert_log(&conn, morning.id, "2026-11-09", 1, true)
            .expect("save Adhkar progress");
    }

    let conn = Connection::open(&database_path).expect("reopen persisted database");
    run_migrations(&conn).expect("migrations remain safe after reopening");

    let prayers = PrayerRepo::get_by_date(&conn, "2026-11-09").expect("reload prayers");
    let fajr = prayers
        .iter()
        .find(|prayer| prayer.prayer_type.as_str() == "fajr")
        .expect("Fajr row exists");
    let pages = QuranRepo::get_today(&conn, "2026-11-09").expect("reload Quran total");
    let qada_count = QadaRepo::count_pending(&conn).expect("reload Qada count");
    let adhkar_logs = DhikrRepo::get_log_for_date(&conn, "2026-11-09").expect("reload Adhkar logs");

    assert_eq!(fajr.status.as_str(), "done");
    assert!((pages - 2.5).abs() < f64::EPSILON);
    assert_eq!(qada_count, 1);
    assert_eq!(adhkar_logs.len(), 1);
    assert!(adhkar_logs[0].completed);
}

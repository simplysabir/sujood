//! Regression tests for correctness fixes implemented in Phase 2.

use chrono::{Duration, FixedOffset, NaiveDate, TimeZone, Utc};

use crate::{
    cli::setup_tui::parse_tz,
    common::in_memory_database,
    config::AppConfig,
    db::repository::{DhikrRepo, PrayerRepo, QadaRepo, QuranRepo},
    prayer_times::PrayerCalculator,
    tui::app::App,
    utils::time::now_from_utc,
};

#[test]
fn calculator_rejects_timezone_offsets_outside_the_supported_range() {
    for offset in [i32::MIN, -12 * 60 - 1, 14 * 60 + 1, 24 * 60, i32::MAX] {
        let result = PrayerCalculator::new(33.6938, 73.0651, "Karachi", "Hanafi", offset);
        assert!(result.is_err(), "offset {offset} should be rejected");
    }
}

#[test]
fn quran_repository_rejects_non_positive_and_non_finite_page_amounts() {
    let conn = in_memory_database();

    for pages in [-1.0, 0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            QuranRepo::log_pages(&conn, "2026-11-09", pages).is_err(),
            "Quran page amount {pages:?} should be rejected"
        );
    }
}

#[test]
fn custom_counter_definitions_reject_non_positive_targets_and_invalid_fields() {
    let conn = in_memory_database();

    for target in [0, -1] {
        assert!(
            DhikrRepo::add_custom(&conn, "Invalid target", "counter", target, "daily").is_err(),
            "counter target {target} should be rejected"
        );
    }

    assert!(DhikrRepo::add_custom(&conn, "   ", "counter", 10, "daily").is_err());
    assert!(DhikrRepo::add_custom(&conn, "Valid name", "unknown", 10, "daily").is_err());
    assert!(DhikrRepo::add_custom(&conn, "Valid name", "counter", 10, "monthly").is_err());
}

#[test]
fn tui_tick_refreshes_daily_state_after_midnight() {
    let conn = in_memory_database();
    let config = AppConfig::default();
    let timezone = FixedOffset::east_opt(config.salah.timezone_offset * 60)
        .expect("default timezone offset is valid");
    let today = NaiveDate::from_ymd_opt(2026, 11, 9).expect("valid date");
    let yesterday = today - Duration::days(1);
    let now = timezone
        .from_local_datetime(&today.and_hms_opt(0, 5, 0).expect("valid time"))
        .single()
        .expect("unambiguous fixed-offset time");

    let mut app = App::new(config).expect("construct app with valid config");
    app.today_str = yesterday.format("%Y-%m-%d").to_string();

    app.tick_at(&conn, now);

    assert_eq!(app.today_str, today.format("%Y-%m-%d").to_string());
    assert_eq!(app.prayers.len(), 5);
    assert_eq!(
        app.status_message.as_deref(),
        Some("New tracking day loaded")
    );
}

#[test]
fn calculator_rejects_non_finite_or_out_of_range_coordinates() {
    let invalid_coordinates = [
        (f64::NAN, 73.0651),
        (f64::INFINITY, 73.0651),
        (91.0, 73.0651),
        (33.6938, f64::NEG_INFINITY),
        (33.6938, 181.0),
    ];

    for (latitude, longitude) in invalid_coordinates {
        let result = PrayerCalculator::new(latitude, longitude, "Karachi", "Hanafi", 300);
        assert!(
            result.is_err(),
            "invalid coordinates ({latitude}, {longitude}) should be rejected"
        );
    }
}

#[test]
fn configured_clock_uses_the_sujood_offset_instead_of_the_host_timezone() {
    let utc = chrono::DateTime::parse_from_rfc3339("2026-01-01T20:00:00Z")
        .expect("valid UTC timestamp")
        .with_timezone(&Utc);

    let configured = now_from_utc(utc, 330).expect("valid configured offset");

    assert_eq!(configured.date_naive().to_string(), "2026-01-02");
    assert_eq!(configured.time().format("%H:%M").to_string(), "01:30");
}

#[test]
fn timezone_setup_parser_rejects_invalid_minute_components_and_out_of_range_offsets() {
    assert_eq!(parse_tz("+5:30").expect("valid offset"), 330);
    assert_eq!(parse_tz("-3:30").expect("valid offset"), -210);
    assert_eq!(parse_tz("+5.5").expect("valid fractional-hour offset"), 330);
    assert_eq!(parse_tz("+5.1").expect("valid whole-minute offset"), 306);
    assert_eq!(parse_tz("+14:00").expect("maximum positive offset"), 840);
    assert_eq!(parse_tz("-12:00").expect("maximum negative offset"), -720);

    for invalid in ["+5:90", "+14:01", "-12:01", "+5:30:00", "abc", ""] {
        assert!(parse_tz(invalid).is_err(), "{invalid:?} should be rejected");
    }
}

#[test]
fn marking_a_prayer_missed_is_transactional_and_repeat_safe() {
    let conn = in_memory_database();
    let date = "2026-11-09";
    PrayerRepo::ensure_today_rows(&conn, date).expect("create daily prayers");

    assert!(PrayerRepo::mark_missed_with_qada(&conn, "fajr", date).expect("first missed mark"));
    assert!(!PrayerRepo::mark_missed_with_qada(&conn, "fajr", date).expect("repeat missed mark"));

    let prayer = PrayerRepo::get_by_date(&conn, date)
        .expect("load prayer rows")
        .into_iter()
        .find(|prayer| prayer.prayer_type.as_str() == "fajr")
        .expect("Fajr row exists");
    assert_eq!(prayer.status.as_str(), "missed");
    assert_eq!(
        QadaRepo::count_pending(&conn).expect("count Qada entries"),
        1
    );
}

#[test]
fn failed_qada_insert_rolls_back_the_prayer_status_change() {
    let conn = in_memory_database();
    let date = "2026-11-09";
    PrayerRepo::ensure_today_rows(&conn, date).expect("create daily prayers");
    conn.execute_batch(
        "CREATE TRIGGER reject_qada_insert BEFORE INSERT ON qada_queue
         BEGIN SELECT RAISE(ABORT, 'simulated qada write failure'); END;",
    )
    .expect("install failure trigger");

    assert!(PrayerRepo::mark_missed_with_qada(&conn, "fajr", date).is_err());

    let prayer = PrayerRepo::get_by_date(&conn, date)
        .expect("load prayer rows")
        .into_iter()
        .find(|prayer| prayer.prayer_type.as_str() == "fajr")
        .expect("Fajr row exists");
    assert_eq!(prayer.status.as_str(), "pending");
    assert_eq!(
        QadaRepo::count_pending(&conn).expect("count Qada entries"),
        0
    );
}

#[test]
fn manual_qada_addition_is_repeat_safe_and_completion_is_atomic() {
    let conn = in_memory_database();
    assert!(QadaRepo::add_entry(&conn, "asr", "2026-11-08").expect("add Qada"));
    assert!(!QadaRepo::add_entry(&conn, "asr", "2026-11-08").expect("repeat Qada add"));
    assert_eq!(QadaRepo::count_pending(&conn).expect("count pending"), 1);
    assert!(QadaRepo::complete_oldest(&conn).expect("complete oldest"));
    assert!(!QadaRepo::complete_oldest(&conn).expect("no pending entry remains"));
    assert_eq!(QadaRepo::count_pending(&conn).expect("count pending"), 0);
}

#[test]
fn repository_rejects_negative_dhikr_counts_and_invalid_tracking_dates() {
    let conn = in_memory_database();
    let definition = DhikrRepo::find_by_name(&conn, "Morning Adhkar")
        .expect("find builtin")
        .expect("builtin definition exists");

    assert!(DhikrRepo::upsert_log(&conn, definition.id, "2026-11-09", -1, false).is_err());
    assert!(DhikrRepo::upsert_log(&conn, definition.id, "not-a-date", 1, true).is_err());
    assert!(QuranRepo::log_pages(&conn, "not-a-date", 1.0).is_err());
    assert!(QadaRepo::add_entry(&conn, "fajr", "not-a-date").is_err());
}

#[test]
fn tui_surfaces_database_write_failures_instead_of_silently_dropping_them() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let conn = in_memory_database();
    let config = AppConfig::default();
    let mut app = App::new(config).expect("construct app with valid config");
    app.load(&conn).expect("load dashboard state");
    conn.execute_batch(
        "CREATE TRIGGER reject_prayer_update BEFORE UPDATE ON prayers
         BEGIN SELECT RAISE(ABORT, 'simulated prayer write failure'); END;",
    )
    .expect("install failure trigger");

    app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE), &conn);

    assert!(app.status_is_error);
    assert!(
        app.status_message
            .as_deref()
            .is_some_and(|message| message.contains("Could not mark prayer as done"))
    );
}

#[test]
fn application_configuration_rejects_invalid_coordinates_timezone_and_quran_target() {
    let mut config = AppConfig::default();
    config.salah.latitude = f64::NAN;
    assert!(config.validate().is_err());

    let mut config = AppConfig::default();
    config.salah.timezone_offset = 15 * 60;
    assert!(config.validate().is_err());

    let mut config = AppConfig::default();
    config.quran.daily_target = f64::INFINITY;
    assert!(config.validate().is_err());

    let mut config = AppConfig::default();
    config.salah.hijri_offset = i32::MAX;
    assert!(config.validate().is_err());
}

#[test]
fn quran_repository_rejects_totals_that_would_overflow() {
    let conn = in_memory_database();
    let date = "2026-11-09";

    QuranRepo::log_pages(&conn, date, f64::MAX).expect("store first finite amount");
    assert!(QuranRepo::log_pages(&conn, date, f64::MAX).is_err());

    let total = QuranRepo::get_today(&conn, date).expect("read unchanged total");
    assert_eq!(total, f64::MAX);
}

#[test]
fn dhikr_increment_validation_rejects_zero_and_negative_values() {
    use crate::utils::validation::validate_dhikr_increment;

    for increment in [0, -1, i32::MIN] {
        assert!(validate_dhikr_increment(increment).is_err());
    }
    assert!(validate_dhikr_increment(1).is_ok());
}

#[test]
fn creating_daily_prayer_rows_is_all_or_nothing() {
    let conn = in_memory_database();
    conn.execute_batch(
        "CREATE TRIGGER reject_isha BEFORE INSERT ON prayers
         WHEN NEW.prayer_type = 'isha'
         BEGIN SELECT RAISE(ABORT, 'simulated prayer row failure'); END;",
    )
    .expect("install insert failure trigger");

    assert!(PrayerRepo::ensure_today_rows(&conn, "2026-11-09").is_err());

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM prayers WHERE date = '2026-11-09' AND is_qada = 0",
            [],
            |row| row.get(0),
        )
        .expect("count created rows");
    assert_eq!(count, 0, "no partial daily prayer set should remain");
}

#[test]
fn failed_prayer_status_change_rolls_back_new_daily_rows() {
    let conn = in_memory_database();
    conn.execute_batch(
        "CREATE TRIGGER reject_prayer_update BEFORE UPDATE ON prayers
         BEGIN SELECT RAISE(ABORT, 'simulated prayer update failure'); END;",
    )
    .expect("install update failure trigger");

    assert!(PrayerRepo::mark_status(&conn, "fajr", "2026-11-09", "done").is_err());

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM prayers WHERE date = '2026-11-09' AND is_qada = 0",
            [],
            |row| row.get(0),
        )
        .expect("count created rows");
    assert_eq!(
        count, 0,
        "status update and row creation must roll back together"
    );
}

#[test]
fn prayer_countdown_rejects_a_clock_with_a_different_offset() {
    let conn = in_memory_database();
    let calculator = PrayerCalculator::new(19.0760, 72.8777, "Karachi", "Hanafi", 330)
        .expect("valid configured calculator");
    let utc = FixedOffset::east_opt(0).expect("valid UTC offset");
    let now = utc
        .with_ymd_and_hms(2026, 11, 9, 12, 0, 0)
        .single()
        .expect("valid UTC instant");

    assert!(calculator.get_next_prayer(&conn, now).is_err());
}

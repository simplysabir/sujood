use std::fs;

use crate::{
    common::in_memory_database,
    db::{backup, repository::DhikrRepo},
    utils::format::{format_duration_secs, format_pages},
};

#[test]
fn dhikr_lifecycle_preserves_logs_and_protects_builtins() {
    let conn = in_memory_database();
    DhikrRepo::add_custom(&conn, "  Istighfar  ", "counter", 100, "daily")
        .expect("create custom Dhikr");
    let definition = DhikrRepo::find_by_name(&conn, "istighfar")
        .expect("find custom Dhikr")
        .expect("custom Dhikr exists");
    DhikrRepo::upsert_log(&conn, definition.id, "2026-11-09", 40, false).expect("save progress");

    DhikrRepo::edit_custom(
        &conn,
        "ISTIGHFAR",
        Some("Astaghfirullah"),
        None,
        Some(200),
        None,
    )
    .expect("edit custom Dhikr");
    let logs = DhikrRepo::get_log_for_date(&conn, "2026-11-09").expect("load progress");
    assert_eq!(logs[0].count, 40);
    assert!(DhikrRepo::add_custom(&conn, " astaghfirullah ", "checkbox", 1, "daily").is_err());
    assert!(DhikrRepo::set_active(&conn, "Morning Adhkar", false).is_err());

    DhikrRepo::set_active(&conn, "Astaghfirullah", false).expect("deactivate custom Dhikr");
    assert!(
        DhikrRepo::find_by_name(&conn, "Astaghfirullah")
            .expect("query inactive Dhikr")
            .is_none()
    );
    DhikrRepo::set_active(&conn, "Astaghfirullah", true).expect("reactivate custom Dhikr");
    DhikrRepo::reorder(&conn, "Astaghfirullah", 0).expect("reorder custom Dhikr");
}

#[test]
fn formatting_preserves_fractional_pages_and_short_durations() {
    assert_eq!(format_pages(0.25), "0.25");
    assert_eq!(format_pages(2.50), "2.5");
    assert_eq!(format_duration_secs(45), "45s");
    assert_eq!(format_duration_secs(80), "1m 20s");
}

#[test]
fn backup_and_restore_preserve_database_records() {
    let source_dir = tempfile::tempdir().expect("create source directory");
    let backup_dir = tempfile::tempdir().expect("create backup directory");
    let source = source_dir.path().join("sujood.db");
    let backup_path = backup_dir.path().join("backup.db");
    let conn = rusqlite::Connection::open(&source).expect("open source");
    crate::db::migrations::run_migrations(&conn).expect("migrate source");
    DhikrRepo::add_custom(&conn, "Backup Probe", "checkbox", 1, "daily")
        .expect("add backup record");
    drop(conn);

    backup::create_backup(&source, &backup_path).expect("create backup");
    assert!(backup_path.is_file());

    let replacement = source_dir.path().join("replacement.db");
    let replacement_conn = rusqlite::Connection::open(&replacement).expect("open replacement");
    crate::db::migrations::run_migrations(&replacement_conn).expect("migrate replacement");
    drop(replacement_conn);
    let previous = backup::restore_backup(&replacement, &backup_path).expect("restore backup");
    assert!(previous.is_file());

    let restored = rusqlite::Connection::open(&replacement).expect("open restored database");
    assert!(
        DhikrRepo::find_by_name(&restored, "backup probe")
            .expect("query restored Dhikr")
            .is_some()
    );
    drop(restored);
    fs::remove_file(previous).expect("clean previous database");
}

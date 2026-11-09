use chrono::{NaiveDate, Timelike};

use crate::prayer_times::PrayerCalculator;

fn seconds_since_midnight(time: chrono::NaiveTime) -> i64 {
    i64::from(time.num_seconds_from_midnight())
}

fn assert_shifted_by_minutes(
    base: chrono::NaiveTime,
    shifted: chrono::NaiveTime,
    offset_minutes: i64,
) {
    let expected = (seconds_since_midnight(base) + offset_minutes * 60).rem_euclid(86_400);
    assert_eq!(seconds_since_midnight(shifted), expected);
}

#[test]
fn prayer_times_are_converted_using_the_configured_fixed_utc_offset() {
    let date = NaiveDate::from_ymd_opt(2026, 1, 15).expect("valid test date");
    let utc = PrayerCalculator::new(19.0760, 72.8777, "Karachi", "Hanafi", 0)
        .expect("valid UTC calculator");
    let india = PrayerCalculator::new(19.0760, 72.8777, "Karachi", "Hanafi", 330)
        .expect("valid UTC+05:30 calculator");

    let utc_times = utc
        .times_for_date(date)
        .expect("calculate UTC prayer times");
    let local_times = india
        .times_for_date(date)
        .expect("calculate local prayer times");

    assert_shifted_by_minutes(utc_times.fajr, local_times.fajr, 330);
    assert_shifted_by_minutes(utc_times.sunrise, local_times.sunrise, 330);
    assert_shifted_by_minutes(utc_times.zuhr, local_times.zuhr, 330);
    assert_shifted_by_minutes(utc_times.asr, local_times.asr, 330);
    assert_shifted_by_minutes(utc_times.maghrib, local_times.maghrib, 330);
    assert_shifted_by_minutes(utc_times.isha, local_times.isha, 330);
}

#[test]
fn calculator_rejects_unknown_calculation_methods_and_madhabs() {
    let unknown_method = PrayerCalculator::new(19.0760, 72.8777, "NotARealMethod", "Hanafi", 300);
    let unknown_madhab = PrayerCalculator::new(19.0760, 72.8777, "Karachi", "NotAMadhab", 300);

    assert!(unknown_method.is_err());
    assert!(unknown_madhab.is_err());
}

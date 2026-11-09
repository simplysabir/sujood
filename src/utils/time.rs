use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};

use crate::utils::validation::validate_timezone_offset_minutes;

/// Return the current instant expressed in Sujood's configured fixed offset.
///
/// The host machine's local timezone is intentionally not consulted. The
/// configured offset is used consistently by the CLI, TUI, prayer countdown,
/// Hijri date and daily database records.
pub fn now_at_offset(offset_minutes: i32) -> Result<DateTime<FixedOffset>> {
    now_from_utc(Utc::now(), offset_minutes)
}

/// Convert a UTC instant to the configured fixed offset.
///
/// Kept separate from `now_at_offset` so timezone boundaries can be tested
/// deterministically without changing the system clock.
pub fn now_from_utc(utc: DateTime<Utc>, offset_minutes: i32) -> Result<DateTime<FixedOffset>> {
    validate_timezone_offset_minutes(offset_minutes)?;
    let offset = FixedOffset::east_opt(offset_minutes * 60)
        .context("Configured timezone offset cannot be represented")?;
    Ok(utc.with_timezone(&offset))
}

pub fn today_at_offset(offset_minutes: i32) -> Result<NaiveDate> {
    Ok(now_at_offset(offset_minutes)?.date_naive())
}

pub fn format_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

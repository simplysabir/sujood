use anyhow::{Result, anyhow};
use chrono::{Datelike, NaiveDate, NaiveTime};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::str::FromStr;

use crate::models::{
    DailyStats, DhikrCategory, DhikrDef, DhikrFrequency, DhikrLog, DhikrType, Prayer, PrayerStatus,
    PrayerType, QadaEntry, Streak,
};
use crate::utils::validation::{validate_dhikr_definition, validate_quran_pages};

// ─── Cached prayer times ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CachedTimes {
    pub fajr: NaiveTime,
    pub sunrise: NaiveTime,
    pub zuhr: NaiveTime,
    pub asr: NaiveTime,
    pub maghrib: NaiveTime,
    pub isha: NaiveTime,
}

fn parse_time(s: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M").map_err(|e| anyhow!("Bad time '{}': {}", s, e))
}

pub struct CacheRepo;

impl CacheRepo {
    /// Retrieve cached times only when they were calculated from the same
    /// coordinates, method, madhab, and configured UTC offset.
    pub fn get_times_for_date(
        conn: &Connection,
        date: &str,
        config_fingerprint: &str,
    ) -> Result<Option<CachedTimes>> {
        let row = conn
            .query_row(
                "SELECT fajr, sunrise, zuhr, asr, maghrib, isha
                 FROM prayer_times_cache
                 WHERE date = ?1 AND config_fingerprint = ?2",
                params![date, config_fingerprint],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;

        match row {
            None => Ok(None),
            Some((fajr, sunrise, zuhr, asr, maghrib, isha)) => Ok(Some(CachedTimes {
                fajr: parse_time(&fajr)?,
                sunrise: parse_time(&sunrise)?,
                zuhr: parse_time(&zuhr)?,
                asr: parse_time(&asr)?,
                maghrib: parse_time(&maghrib)?,
                isha: parse_time(&isha)?,
            })),
        }
    }

    pub fn clear_all(conn: &Connection) -> Result<()> {
        conn.execute("DELETE FROM prayer_times_cache", [])?;
        Ok(())
    }

    pub fn store_times(
        conn: &Connection,
        date: &str,
        config_fingerprint: &str,
        times: &CachedTimes,
    ) -> Result<()> {
        conn.execute(
            "INSERT INTO prayer_times_cache
                (date, fajr, sunrise, zuhr, asr, maghrib, isha, config_fingerprint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(date) DO UPDATE SET
                fajr = excluded.fajr,
                sunrise = excluded.sunrise,
                zuhr = excluded.zuhr,
                asr = excluded.asr,
                maghrib = excluded.maghrib,
                isha = excluded.isha,
                config_fingerprint = excluded.config_fingerprint",
            params![
                date,
                times.fajr.format("%H:%M").to_string(),
                times.sunrise.format("%H:%M").to_string(),
                times.zuhr.format("%H:%M").to_string(),
                times.asr.format("%H:%M").to_string(),
                times.maghrib.format("%H:%M").to_string(),
                times.isha.format("%H:%M").to_string(),
                config_fingerprint,
            ],
        )?;
        Ok(())
    }
}

// ─── Prayer repo ─────────────────────────────────────────────────────────────

pub struct PrayerRepo;

impl PrayerRepo {
    /// Ensure a row exists for each prayer type for the given date (status='pending').
    /// The five inserts commit together so a failed insert cannot leave a partial day.
    pub fn ensure_today_rows(conn: &Connection, date: &str) -> Result<()> {
        validate_tracking_date(date)?;
        let tx = conn.unchecked_transaction()?;
        Self::ensure_today_rows_in_transaction(&tx, date)?;
        tx.commit()?;
        Ok(())
    }

    fn ensure_today_rows_in_transaction(conn: &Connection, date: &str) -> Result<()> {
        for prayer_type in PrayerType::all() {
            conn.execute(
                "INSERT OR IGNORE INTO prayers (prayer_type, date, status, is_qada)
                 VALUES (?1, ?2, 'pending', 0)",
                params![prayer_type.as_str(), date],
            )?;
        }
        Ok(())
    }

    pub fn get_by_date(conn: &Connection, date: &str) -> Result<Vec<Prayer>> {
        let mut stmt = conn.prepare(
            "SELECT id, prayer_type, date, status, is_qada, note
             FROM prayers WHERE date = ?1 AND is_qada = 0
             ORDER BY CASE prayer_type
               WHEN 'fajr' THEN 1 WHEN 'zuhr' THEN 2 WHEN 'asr' THEN 3
               WHEN 'maghrib' THEN 4 WHEN 'isha' THEN 5 END",
        )?;

        let prayers = stmt.query_map(params![date], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i32>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })?;

        let mut result = Vec::new();
        for p in prayers {
            let (id, prayer_type, date, status, is_qada, note) = p?;
            result.push(Prayer {
                id: Some(id),
                prayer_type: PrayerType::from_str(&prayer_type)
                    .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?,
                date,
                status: PrayerStatus::from_str(&status)
                    .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?,
                is_qada: is_qada != 0,
                note,
                time: None,
            });
        }
        Ok(result)
    }

    pub fn mark_status(
        conn: &Connection,
        prayer_type: &str,
        date: &str,
        status: &str,
    ) -> Result<()> {
        PrayerType::from_str(prayer_type)
            .map_err(|_| anyhow!("Unknown prayer type '{prayer_type}'"))?;
        if !matches!(status, "pending" | "done" | "missed") {
            return Err(anyhow!("Unsupported prayer status '{status}'"));
        }
        validate_tracking_date(date)?;

        let tx = conn.unchecked_transaction()?;
        Self::ensure_today_rows_in_transaction(&tx, date)?;
        let changed = tx.execute(
            "UPDATE prayers SET status = ?1 WHERE prayer_type = ?2 AND date = ?3 AND is_qada = 0",
            params![status, prayer_type, date],
        )?;
        if changed != 1 {
            return Err(anyhow!(
                "Expected one {prayer_type} prayer record for {date}, but updated {changed}"
            ));
        }
        tx.commit()?;
        Ok(())
    }

    /// Mark a prayer missed and add its Qada record as one repeat-safe transaction.
    /// Repeating the same operation for the same prayer/date never creates a duplicate.
    pub fn mark_missed_with_qada(conn: &Connection, prayer_type: &str, date: &str) -> Result<bool> {
        PrayerType::from_str(prayer_type)
            .map_err(|_| anyhow!("Unknown prayer type '{prayer_type}'"))?;
        validate_tracking_date(date)?;

        let tx = conn.unchecked_transaction()?;
        Self::ensure_today_rows_in_transaction(&tx, date)?;
        let changed = tx.execute(
            "UPDATE prayers SET status = 'missed'
             WHERE prayer_type = ?1 AND date = ?2 AND is_qada = 0",
            params![prayer_type, date],
        )?;
        if changed != 1 {
            return Err(anyhow!(
                "Expected one {prayer_type} prayer record for {date}, but updated {changed}"
            ));
        }

        let inserted = tx.execute(
            "INSERT INTO qada_queue (prayer_type, original_date, completed)
             SELECT ?1, ?2, 0
             WHERE NOT EXISTS (
                 SELECT 1 FROM qada_queue WHERE prayer_type = ?1 AND original_date = ?2
             )",
            params![prayer_type, date],
        )?;
        tx.commit()?;
        Ok(inserted == 1)
    }

    pub fn get_date_range(conn: &Connection, start: &str, end: &str) -> Result<Vec<Prayer>> {
        let mut stmt = conn.prepare(
            "SELECT id, prayer_type, date, status, is_qada, note
             FROM prayers WHERE date >= ?1 AND date <= ?2 AND is_qada = 0
             ORDER BY date, CASE prayer_type
               WHEN 'fajr' THEN 1 WHEN 'zuhr' THEN 2 WHEN 'asr' THEN 3
               WHEN 'maghrib' THEN 4 WHEN 'isha' THEN 5 END",
        )?;

        let rows = stmt.query_map(params![start, end], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i32>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })?;

        let mut result = Vec::new();
        for r in rows {
            let (id, prayer_type, date, status, is_qada, note) = r?;
            result.push(Prayer {
                id: Some(id),
                prayer_type: PrayerType::from_str(&prayer_type)
                    .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?,
                date,
                status: PrayerStatus::from_str(&status)
                    .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?,
                is_qada: is_qada != 0,
                note,
                time: None,
            });
        }
        Ok(result)
    }
}

// ─── Dhikr repo ──────────────────────────────────────────────────────────────

pub struct DhikrRepo;

impl DhikrRepo {
    /// Return active definitions in their configured display order.
    pub fn get_active_definitions(conn: &Connection) -> Result<Vec<DhikrDef>> {
        Self::get_definitions(conn, Some(true))
    }

    /// Return all definitions, including inactive definitions.
    pub fn get_all_definitions(conn: &Connection) -> Result<Vec<DhikrDef>> {
        Self::get_definitions(conn, None)
    }

    fn get_definitions(conn: &Connection, active: Option<bool>) -> Result<Vec<DhikrDef>> {
        let query = match active {
            Some(_) => {
                "SELECT id, name, dhikr_type, frequency, target_count, category, sort_order, active
                 FROM dhikr_definitions WHERE active = ?1 ORDER BY sort_order, id"
            }
            None => {
                "SELECT id, name, dhikr_type, frequency, target_count, category, sort_order, active
                 FROM dhikr_definitions ORDER BY sort_order, id"
            }
        };
        let mut stmt = conn.prepare(query)?;

        let map_row = |row: &Row<'_>| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i32>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i32>(6)?,
                row.get::<_, i32>(7)?,
            ))
        };
        let rows = match active {
            Some(value) => stmt.query_map(params![value as i32], map_row)?,
            None => stmt.query_map([], map_row)?,
        };

        let mut result = Vec::new();
        for row in rows {
            let (id, name, dhikr_type, frequency, target_count, category, sort_order, active) =
                row?;
            let dhikr_type = DhikrType::from_str(&dhikr_type)
                .map_err(|error| anyhow!("Invalid Dhikr type for '{name}': {error}"))?;
            let frequency = DhikrFrequency::from_str(&frequency)
                .map_err(|error| anyhow!("Invalid Dhikr frequency for '{name}': {error}"))?;
            let category = match category.as_str() {
                "builtin" => DhikrCategory::Builtin,
                "custom" => DhikrCategory::Custom,
                _ => return Err(anyhow!("Invalid Dhikr category for '{name}': {category}")),
            };

            result.push(DhikrDef {
                id,
                name,
                dhikr_type,
                frequency,
                target_count,
                category,
                sort_order,
                active: active != 0,
            });
        }
        Ok(result)
    }

    /// Return the Monday-to-Sunday period containing `date`.
    /// Weekly goals use the configured-local calendar date supplied by the CLI
    /// or TUI, not the computer's independent timezone.
    pub fn weekly_period_for_date(date: &str) -> Result<(String, String)> {
        let parsed = parse_tracking_date(date)?;
        let days_since_monday = i64::from(parsed.weekday().num_days_from_monday());
        let start = parsed
            .checked_sub_signed(chrono::Duration::days(days_since_monday))
            .ok_or_else(|| anyhow!("Could not determine the weekly period for '{date}'"))?;
        let end = start
            .checked_add_signed(chrono::Duration::days(6))
            .ok_or_else(|| anyhow!("Could not determine the weekly period end for '{date}'"))?;
        Ok((
            start.format("%Y-%m-%d").to_string(),
            end.format("%Y-%m-%d").to_string(),
        ))
    }

    /// Load the progress relevant to `date` for every active frequency.
    /// Daily definitions read from `dhikr_log`; weekly definitions read from
    /// their Monday-anchored aggregate in `dhikr_weekly_log`.
    pub fn get_log_for_date(conn: &Connection, date: &str) -> Result<Vec<DhikrLog>> {
        validate_tracking_date(date)?;
        let (week_start, _) = Self::weekly_period_for_date(date)?;
        let mut result = Vec::new();

        {
            let mut stmt = conn.prepare(
                "SELECT l.id, l.dhikr_id, l.date, l.count, l.completed
                 FROM dhikr_log AS l
                 INNER JOIN dhikr_definitions AS d ON d.id = l.dhikr_id
                 WHERE l.date = ?1 AND d.frequency = 'daily' AND d.active = 1
                 ORDER BY l.dhikr_id",
            )?;
            let rows = stmt.query_map(params![date], |row| {
                Ok(DhikrLog {
                    id: Some(row.get::<_, i64>(0)?),
                    dhikr_id: row.get::<_, i64>(1)?,
                    date: row.get::<_, String>(2)?,
                    count: row.get::<_, i32>(3)?,
                    completed: row.get::<_, i32>(4)? != 0,
                })
            })?;
            result.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
        }

        {
            let mut stmt = conn.prepare(
                "SELECT l.id, l.dhikr_id, l.week_start, l.count, l.completed
                 FROM dhikr_weekly_log AS l
                 INNER JOIN dhikr_definitions AS d ON d.id = l.dhikr_id
                 WHERE l.week_start = ?1 AND d.frequency = 'weekly' AND d.active = 1
                 ORDER BY l.dhikr_id",
            )?;
            let rows = stmt.query_map(params![week_start], |row| {
                Ok(DhikrLog {
                    id: Some(row.get::<_, i64>(0)?),
                    dhikr_id: row.get::<_, i64>(1)?,
                    date: row.get::<_, String>(2)?,
                    count: row.get::<_, i32>(3)?,
                    completed: row.get::<_, i32>(4)? != 0,
                })
            })?;
            result.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
        }

        let definitions = Self::get_active_definitions(conn)?;
        for log in &mut result {
            if let Some(definition) = definitions.iter().find(|def| def.id == log.dhikr_id) {
                if definition.dhikr_type == DhikrType::Counter {
                    log.completed = log.count >= definition.target_count;
                }
            }
        }
        Ok(result)
    }

    /// Save progress in the correct period store for the definition.
    /// Weekly rows are keyed by the Monday of their ISO-style Monday-to-Sunday
    /// week; previous daily rows are preserved by the schema migration.
    pub fn upsert_log(
        conn: &Connection,
        dhikr_id: i64,
        date: &str,
        count: i32,
        completed: bool,
    ) -> Result<()> {
        if count < 0 {
            return Err(anyhow!("Dhikr count cannot be negative"));
        }
        validate_tracking_date(date)?;

        let frequency: String = conn
            .query_row(
                "SELECT frequency FROM dhikr_definitions WHERE id = ?1",
                params![dhikr_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow!("Dhikr definition {dhikr_id} does not exist"))?;

        match DhikrFrequency::from_str(&frequency)? {
            DhikrFrequency::Daily => {
                conn.execute(
                    "INSERT INTO dhikr_log (dhikr_id, date, count, completed)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(dhikr_id, date) DO UPDATE
                     SET count = excluded.count, completed = excluded.completed",
                    params![dhikr_id, date, count, completed as i32],
                )?;
            }
            DhikrFrequency::Weekly => {
                let (week_start, _) = Self::weekly_period_for_date(date)?;
                conn.execute(
                    "INSERT INTO dhikr_weekly_log
                        (dhikr_id, week_start, count, completed)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(dhikr_id, week_start) DO UPDATE
                     SET count = excluded.count, completed = excluded.completed",
                    params![dhikr_id, week_start, count, completed as i32],
                )?;
            }
        }
        Ok(())
    }

    /// Add a custom definition after validating its fields and display order.
    pub fn add_custom(
        conn: &Connection,
        name: &str,
        dhikr_type: &str,
        target: i32,
        frequency: &str,
    ) -> Result<()> {
        let normalized_name = validate_dhikr_definition(name, dhikr_type, target, frequency)?;

        // Do not hide an actual database error while determining sort order.
        let max_order: i32 = conn.query_row(
            "SELECT COALESCE(MAX(sort_order), 100) FROM dhikr_definitions WHERE category = 'custom'",
            [],
            |row| row.get(0),
        )?;
        if max_order == i32::MAX {
            return Err(anyhow!("Dhikr display order is exhausted"));
        }

        conn.execute(
            "INSERT INTO dhikr_definitions (name, dhikr_type, frequency, target_count, category, sort_order, active)
             VALUES (?1, ?2, ?3, ?4, 'custom', ?5, 1)",
            params![normalized_name, dhikr_type, frequency, target, max_order + 1],
        )
        .map_err(|error| {
            if matches!(
                error,
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error { code: rusqlite::ffi::ErrorCode::ConstraintViolation, .. },
                    _
                )
            ) {
                anyhow!("A Dhikr with that name already exists (names are case-insensitive)")
            } else {
                error.into()
            }
        })?;
        Ok(())
    }

    /// Find an active definition by trimmed, case-insensitive name.
    pub fn find_by_name(conn: &Connection, name: &str) -> Result<Option<DhikrDef>> {
        let defs = Self::get_definitions(conn, Some(true))?;
        Ok(defs
            .into_iter()
            .find(|definition| definition.name.trim().eq_ignore_ascii_case(name.trim())))
    }

    /// Find a definition by name regardless of its active state.
    pub fn find_any_by_name(conn: &Connection, name: &str) -> Result<Option<DhikrDef>> {
        let normalized = name.trim();
        if normalized.is_empty() {
            return Err(anyhow!("Dhikr name cannot be empty"));
        }
        Ok(Self::get_all_definitions(conn)?
            .into_iter()
            .find(|definition| definition.name.trim().eq_ignore_ascii_case(normalized)))
    }

    /// Edit a custom definition without changing its historical progress.
    pub fn edit_custom(
        conn: &Connection,
        name: &str,
        new_name: Option<&str>,
        dhikr_type: Option<&str>,
        target: Option<i32>,
        frequency: Option<&str>,
    ) -> Result<()> {
        let definition = Self::find_any_by_name(conn, name)?
            .ok_or_else(|| anyhow!("Dhikr '{}' not found", name))?;
        if definition.category != DhikrCategory::Custom {
            return Err(anyhow!("Built-in Dhikr definitions are protected"));
        }
        if new_name.is_none() && dhikr_type.is_none() && target.is_none() && frequency.is_none() {
            return Err(anyhow!("Specify at least one property to edit"));
        }
        let candidate_name = new_name.unwrap_or(&definition.name);
        let candidate_type = dhikr_type.unwrap_or(definition.dhikr_type.as_str());
        let candidate_target = target.unwrap_or(definition.target_count);
        let candidate_frequency = frequency.unwrap_or(definition.frequency.as_str());
        if candidate_frequency != definition.frequency.as_str() {
            let daily: i64 = conn.query_row(
                "SELECT COUNT(*) FROM dhikr_log WHERE dhikr_id = ?1 AND (count > 0 OR completed = 1)",
                params![definition.id],
                |row| row.get(0),
            )?;
            let weekly: i64 = conn.query_row(
                "SELECT COUNT(*) FROM dhikr_weekly_log WHERE dhikr_id = ?1 AND (count > 0 OR completed = 1)",
                params![definition.id],
                |row| row.get(0),
            )?;
            if daily > 0 || weekly > 0 {
                return Err(anyhow!(
                    "Cannot change frequency after Dhikr progress exists"
                ));
            }
        }
        let normalized_name = validate_dhikr_definition(
            candidate_name,
            candidate_type,
            candidate_target,
            candidate_frequency,
        )?;

        conn.execute(
            "UPDATE dhikr_definitions
             SET name = ?1, dhikr_type = ?2, target_count = ?3, frequency = ?4
             WHERE id = ?5",
            params![
                normalized_name,
                candidate_type,
                candidate_target,
                candidate_frequency,
                definition.id
            ],
        )
        .map_err(|error| {
            if matches!(
                error,
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error {
                        code: rusqlite::ffi::ErrorCode::ConstraintViolation,
                        ..
                    },
                    _
                )
            ) {
                anyhow!("A Dhikr with that name already exists (names are case-insensitive)")
            } else {
                error.into()
            }
        })?;
        Ok(())
    }

    /// Set a definition's active state, protecting built-ins from deactivation.
    pub fn set_active(conn: &Connection, name: &str, active: bool) -> Result<()> {
        let definition = Self::find_any_by_name(conn, name)?
            .ok_or_else(|| anyhow!("Dhikr '{}' not found", name))?;
        if !active && definition.category != DhikrCategory::Custom {
            return Err(anyhow!("Built-in Dhikr definitions are protected"));
        }
        conn.execute(
            "UPDATE dhikr_definitions SET active = ?1 WHERE id = ?2",
            params![active as i32, definition.id],
        )?;
        Ok(())
    }

    /// Set the display order for a custom definition.
    pub fn reorder(conn: &Connection, name: &str, sort_order: i32) -> Result<()> {
        if sort_order < 0 {
            return Err(anyhow!("Dhikr display order cannot be negative"));
        }
        let definition = Self::find_any_by_name(conn, name)?
            .ok_or_else(|| anyhow!("Dhikr '{}' not found", name))?;
        if definition.category != DhikrCategory::Custom {
            return Err(anyhow!("Built-in Dhikr definitions are protected"));
        }
        conn.execute(
            "UPDATE dhikr_definitions SET sort_order = ?1 WHERE id = ?2",
            params![sort_order, definition.id],
        )?;
        Ok(())
    }
}

// ─── Qada repo ───────────────────────────────────────────────────────────────

pub struct QadaRepo;

impl QadaRepo {
    pub fn get_queue(conn: &Connection) -> Result<Vec<QadaEntry>> {
        let mut stmt = conn.prepare(
            "SELECT id, prayer_type, original_date, completed, completed_at
             FROM qada_queue WHERE completed = 0
             ORDER BY original_date, id",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i32>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;

        let mut result = Vec::new();
        for r in rows {
            let (id, prayer_type, original_date, completed, completed_at) = r?;
            result.push(QadaEntry {
                id,
                prayer_type: PrayerType::from_str(&prayer_type)
                    .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?,
                original_date,
                completed: completed != 0,
                completed_at,
            });
        }
        Ok(result)
    }

    /// Add a manual Qada entry only when this prayer/date has not already been
    /// recorded. This keeps repeated CLI/TUI actions from creating duplicates.
    /// Add a Qada entry for the supplied original missed date.
    pub fn add_entry(conn: &Connection, prayer_type: &str, original_date: &str) -> Result<bool> {
        PrayerType::from_str(prayer_type)
            .map_err(|_| anyhow!("Unknown prayer type '{prayer_type}'"))?;
        validate_tracking_date(original_date)?;

        let inserted = conn.execute(
            "INSERT INTO qada_queue (prayer_type, original_date, completed)
             SELECT ?1, ?2, 0
             WHERE NOT EXISTS (
                 SELECT 1 FROM qada_queue WHERE prayer_type = ?1 AND original_date = ?2
             )",
            params![prayer_type, original_date],
        )?;
        Ok(inserted == 1)
    }

    pub fn complete_oldest(conn: &Connection) -> Result<bool> {
        // A single UPDATE selects and completes the oldest outstanding entry,
        // avoiding the previous SELECT-then-UPDATE race between concurrent runs.
        let changed = conn.execute(
            "UPDATE qada_queue
             SET completed = 1, completed_at = datetime('now')
             WHERE id = (
                 SELECT id FROM qada_queue
                 WHERE completed = 0
                 ORDER BY original_date, id
                 LIMIT 1
             ) AND completed = 0",
            [],
        )?;
        Ok(changed == 1)
    }

    pub fn count_pending(conn: &Connection) -> Result<i64> {
        conn.query_row(
            "SELECT COUNT(*) FROM qada_queue WHERE completed = 0",
            [],
            |row| row.get(0),
        )
        .map_err(anyhow::Error::from)
    }
}

// ─── Quran repo ──────────────────────────────────────────────────────────────

pub struct QuranRepo;

impl QuranRepo {
    /// Add pages using an immediate transaction to serialize concurrent writes.
    pub fn log_pages(conn: &Connection, date: &str, pages: f64) -> Result<()> {
        validate_quran_pages(pages)?;
        validate_tracking_date(date)?;

        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<()> {
            let current: f64 = conn
                .query_row(
                    "SELECT COALESCE(pages, 0) FROM quran_log WHERE date = ?1",
                    params![date],
                    |row| row.get(0),
                )
                .optional()?
                .unwrap_or(0.0);
            if !current.is_finite() || current < 0.0 {
                return Err(anyhow!(
                    "Stored Quran page total is invalid; refusing to update it"
                ));
            }
            let updated_total = current + pages;
            if !updated_total.is_finite() {
                return Err(anyhow!(
                    "The resulting Quran page total is outside the supported numeric range"
                ));
            }

            conn.execute(
                "INSERT INTO quran_log (date, pages) VALUES (?1, ?2)
             ON CONFLICT(date) DO UPDATE
             SET pages = COALESCE(quran_log.pages, 0) + excluded.pages",
                params![date, pages],
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn get_today(conn: &Connection, date: &str) -> Result<f64> {
        conn.query_row(
            "SELECT COALESCE(pages, 0) FROM quran_log WHERE date = ?1",
            params![date],
            |row| row.get(0),
        )
        .optional()
        .map(|v| v.unwrap_or(0.0))
        .map_err(anyhow::Error::from)
    }

    pub fn get_weekly_total(conn: &Connection, start_date: &str, end_date: &str) -> Result<f64> {
        let start = parse_tracking_date(start_date)?;
        let end = parse_tracking_date(end_date)?;
        if start > end {
            return Err(anyhow!(
                "Quran statistics range start {start_date} must not be after end {end_date}"
            ));
        }

        // Sum in Rust so several individually valid finite daily values cannot
        // silently produce an infinite weekly total in SQLite.
        let mut stmt = conn.prepare(
            "SELECT COALESCE(pages, 0) FROM quran_log WHERE date >= ?1 AND date <= ?2 ORDER BY date",
        )?;
        let rows = stmt.query_map(params![start_date, end_date], |row| row.get::<_, f64>(0))?;
        let mut total = 0.0f64;
        for row in rows {
            let pages = row?;
            if !pages.is_finite() || pages < 0.0 {
                return Err(anyhow!(
                    "Stored Quran page amount is invalid in the requested statistics range"
                ));
            }

            total += pages;
            if !total.is_finite() {
                return Err(anyhow!(
                    "Weekly Quran total exceeds the supported numeric range"
                ));
            }
        }

        Ok(total)
    }
}

// ─── Stats repo ──────────────────────────────────────────────────────────────

pub struct StatsRepo;

impl StatsRepo {
    /// Return one row for every calendar date in the requested inclusive range.
    /// Dates without prayer records are represented as 0/0 instead of omitted.
    pub fn get_daily_stats_range(
        conn: &Connection,
        start: &str,
        end: &str,
    ) -> Result<Vec<DailyStats>> {
        let start_date = parse_tracking_date(start)?;
        let end_date = parse_tracking_date(end)?;
        if start_date > end_date {
            return Err(anyhow!(
                "Statistics range start {start} must not be after end {end}"
            ));
        }

        let mut stmt = conn.prepare(
            "SELECT date,
                    SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END) AS done,
                    COUNT(*) AS total
             FROM prayers
             WHERE date >= ?1 AND date <= ?2 AND is_qada = 0
             GROUP BY date
             ORDER BY date",
        )?;
        let rows = stmt.query_map(params![start, end], |row| {
            Ok(DailyStats {
                date: row.get(0)?,
                prayers_done: row.get::<_, i32>(1)? as u8,
                prayers_total: row.get::<_, i32>(2)? as u8,
            })
        })?;

        let mut by_date = std::collections::HashMap::new();
        for row in rows {
            let stats = row?;
            by_date.insert(stats.date.clone(), stats);
        }

        let mut result = Vec::new();
        let mut date = start_date;
        loop {
            let date_string = date.format("%Y-%m-%d").to_string();
            result.push(by_date.remove(&date_string).unwrap_or(DailyStats {
                date: date_string,
                prayers_done: 0,
                prayers_total: 0,
            }));

            if date == end_date {
                break;
            }
            date = date
                .succ_opt()
                .ok_or_else(|| anyhow!("Statistics date range exceeds supported dates"))?;
        }

        Ok(result)
    }

    /// A completed streak is a sequence of days with all five daily prayers
    /// marked done. The existing product rule is retained: the current streak
    /// must end today, so it remains zero until today's five prayers are done.
    /// Future-dated records never affect current or best streaks.
    pub fn calculate_streak(conn: &Connection, today: NaiveDate) -> Result<Streak> {
        let today_string = today.format("%Y-%m-%d").to_string();
        let mut stmt = conn.prepare(
            "SELECT date
             FROM prayers
             WHERE is_qada = 0 AND date <= ?1
             GROUP BY date
             HAVING COUNT(*) = 5
                AND SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END) = 5
             ORDER BY date DESC",
        )?;

        let rows = stmt.query_map(params![today_string], |row| row.get::<_, String>(0))?;
        let mut completed_dates = std::collections::BTreeSet::new();
        for row in rows {
            if let Ok(date) = NaiveDate::parse_from_str(&row?, "%Y-%m-%d") {
                if date <= today {
                    completed_dates.insert(date);
                }
            }
        }

        let mut current = 0u32;
        let mut expected = today;
        while completed_dates.contains(&expected) {
            current = current.saturating_add(1);
            let Some(previous) = expected.pred_opt() else {
                break;
            };
            expected = previous;
        }

        let best = calculate_best_streak(&completed_dates);
        Ok(Streak { current, best })
    }

    pub fn get_weekly_grid(conn: &Connection, start: &str, end: &str) -> Result<Vec<DailyStats>> {
        Self::get_daily_stats_range(conn, start, end)
    }
}
fn calculate_best_streak(dates: &std::collections::BTreeSet<NaiveDate>) -> u32 {
    let mut best = 0u32;
    let mut current = 0u32;
    let mut previous: Option<NaiveDate> = None;

    for date in dates {
        current = match previous.and_then(|date| date.succ_opt()) {
            Some(expected) if expected == *date => current.saturating_add(1),
            _ => 1,
        };
        best = best.max(current);
        previous = Some(*date);
    }

    best
}

fn parse_tracking_date(date: &str) -> Result<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|error| anyhow!("Invalid tracking date '{date}': {error}"))?;
    if parsed.format("%Y-%m-%d").to_string() != date {
        return Err(anyhow!(
            "Invalid tracking date '{date}': expected YYYY-MM-DD format"
        ));
    }
    Ok(parsed)
}

// ─── App meta ────────────────────────────────────────────────────────────────

pub struct MetaRepo;

impl MetaRepo {
    pub fn get(conn: &Connection, key: &str) -> Result<Option<String>> {
        conn.query_row(
            "SELECT value FROM app_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(anyhow::Error::from)
    }

    pub fn set(conn: &Connection, key: &str, value: &str) -> Result<()> {
        conn.execute(
            "INSERT INTO app_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }
}

fn validate_tracking_date(date: &str) -> Result<()> {
    parse_tracking_date(date).map(|_| ())
}

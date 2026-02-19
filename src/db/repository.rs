use anyhow::{anyhow, Result};
use chrono::NaiveTime;
use rusqlite::{params, Connection, OptionalExtension};
use std::str::FromStr;

use crate::models::{
    DailyStats, DhikrCategory, DhikrDef, DhikrFrequency, DhikrLog, DhikrType, Prayer,
    PrayerStatus, PrayerType, QadaEntry, Streak,
};

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
    pub fn get_times_for_date(conn: &Connection, date: &str) -> Result<Option<CachedTimes>> {
        let row = conn
            .query_row(
                "SELECT fajr, sunrise, zuhr, asr, maghrib, isha FROM prayer_times_cache WHERE date = ?1",
                params![date],
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

    pub fn store_times(conn: &Connection, date: &str, times: &CachedTimes) -> Result<()> {
        conn.execute(
            "INSERT OR REPLACE INTO prayer_times_cache (date, fajr, sunrise, zuhr, asr, maghrib, isha)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                date,
                times.fajr.format("%H:%M").to_string(),
                times.sunrise.format("%H:%M").to_string(),
                times.zuhr.format("%H:%M").to_string(),
                times.asr.format("%H:%M").to_string(),
                times.maghrib.format("%H:%M").to_string(),
                times.isha.format("%H:%M").to_string(),
            ],
        )?;
        Ok(())
    }
}

// ─── Prayer repo ─────────────────────────────────────────────────────────────

pub struct PrayerRepo;

impl PrayerRepo {
    /// Ensure a row exists for each prayer type for the given date (status='pending')
    pub fn ensure_today_rows(conn: &Connection, date: &str) -> Result<()> {
        for pt in PrayerType::all() {
            conn.execute(
                "INSERT OR IGNORE INTO prayers (prayer_type, date, status, is_qada)
                 VALUES (?1, ?2, 'pending', 0)",
                params![pt.as_str(), date],
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
        conn.execute(
            "UPDATE prayers SET status = ?1 WHERE prayer_type = ?2 AND date = ?3 AND is_qada = 0",
            params![status, prayer_type, date],
        )?;
        Ok(())
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
    pub fn get_active_definitions(conn: &Connection) -> Result<Vec<DhikrDef>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, dhikr_type, frequency, target_count, category, sort_order
             FROM dhikr_definitions WHERE active = 1 ORDER BY sort_order, id",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i32>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i32>(6)?,
            ))
        })?;

        let mut result = Vec::new();
        for r in rows {
            let (id, name, dhikr_type, frequency, target_count, category, sort_order) = r?;
            let dhikr_type = match dhikr_type.as_str() {
                "checkbox" => DhikrType::Checkbox,
                _ => DhikrType::Counter,
            };
            let frequency = match frequency.as_str() {
                "weekly" => DhikrFrequency::Weekly,
                _ => DhikrFrequency::Daily,
            };
            let category = match category.as_str() {
                "custom" => DhikrCategory::Custom,
                _ => DhikrCategory::Builtin,
            };
            result.push(DhikrDef {
                id,
                name,
                dhikr_type,
                frequency,
                target_count,
                category,
                sort_order,
                active: true,
            });
        }
        Ok(result)
    }

    pub fn get_log_for_date(conn: &Connection, date: &str) -> Result<Vec<DhikrLog>> {
        let mut stmt = conn.prepare(
            "SELECT id, dhikr_id, date, count, completed FROM dhikr_log WHERE date = ?1",
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

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(anyhow::Error::from)
    }

    pub fn upsert_log(
        conn: &Connection,
        dhikr_id: i64,
        date: &str,
        count: i32,
        completed: bool,
    ) -> Result<()> {
        conn.execute(
            "INSERT INTO dhikr_log (dhikr_id, date, count, completed)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(dhikr_id, date) DO UPDATE SET count = ?3, completed = ?4",
            params![dhikr_id, date, count, completed as i32],
        )?;
        Ok(())
    }

    pub fn add_custom(
        conn: &Connection,
        name: &str,
        dhikr_type: &str,
        target: i32,
        frequency: &str,
    ) -> Result<()> {
        // Get max sort_order for custom
        let max_order: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(sort_order), 100) FROM dhikr_definitions WHERE category = 'custom'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(100);

        conn.execute(
            "INSERT INTO dhikr_definitions (name, dhikr_type, frequency, target_count, category, sort_order, active)
             VALUES (?1, ?2, ?3, ?4, 'custom', ?5, 1)",
            params![name, dhikr_type, frequency, target, max_order + 1],
        )?;
        Ok(())
    }

    pub fn find_by_name(conn: &Connection, name: &str) -> Result<Option<DhikrDef>> {
        let defs = Self::get_active_definitions(conn)?;
        Ok(defs.into_iter().find(|d| d.name.to_lowercase() == name.to_lowercase()))
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

    pub fn add_entry(conn: &Connection, prayer_type: &str, original_date: &str) -> Result<()> {
        conn.execute(
            "INSERT INTO qada_queue (prayer_type, original_date, completed) VALUES (?1, ?2, 0)",
            params![prayer_type, original_date],
        )?;
        Ok(())
    }

    pub fn complete_oldest(conn: &Connection) -> Result<bool> {
        let oldest_id: Option<i64> = conn
            .query_row(
                "SELECT id FROM qada_queue WHERE completed = 0 ORDER BY original_date, id LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;

        match oldest_id {
            None => Ok(false),
            Some(id) => {
                conn.execute(
                    "UPDATE qada_queue SET completed = 1, completed_at = datetime('now') WHERE id = ?1",
                    params![id],
                )?;
                Ok(true)
            }
        }
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
    pub fn log_pages(conn: &Connection, date: &str, pages: f64) -> Result<()> {
        conn.execute(
            "INSERT INTO quran_log (date, pages) VALUES (?1, ?2)
             ON CONFLICT(date) DO UPDATE SET pages = pages + ?2",
            params![date, pages],
        )?;
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
        conn.query_row(
            "SELECT COALESCE(SUM(pages), 0) FROM quran_log WHERE date >= ?1 AND date <= ?2",
            params![start_date, end_date],
            |row| row.get(0),
        )
        .map_err(anyhow::Error::from)
    }
}

// ─── Stats repo ──────────────────────────────────────────────────────────────

pub struct StatsRepo;

impl StatsRepo {
    pub fn get_daily_stats_range(
        conn: &Connection,
        start: &str,
        end: &str,
    ) -> Result<Vec<DailyStats>> {
        let mut stmt = conn.prepare(
            "SELECT date,
                    SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END) as done,
                    COUNT(*) as total
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

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(anyhow::Error::from)
    }

    pub fn calculate_streak(conn: &Connection) -> Result<Streak> {
        // Get all dates with all 5 prayers done, ordered desc
        let mut stmt = conn.prepare(
            "SELECT date FROM prayers
             WHERE is_qada = 0
             GROUP BY date
             HAVING SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END) >= 5
             ORDER BY date DESC",
        )?;

        let dates: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // Calculate current streak (consecutive days ending at today)
        let today = chrono::Local::now().date_naive();
        let mut current = 0u32;
        let mut check_date = today;

        for date_str in &dates {
            let d = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .unwrap_or(chrono::NaiveDate::MIN);
            if d == check_date || d == today {
                if d == check_date {
                    current += 1;
                    check_date = check_date.pred_opt().unwrap_or(check_date);
                }
            } else {
                break;
            }
        }

        // Calculate best streak from all dates
        let best = calculate_best_streak(&dates);

        Ok(Streak { current, best })
    }

    pub fn get_weekly_grid(conn: &Connection, start: &str, end: &str) -> Result<Vec<DailyStats>> {
        Self::get_daily_stats_range(conn, start, end)
    }
}

fn calculate_best_streak(dates: &[String]) -> u32 {
    if dates.is_empty() {
        return 0;
    }

    // dates is sorted DESC - reverse for processing
    let mut sorted: Vec<chrono::NaiveDate> = dates
        .iter()
        .filter_map(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .collect();
    sorted.sort();

    let mut best = 0u32;
    let mut current = 1u32;

    for i in 1..sorted.len() {
        let prev = sorted[i - 1];
        let curr = sorted[i];
        if curr == prev.succ_opt().unwrap_or(curr) {
            current += 1;
        } else {
            current = 1;
        }
        best = best.max(current);
    }
    best.max(current)
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

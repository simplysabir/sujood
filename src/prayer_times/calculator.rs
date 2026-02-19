use anyhow::{anyhow, Result};
use chrono::{Duration, FixedOffset, NaiveDate, NaiveTime};
use rusqlite::Connection;
use salah::prelude::*;

use crate::db::repository::CacheRepo;
use crate::models::PrayerType;

#[derive(Debug, Clone)]
pub struct PrayerTimesLocal {
    pub fajr: NaiveTime,
    pub sunrise: NaiveTime,
    pub zuhr: NaiveTime,
    pub asr: NaiveTime,
    pub maghrib: NaiveTime,
    pub isha: NaiveTime,
}

pub struct PrayerCalculator {
    pub lat: f64,
    pub lng: f64,
    pub method_str: String,
    pub madhab_str: String,
    pub tz_offset_minutes: i32,
}

impl PrayerCalculator {
    pub fn new(
        lat: f64,
        lng: f64,
        method: &str,
        madhab: &str,
        tz_offset_minutes: i32,
    ) -> Result<Self> {
        // Validate method + madhab early
        parse_method(method)?;
        parse_madhab(madhab)?;
        Ok(Self {
            lat,
            lng,
            method_str: method.to_string(),
            madhab_str: madhab.to_string(),
            tz_offset_minutes,
        })
    }

    fn compute_times(&self, date: NaiveDate) -> Result<PrayerTimesLocal> {
        let coords = Coordinates::new(self.lat, self.lng);
        let method = parse_method(&self.method_str)?;
        let madhab = parse_madhab(&self.madhab_str)?;
        let params = Configuration::with(method, madhab);

        let times = PrayerSchedule::new()
            .on(date)
            .for_location(coords)
            .with_configuration(params)
            .calculate()
            .map_err(|e| anyhow!("Prayer calculation failed: {}", e))?;

        let offset = FixedOffset::east_opt(self.tz_offset_minutes * 60)
            .ok_or_else(|| anyhow!("Invalid timezone offset: {}", self.tz_offset_minutes))?;

        let to_local = |utc: chrono::DateTime<chrono::Utc>| -> NaiveTime {
            utc.with_timezone(&offset).time()
        };

        Ok(PrayerTimesLocal {
            fajr: to_local(times.time(Prayer::Fajr)),
            sunrise: to_local(times.time(Prayer::Sunrise)),
            zuhr: to_local(times.time(Prayer::Dhuhr)),
            asr: to_local(times.time(Prayer::Asr)),
            maghrib: to_local(times.time(Prayer::Maghrib)),
            isha: to_local(times.time(Prayer::Isha)),
        })
    }

    pub fn times_for_date(&self, date: NaiveDate) -> Result<PrayerTimesLocal> {
        self.compute_times(date)
    }

    /// Ensure prayer_times_cache has entries for today through `days_ahead` days.
    pub fn ensure_cached(&self, conn: &Connection, days_ahead: u32) -> Result<()> {
        let today = chrono::Local::now().date_naive();

        for i in 0..=(days_ahead as i64) {
            let date = today + Duration::days(i);
            let date_str = date.format("%Y-%m-%d").to_string();

            if CacheRepo::get_times_for_date(conn, &date_str)?.is_none() {
                let times = self.compute_times(date)?;
                let cached = crate::db::repository::CachedTimes {
                    fajr: times.fajr,
                    sunrise: times.sunrise,
                    zuhr: times.zuhr,
                    asr: times.asr,
                    maghrib: times.maghrib,
                    isha: times.isha,
                };
                CacheRepo::store_times(conn, &date_str, &cached)?;
            }
        }
        Ok(())
    }

    /// Get times from cache (or compute if missing) for a specific date.
    pub fn get_cached_or_compute(
        &self,
        conn: &Connection,
        date: NaiveDate,
    ) -> Result<PrayerTimesLocal> {
        let date_str = date.format("%Y-%m-%d").to_string();

        if let Some(cached) = CacheRepo::get_times_for_date(conn, &date_str)? {
            return Ok(PrayerTimesLocal {
                fajr: cached.fajr,
                sunrise: cached.sunrise,
                zuhr: cached.zuhr,
                asr: cached.asr,
                maghrib: cached.maghrib,
                isha: cached.isha,
            });
        }

        let times = self.compute_times(date)?;
        let cached = crate::db::repository::CachedTimes {
            fajr: times.fajr,
            sunrise: times.sunrise,
            zuhr: times.zuhr,
            asr: times.asr,
            maghrib: times.maghrib,
            isha: times.isha,
        };
        CacheRepo::store_times(conn, &date_str, &cached)?;
        Ok(times)
    }

    /// Returns (next PrayerType, seconds until it).
    /// `now_time` is the current local time.
    pub fn get_next_prayer(
        &self,
        conn: &Connection,
        now_date: NaiveDate,
        now_time: NaiveTime,
    ) -> Result<Option<(PrayerType, i64)>> {
        let today_times = self.get_cached_or_compute(conn, now_date)?;

        let schedule = [
            (PrayerType::Fajr, today_times.fajr),
            (PrayerType::Zuhr, today_times.zuhr),
            (PrayerType::Asr, today_times.asr),
            (PrayerType::Maghrib, today_times.maghrib),
            (PrayerType::Isha, today_times.isha),
        ];

        for (prayer, time) in &schedule {
            if *time > now_time {
                let secs = (*time - now_time).num_seconds();
                return Ok(Some((prayer.clone(), secs)));
            }
        }

        // All prayers passed — next is Fajr tomorrow
        let tomorrow = now_date.succ_opt().unwrap_or(now_date);
        let tomorrow_times = self.get_cached_or_compute(conn, tomorrow)?;
        let midnight_to_fajr = tomorrow_times.fajr.signed_duration_since(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        let remaining_today = NaiveTime::from_hms_opt(23, 59, 59).unwrap()
            .signed_duration_since(now_time);
        let secs = remaining_today.num_seconds() + midnight_to_fajr.num_seconds() + 1;
        Ok(Some((PrayerType::Fajr, secs)))
    }
}

fn parse_method(s: &str) -> Result<Method> {
    match s {
        "MuslimWorldLeague" => Ok(Method::MuslimWorldLeague),
        "Egyptian" => Ok(Method::Egyptian),
        "Karachi" => Ok(Method::Karachi),
        "UmmAlQura" => Ok(Method::UmmAlQura),
        "Dubai" => Ok(Method::Dubai),
        "MoonsightingCommittee" => Ok(Method::MoonsightingCommittee),
        "NorthAmerica" => Ok(Method::NorthAmerica),
        "Kuwait" => Ok(Method::Kuwait),
        "Qatar" => Ok(Method::Qatar),
        "Singapore" => Ok(Method::Singapore),
        "Tehran" => Ok(Method::Tehran),
        "Turkey" => Ok(Method::Turkey),
        "Other" => Ok(Method::Other),
        _ => Err(anyhow!("Unknown calculation method: '{}'", s)),
    }
}

fn parse_madhab(s: &str) -> Result<Madhab> {
    match s {
        "Hanafi" => Ok(Madhab::Hanafi),
        "Shafi" | "Shafi'i" => Ok(Madhab::Shafi),
        _ => Err(anyhow!("Unknown madhab: '{}'", s)),
    }
}

pub const CALC_METHODS: &[&str] = &[
    "MuslimWorldLeague",
    "Egyptian",
    "Karachi",
    "UmmAlQura",
    "Dubai",
    "MoonsightingCommittee",
    "NorthAmerica",
    "Kuwait",
    "Qatar",
    "Singapore",
    "Tehran",
    "Turkey",
    "Other",
];

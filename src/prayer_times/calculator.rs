use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveTime};
use rusqlite::Connection;
use salah::prelude::*;

use crate::db::repository::CacheRepo;
use crate::models::PrayerType;
use crate::utils::{
    time::now_at_offset,
    validation::{validate_coordinates, validate_timezone_offset_minutes},
};

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
        // Validate all calculation inputs before they reach the prayer library.
        validate_coordinates(lat, lng)?;
        validate_timezone_offset_minutes(tz_offset_minutes)?;
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

    /// Stable identity for every input that affects the calculated schedule.
    /// The bit representation avoids locale/rounding differences in float text.
    pub fn cache_fingerprint(&self) -> String {
        format!(
            "lat={:016x};lng={:016x};method={};madhab={};offset={}",
            self.lat.to_bits(),
            self.lng.to_bits(),
            self.method_str,
            self.madhab_str,
            self.tz_offset_minutes,
        )
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

    /// Ensure prayer_times_cache has entries for the configured local date through
    /// `days_ahead` days. The host machine timezone is never used.
    pub fn ensure_cached(&self, conn: &Connection, days_ahead: u32) -> Result<()> {
        let today = now_at_offset(self.tz_offset_minutes)?.date_naive();
        self.ensure_cached_from(conn, today, days_ahead)
    }

    /// Cache a deterministic date range. This is also useful for regression tests.
    pub fn ensure_cached_from(
        &self,
        conn: &Connection,
        start_date: NaiveDate,
        days_ahead: u32,
    ) -> Result<()> {
        let fingerprint = self.cache_fingerprint();
        for i in 0..=(days_ahead as i64) {
            let date = start_date + Duration::days(i);
            let date_str = date.format("%Y-%m-%d").to_string();

            if CacheRepo::get_times_for_date(conn, &date_str, &fingerprint)?.is_none() {
                let times = self.compute_times(date)?;
                let cached = crate::db::repository::CachedTimes {
                    fajr: times.fajr,
                    sunrise: times.sunrise,
                    zuhr: times.zuhr,
                    asr: times.asr,
                    maghrib: times.maghrib,
                    isha: times.isha,
                };
                CacheRepo::store_times(conn, &date_str, &fingerprint, &cached)?;
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
        let fingerprint = self.cache_fingerprint();

        if let Some(cached) =
            CacheRepo::get_times_for_date(conn, &date_str, &fingerprint)?
        {
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
        CacheRepo::store_times(conn, &date_str, &fingerprint, &cached)?;
        Ok(times)
    }

    /// Returns (next PrayerType, seconds until it) for an instant expressed in
    /// this calculator's configured fixed offset. Passing one timezone-aware
    /// instant prevents callers from accidentally mixing a host-local date and
    /// time with the configured prayer schedule.
    pub fn get_next_prayer(
        &self,
        conn: &Connection,
        now: DateTime<FixedOffset>,
    ) -> Result<Option<(PrayerType, i64)>> {
        let expected_offset_seconds = self.tz_offset_minutes * 60;
        if now.offset().local_minus_utc() != expected_offset_seconds {
            return Err(anyhow!(
                "Current time uses UTC offset {} seconds, but Sujood is configured for {} seconds",
                now.offset().local_minus_utc(),
                expected_offset_seconds
            ));
        }

        let now_date = now.date_naive();
        let now_time = now.time();
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
        let tomorrow = now_date.succ_opt().ok_or_else(|| {
            anyhow!("Cannot calculate prayer time beyond the maximum supported date")
        })?;
        let tomorrow_times = self.get_cached_or_compute(conn, tomorrow)?;
        let now = now_date.and_time(now_time);
        let next_fajr = tomorrow.and_time(tomorrow_times.fajr);
        let secs = (next_fajr - now).num_seconds();
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

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate};
use hijri_date::HijriDate;

/// Islamic month names in English (index 0 = Muharram = month 1)
const HIJRI_MONTH_NAMES: &[&str] = &[
    "Muharram",
    "Safar",
    "Rabi' al-Awwal",
    "Rabi' al-Thani",
    "Jumada al-Awwal",
    "Jumada al-Thani",
    "Rajab",
    "Sha'ban",
    "Ramadan",
    "Shawwal",
    "Dhu al-Qi'dah",
    "Dhu al-Hijjah",
];

fn hijri_month_name(month: usize) -> &'static str {
    if (1..=12).contains(&month) {
        HIJRI_MONTH_NAMES[month - 1]
    } else {
        "Unknown"
    }
}

pub struct HijriInfo {
    pub day: usize,
    pub month: usize,
    pub year: usize,
    pub month_name: String,
    pub day_name: String,
}

impl HijriInfo {
    pub fn formatted(&self) -> String {
        format!("{} {} {}", self.day, self.month_name, self.year)
    }
}

pub fn to_hijri(date: NaiveDate) -> Result<HijriInfo> {
    let hd = HijriDate::from_gr(
        date.year() as usize,
        date.month() as usize,
        date.day() as usize,
    )
    .map_err(|e| anyhow::anyhow!("Hijri conversion error: {}", e))?;

    let month = hd.month();
    Ok(HijriInfo {
        day: hd.day(),
        month,
        year: hd.year(),
        month_name: hijri_month_name(month).to_string(),
        day_name: hd.day_name_en(),
    })
}

/// Convert the supplied Sujood-local date to its Hijri representation, applying
/// the configured moon-sighting adjustment before conversion.
pub fn hijri_string_for_date(date: NaiveDate, offset_days: i32) -> Result<String> {
    let adjusted = date + Duration::days(i64::from(offset_days));
    match to_hijri(adjusted) {
        Ok(hijri) => Ok(hijri.formatted()),
        Err(_) => Ok(format!("Hijri unavailable ({adjusted})")),
    }
}

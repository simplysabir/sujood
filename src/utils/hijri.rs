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
    if month >= 1 && month <= 12 {
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

/// Returns the Hijri date string for today, with an optional day offset.
/// `offset_days` lets users adjust for local moon sighting differences
/// (e.g., -1 if your country is one day behind Saudi Arabia).
pub fn today_hijri_string(offset_days: i32) -> String {
    let today = chrono::Local::now().date_naive();
    let adjusted = today + Duration::days(offset_days as i64);

    match HijriDate::from_gr(
        adjusted.year() as usize,
        adjusted.month() as usize,
        adjusted.day() as usize,
    ) {
        Ok(hd) => format!("{} {} {}", hd.day(), hijri_month_name(hd.month()), hd.year()),
        Err(_) => {
            // Fallback: use today without offset
            let hd = HijriDate::today();
            format!("{} {} {}", hd.day(), hijri_month_name(hd.month()), hd.year())
        }
    }
}

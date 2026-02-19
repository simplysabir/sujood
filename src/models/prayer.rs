#![allow(dead_code)]
use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrayerType {
    Fajr,
    Zuhr,
    Asr,
    Maghrib,
    Isha,
}

impl PrayerType {
    pub fn all() -> Vec<PrayerType> {
        vec![
            PrayerType::Fajr,
            PrayerType::Zuhr,
            PrayerType::Asr,
            PrayerType::Maghrib,
            PrayerType::Isha,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PrayerType::Fajr => "fajr",
            PrayerType::Zuhr => "zuhr",
            PrayerType::Asr => "asr",
            PrayerType::Maghrib => "maghrib",
            PrayerType::Isha => "isha",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            PrayerType::Fajr => "Fajr",
            PrayerType::Zuhr => "Zuhr",
            PrayerType::Asr => "Asr",
            PrayerType::Maghrib => "Maghrib",
            PrayerType::Isha => "Isha",
        }
    }
}

impl std::fmt::Display for PrayerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

impl FromStr for PrayerType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "fajr" => Ok(PrayerType::Fajr),
            "zuhr" | "dhuhr" | "dhuhur" => Ok(PrayerType::Zuhr),
            "asr" => Ok(PrayerType::Asr),
            "maghrib" => Ok(PrayerType::Maghrib),
            "isha" => Ok(PrayerType::Isha),
            _ => Err(anyhow::anyhow!("Unknown prayer type: {}", s)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrayerStatus {
    Pending,
    Done,
    Missed,
}

impl PrayerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrayerStatus::Pending => "pending",
            PrayerStatus::Done => "done",
            PrayerStatus::Missed => "missed",
        }
    }
}

impl FromStr for PrayerStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(PrayerStatus::Pending),
            "done" => Ok(PrayerStatus::Done),
            "missed" => Ok(PrayerStatus::Missed),
            _ => Err(anyhow::anyhow!("Unknown prayer status: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prayer {
    pub id: Option<i64>,
    pub prayer_type: PrayerType,
    pub date: String,
    pub status: PrayerStatus,
    pub is_qada: bool,
    pub note: Option<String>,
    /// Computed from cache — not stored directly in this struct
    pub time: Option<NaiveTime>,
}

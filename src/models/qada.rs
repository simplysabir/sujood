use serde::{Deserialize, Serialize};

use crate::models::PrayerType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QadaEntry {
    pub id: i64,
    pub prayer_type: PrayerType,
    pub original_date: String,
    pub completed: bool,
    pub completed_at: Option<String>,
}

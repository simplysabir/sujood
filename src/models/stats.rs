use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyStats {
    pub date: String,
    pub prayers_done: u8,
    pub prayers_total: u8,
}

impl DailyStats {
    pub fn completion_ratio(&self) -> f64 {
        if self.prayers_total == 0 {
            0.0
        } else {
            self.prayers_done as f64 / self.prayers_total as f64
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Streak {
    pub current: u32,
    pub best: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyGrid {
    pub days: Vec<DailyStats>,
}

impl WeeklyGrid {
    pub fn new(days: Vec<DailyStats>) -> Self {
        Self { days }
    }

    pub fn total_done(&self) -> u32 {
        self.days.iter().map(|d| d.prayers_done as u32).sum()
    }

    pub fn days_with_full_prayers(&self) -> u32 {
        self.days
            .iter()
            .filter(|d| d.prayers_done >= 5)
            .count() as u32
    }
}

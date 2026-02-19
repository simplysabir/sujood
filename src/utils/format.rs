use chrono::NaiveTime;

/// Format a duration in seconds to "Xh Ym" or "Ym" string
pub fn format_duration_secs(secs: i64) -> String {
    if secs <= 0 {
        return "now".to_string();
    }
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

/// Format a NaiveTime to "HH:MM"
pub fn format_time(t: NaiveTime) -> String {
    t.format("%H:%M").to_string()
}

/// Format pages as a decimal string, trimming trailing zeros
pub fn format_pages(pages: f64) -> String {
    if pages == pages.floor() {
        format!("{}", pages as i64)
    } else {
        format!("{:.1}", pages)
    }
}

/// Create a simple ASCII progress bar
pub fn progress_bar(filled: u32, total: u32, width: usize) -> String {
    if total == 0 {
        return "░".repeat(width);
    }
    let ratio = (filled as f64 / total as f64).min(1.0);
    let filled_count = (ratio * width as f64).round() as usize;
    let empty_count = width.saturating_sub(filled_count);
    format!("{}{}", "█".repeat(filled_count), "░".repeat(empty_count))
}

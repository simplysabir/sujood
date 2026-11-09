use anyhow::{bail, Result};

/// Validate the fixed UTC offset currently stored by Sujood.
///
/// The supported civil-time range is UTC-12:00 through UTC+14:00. Keeping this
/// validation in one place prevents invalid offsets from overflowing during
/// conversion and ensures every entry point uses the same limits.
pub fn validate_timezone_offset_minutes(offset_minutes: i32) -> Result<()> {
    const MIN_OFFSET_MINUTES: i32 = -12 * 60;
    const MAX_OFFSET_MINUTES: i32 = 14 * 60;

    if !(MIN_OFFSET_MINUTES..=MAX_OFFSET_MINUTES).contains(&offset_minutes) {
        bail!(
            "Timezone offset must be between UTC-12:00 and UTC+14:00 (received {offset_minutes} minutes)"
        );
    }
    Ok(())
}

pub fn validate_coordinates(latitude: f64, longitude: f64) -> Result<()> {
    if !latitude.is_finite() || !(-90.0..=90.0).contains(&latitude) {
        bail!("Latitude must be a finite value between -90 and 90 degrees");
    }
    if !longitude.is_finite() || !(-180.0..=180.0).contains(&longitude) {
        bail!("Longitude must be a finite value between -180 and 180 degrees");
    }
    Ok(())
}

pub fn validate_quran_pages(pages: f64) -> Result<()> {
    if !pages.is_finite() || pages <= 0.0 {
        bail!("Quran pages must be a finite number greater than zero");
    }
    Ok(())
}

pub fn validate_dhikr_increment(count: i32) -> Result<()> {
    if count <= 0 {
        bail!("Dhikr increment must be greater than zero");
    }
    Ok(())
}

pub fn validate_dhikr_target(target: i32) -> Result<()> {
    if target <= 0 {
        bail!("Dhikr target must be greater than zero");
    }
    Ok(())
}

pub fn validate_dhikr_definition(
    name: &str,
    dhikr_type: &str,
    target: i32,
    frequency: &str,
) -> Result<String> {
    let normalized_name = name.trim();
    if normalized_name.is_empty() {
        bail!("Dhikr name cannot be empty");
    }

    if !matches!(dhikr_type, "checkbox" | "counter") {
        bail!("Dhikr type must be either 'checkbox' or 'counter'");
    }
    if !matches!(frequency, "daily" | "weekly") {
        bail!("Dhikr frequency must be either 'daily' or 'weekly'");
    }

    validate_dhikr_target(target)?;
    Ok(normalized_name.to_string())
}

pub fn validate_daily_quran_target(target: f64) -> Result<()> {
    if !target.is_finite() || target <= 0.0 {
        bail!("Daily Quran target must be a finite number greater than zero");
    }
    Ok(())
}

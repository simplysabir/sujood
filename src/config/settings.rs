use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs::{self, File};
use std::io::Write;

use crate::utils::validation::{
    validate_coordinates, validate_daily_quran_target, validate_timezone_offset_minutes,
};

fn default_latitude() -> f64 {
    33.6938
}
fn default_longitude() -> f64 {
    73.0651
}
fn default_location_name() -> String {
    "Islamabad".to_string()
}
fn default_calc_method() -> String {
    "MuslimWorldLeague".to_string()
}
fn default_madhab() -> String {
    "Hanafi".to_string()
}
fn default_timezone_offset() -> i32 {
    300
}
fn default_hijri_offset() -> i32 {
    0
}
fn default_daily_target() -> f64 {
    2.0
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SalahConfig {
    #[serde(default = "default_location_name")]
    pub location_name: String,
    #[serde(default = "default_latitude")]
    pub latitude: f64,
    #[serde(default = "default_longitude")]
    pub longitude: f64,
    #[serde(default = "default_calc_method")]
    pub calc_method: String,
    #[serde(default = "default_madhab")]
    pub madhab: String,
    #[serde(default = "default_timezone_offset")]
    pub timezone_offset: i32, // minutes from UTC
    /// Days to add/subtract from Hijri date for local moon sighting.
    /// 0 = default (Saudi), -1 = one day behind (e.g. some Indian regions), +1 = one day ahead
    #[serde(default = "default_hijri_offset")]
    pub hijri_offset: i32,
}

impl Default for SalahConfig {
    fn default() -> Self {
        Self {
            location_name: default_location_name(),
            latitude: default_latitude(),
            longitude: default_longitude(),
            calc_method: default_calc_method(),
            madhab: default_madhab(),
            timezone_offset: default_timezone_offset(),
            hijri_offset: default_hijri_offset(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomDhikr {
    pub name: String,
    pub dhikr_type: String,
    pub target: i32,
    pub frequency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhikrConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub show_in_main_view: bool,
    #[serde(default)]
    pub custom: Vec<CustomDhikr>,
}

impl Default for DhikrConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_in_main_view: true,
            custom: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuranConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_daily_target")]
    pub daily_target: f64,
}

impl Default for QuranConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            daily_target: 2.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub salah: SalahConfig,
    #[serde(default)]
    pub dhikr: DhikrConfig,
    #[serde(default)]
    pub quran: QuranConfig,
}

impl AppConfig {
    fn project_dirs() -> Result<ProjectDirs> {
        ProjectDirs::from("", "", "sujood")
            .context("Could not determine project directories")
    }

    pub fn config_path() -> Result<PathBuf> {
        let dirs = Self::project_dirs()?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    pub fn data_dir() -> Result<PathBuf> {
        let dirs = Self::project_dirs()?;
        Ok(dirs.data_dir().to_path_buf())
    }

    pub fn db_path() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("sujood.db"))
    }

    pub fn load() -> Result<Self> {
        let config = Self::load_unvalidated()?;
        config.validate()?;
        Ok(config)
    }

    /// Load the on-disk values without validating them. This is reserved for
    /// the setup-recovery path so users can repair an older invalid config
    /// instead of being locked out of the setup wizard.
    pub fn load_unvalidated() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let content =
            std::fs::read_to_string(&path).with_context(|| format!("Reading {:?}", path))?;
        toml::from_str(&content).context("Parsing config.toml")
    }

    /// Validate user-supplied configuration before it is used by any workflow.
    pub fn validate(&self) -> Result<()> {
        validate_coordinates(self.salah.latitude, self.salah.longitude)?;
        validate_timezone_offset_minutes(self.salah.timezone_offset)?;
        if !(-1..=1).contains(&self.salah.hijri_offset) {
            anyhow::bail!("Hijri date adjustment must be -1, 0, or +1 day");
        }
        validate_daily_quran_target(self.quran.daily_target)?;
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        self.validate()?;
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self).context("Serializing config")?;
        let temporary = path.with_extension("toml.tmp");
        let write_result = (|| -> Result<()> {
            let mut file = File::create(&temporary)
                .with_context(|| format!("Writing temporary config {:?}", temporary))?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, &path)
                .with_context(|| format!("Replacing {:?}", path))?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result?;
        Ok(())
    }

    pub fn ensure_data_dir() -> Result<PathBuf> {
        let dir = Self::data_dir()?;
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

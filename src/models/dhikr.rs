use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DhikrType {
    Checkbox,
    Counter,
}

impl DhikrType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DhikrType::Checkbox => "checkbox",
            DhikrType::Counter => "counter",
        }
    }
}

impl std::str::FromStr for DhikrType {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "checkbox" => Ok(DhikrType::Checkbox),
            "counter" => Ok(DhikrType::Counter),
            _ => Err(anyhow::anyhow!("Unknown dhikr type: {}", s)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DhikrFrequency {
    Daily,
    Weekly,
}

impl DhikrFrequency {
    pub fn as_str(&self) -> &'static str {
        match self {
            DhikrFrequency::Daily => "daily",
            DhikrFrequency::Weekly => "weekly",
        }
    }
}

impl std::str::FromStr for DhikrFrequency {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "daily" => Ok(DhikrFrequency::Daily),
            "weekly" => Ok(DhikrFrequency::Weekly),
            _ => Err(anyhow::anyhow!("Unknown dhikr frequency: {}", s)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DhikrCategory {
    Builtin,
    Custom,
}

impl DhikrCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            DhikrCategory::Builtin => "builtin",
            DhikrCategory::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhikrDef {
    pub id: i64,
    pub name: String,
    pub dhikr_type: DhikrType,
    pub frequency: DhikrFrequency,
    pub target_count: i32,
    pub category: DhikrCategory,
    pub sort_order: i32,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhikrLog {
    pub id: Option<i64>,
    pub dhikr_id: i64,
    pub date: String,
    pub count: i32,
    pub completed: bool,
}

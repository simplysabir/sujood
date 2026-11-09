// Sujood is intentionally a binary application rather than a public library.
// Import the existing source modules into this test crate so integration tests
// can exercise the real repository and domain code without changing that API.

#[path = "../src/cli/mod.rs"]
pub mod cli;
#[path = "../src/config/mod.rs"]
pub mod config;
#[path = "../src/db/mod.rs"]
pub mod db;
#[path = "../src/models/mod.rs"]
pub mod models;
#[path = "../src/prayer_times/mod.rs"]
pub mod prayer_times;
#[path = "../src/tui/mod.rs"]
pub mod tui;
#[path = "../src/utils/mod.rs"]
pub mod utils;

mod common;

#[path = "integration/database_foundation.rs"]
mod database_foundation;
#[path = "integration/pending_regressions.rs"]
mod pending_regressions;
#[path = "integration/prayer_calculator.rs"]
mod prayer_calculator;

#[path = "integration/phase3.rs"]
mod phase3;
#[path = "integration/phase4.rs"]
mod phase4;

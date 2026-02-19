mod cli;
mod config;
mod db;
mod models;
mod prayer_times;
mod tui;
mod utils;

use anyhow::{Context, Result};
use clap::Parser;
use rusqlite::Connection;

use cli::args::{Cli, Commands};
use cli::handlers;
use config::AppConfig;
use db::migrations::run_migrations;
use db::repository::MetaRepo;
use prayer_times::PrayerCalculator;

fn main() -> Result<()> {
    env_logger::init();

    let cli = Cli::parse();
    let mut config = AppConfig::load().context("Loading config")?;

    // Ensure data directory exists and open DB
    AppConfig::ensure_data_dir()?;
    let db_path = AppConfig::db_path()?;
    let conn = Connection::open(&db_path)
        .with_context(|| format!("Opening database at {:?}", db_path))?;

    // Enable WAL mode for better concurrent access
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;

    // Run migrations on every startup
    run_migrations(&conn)?;

    match cli.command {
        // Setup wizard
        Some(Commands::Setup { reset }) => {
            handlers::handle_setup(&conn, &mut config, reset)?;
        }

        // Explicit subcommands — check setup first
        Some(cmd) => {
            ensure_setup(&conn, &mut config)?;
            match cmd {
                Commands::Times => {
                    handlers::handle_times(&conn, &config)?;
                }
                Commands::Mark { prayer, missed } => {
                    handlers::handle_mark(&conn, &prayer, missed)?;
                }
                Commands::Qada { action } => {
                    handlers::handle_qada(&conn, &action)?;
                }
                Commands::Dhikr { action } => {
                    handlers::handle_dhikr(&conn, &action)?;
                }
                Commands::Quran { pages } => {
                    handlers::handle_quran(&conn, pages)?;
                }
                Commands::Stats { week } => {
                    handlers::handle_stats(&conn, week)?;
                }
                Commands::Export => {
                    handlers::handle_export(&conn, &config)?;
                }
                Commands::Setup { .. } => unreachable!(),
            }
        }

        // No subcommand → launch TUI
        None => {
            ensure_setup(&conn, &mut config)?;
            // Ensure prayer times are cached for today+7 days
            if let Ok(calc) = PrayerCalculator::new(
                config.salah.latitude,
                config.salah.longitude,
                &config.salah.calc_method,
                &config.salah.madhab,
                config.salah.timezone_offset,
            ) {
                let _ = calc.ensure_cached(&conn, 7);
            }
            tui::app::run(conn, config)?;
        }
    }

    Ok(())
}

/// Check if setup has been done; if not, run the wizard automatically.
fn ensure_setup(conn: &Connection, config: &mut AppConfig) -> Result<()> {
    let done = MetaRepo::get(conn, "setup_done")?;
    if done.as_deref() != Some("1") {
        eprintln!("No configuration found. Running setup...");
        eprintln!();
        handlers::handle_setup(conn, config, false)?;
    }
    Ok(())
}

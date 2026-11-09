use anyhow::{Result, anyhow};
use rusqlite::Connection;
use std::str::FromStr;

use crate::cli::args::{DhikrCommands, QadaCommands};
use crate::config::AppConfig;
use crate::db::repository::{DhikrRepo, MetaRepo, PrayerRepo, QadaRepo, QuranRepo, StatsRepo};
use crate::models::{DhikrFrequency, DhikrType, PrayerType};
use crate::prayer_times::calculator::PrayerCalculator;
use crate::utils::format::{format_duration_secs, format_pages};
use crate::utils::time::{format_date, now_at_offset, today_at_offset};
use crate::utils::validation::validate_dhikr_increment;

// ─── ANSI helpers ────────────────────────────────────────────────────────────

#[allow(unused_macros)]
macro_rules! print_colored {
    ($color:expr, $($arg:tt)*) => {{
        print!("{}", $color);
        print!($($arg)*);
        print!("\x1b[0m");
    }};
}

macro_rules! println_colored {
    ($color:expr, $($arg:tt)*) => {{
        print!("{}", $color);
        print!($($arg)*);
        println!("\x1b[0m");
    }};
}

const GREEN: &str = "\x1b[32m";
const AMBER: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const GOLD: &str = "\x1b[38;2;196;160;68m";

// ─── Setup wizard ────────────────────────────────────────────────────────────

pub fn handle_setup(conn: &Connection, config: &mut AppConfig, reset: bool) -> Result<()> {
    config.repair_for_setup();
    if !reset {
        if let Some(done) = MetaRepo::get(conn, "setup_done")? {
            if done == "1" {
                println!("Sujood is already configured. Use --reset to reconfigure.");
                return Ok(());
            }
        }
    }
    crate::cli::setup_tui::run_setup_tui(conn, config)
}

// ─── Times ───────────────────────────────────────────────────────────────────

pub fn handle_times(conn: &Connection, config: &AppConfig) -> Result<()> {
    let now = now_at_offset(config.salah.timezone_offset)?;
    let today = now.date_naive();
    let today_str = format_date(today);
    let now_time = now.time();

    let calc = PrayerCalculator::new(
        config.salah.latitude,
        config.salah.longitude,
        &config.salah.calc_method,
        &config.salah.madhab,
        config.salah.timezone_offset,
    )?;

    let times = calc.get_cached_or_compute(conn, today)?;

    println!();
    println_colored!(
        GOLD,
        "  Prayer Times — {} ({})",
        config.salah.location_name,
        today_str
    );
    println!();

    let prayers_with_times = [
        ("Fajr", times.fajr),
        ("Sunrise", times.sunrise),
        ("Zuhr", times.zuhr),
        ("Asr", times.asr),
        ("Maghrib", times.maghrib),
        ("Isha", times.isha),
    ];

    for (name, time) in &prayers_with_times {
        let time_str = time.format("%H:%M").to_string();
        let is_past = *time < now_time;
        if is_past {
            println_colored!(DIM, "  {:<10}  {}", name, time_str);
        } else {
            println_colored!(BOLD, "  {:<10}  {}", name, time_str);
        }
    }

    // Countdown to next prayer
    if let Some((next_prayer, secs)) = calc.get_next_prayer(conn, now)? {
        println!();
        println_colored!(
            AMBER,
            "  Next: {} in {}",
            next_prayer.display_name(),
            format_duration_secs(secs)
        );
    }
    println!();
    Ok(())
}

// ─── Mark prayer ─────────────────────────────────────────────────────────────

pub fn handle_mark(
    conn: &Connection,
    config: &AppConfig,
    prayer_str: &str,
    missed: bool,
) -> Result<()> {
    let prayer_type = PrayerType::from_str(prayer_str).map_err(|_| {
        anyhow!(
            "Unknown prayer '{}'. Use: fajr, zuhr, asr, maghrib, isha",
            prayer_str
        )
    })?;
    let today_str = format_date(today_at_offset(config.salah.timezone_offset)?);

    if missed {
        let added_to_qada =
            PrayerRepo::mark_missed_with_qada(conn, prayer_type.as_str(), &today_str)?;
        let detail = if added_to_qada {
            "added to qada queue"
        } else {
            "already present in qada queue"
        };
        println_colored!(
            RED,
            "  ✗ {} marked as missed — {}",
            prayer_type.display_name(),
            detail
        );
    } else {
        PrayerRepo::mark_status(conn, prayer_type.as_str(), &today_str, "done")?;
        println_colored!(GREEN, "  ✓ {} marked as done", prayer_type.display_name());
    }
    Ok(())
}

// ─── Qada ────────────────────────────────────────────────────────────────────

pub fn handle_qada(conn: &Connection, config: &AppConfig, action: &QadaCommands) -> Result<()> {
    match action {
        QadaCommands::List => {
            let queue = QadaRepo::get_queue(conn)?;
            let count = queue.len();
            println!();
            if count == 0 {
                println_colored!(GREEN, "  ✓ No qada prayers outstanding");
            } else {
                println_colored!(AMBER, "  Qada Queue ({} prayers)", count);
                println!();
                for entry in &queue {
                    println!(
                        "  {} — {}",
                        entry.prayer_type.display_name(),
                        entry.original_date
                    );
                }
                // Rough estimate: 1 per day
                println!();
                println_colored!(DIM, "  At 1 per day: ~{} days to clear", count);
            }
            println!();
        }
        QadaCommands::Complete => {
            let completed = QadaRepo::complete_oldest(conn)?;
            if completed {
                println_colored!(GREEN, "  ✓ Oldest qada prayer marked as completed");
            } else {
                println_colored!(GREEN, "  ✓ No qada prayers in queue");
            }
        }
        QadaCommands::Add { prayer, date } => {
            let prayer_type =
                PrayerType::from_str(prayer).map_err(|_| anyhow!("Unknown prayer '{}'", prayer))?;
            let original_date = match date {
                Some(date) => date.clone(),
                None => format_date(today_at_offset(config.salah.timezone_offset)?),
            };
            let inserted = QadaRepo::add_entry(conn, prayer_type.as_str(), &original_date)?;
            if inserted {
                println_colored!(
                    AMBER,
                    "  Added {} to qada queue",
                    prayer_type.display_name()
                );
            } else {
                println_colored!(
                    DIM,
                    "  {} is already recorded for today",
                    prayer_type.display_name()
                );
            }
        }
    }
    Ok(())
}

// ─── Dhikr ───────────────────────────────────────────────────────────────────

pub fn handle_dhikr(conn: &Connection, config: &AppConfig, action: &DhikrCommands) -> Result<()> {
    let today = format_date(today_at_offset(config.salah.timezone_offset)?);

    match action {
        DhikrCommands::Morning => {
            toggle_dhikr_by_name(conn, "Morning Adhkar", &today, None)?;
        }
        DhikrCommands::Evening => {
            toggle_dhikr_by_name(conn, "Evening Adhkar", &today, None)?;
        }
        DhikrCommands::Mark { name, count } => {
            toggle_dhikr_by_name(conn, name, &today, *count)?;
        }
        DhikrCommands::Add {
            name,
            r#type,
            target,
            freq,
        } => {
            DhikrRepo::add_custom(conn, name, r#type, *target, freq)?;
            println_colored!(GREEN, "  ✓ Added dhikr: {}", name);
        }
        DhikrCommands::List => {
            let defs = DhikrRepo::get_active_definitions(conn)?;
            let logs = DhikrRepo::get_log_for_date(conn, &today)?;
            println!();
            println_colored!(GOLD, "  Adhkar");
            println!();
            for def in &defs {
                let log = logs.iter().find(|l| l.dhikr_id == def.id);
                let (count, completed) = log.map(|l| (l.count, l.completed)).unwrap_or((0, false));
                let status = if completed {
                    format!("{}✓\x1b[0m", GREEN)
                } else {
                    match def.dhikr_type {
                        DhikrType::Counter => {
                            format!("{}/{}", count, def.target_count)
                        }
                        DhikrType::Checkbox => "○".to_string(),
                    }
                };
                let frequency_label = match &def.frequency {
                    DhikrFrequency::Daily => "[daily]",
                    DhikrFrequency::Weekly => "[weekly Mon-Sun]",
                };
                println!("  {:<26} {:<8} {}", def.name, frequency_label, status);
            }
            println!();
        }
        DhikrCommands::Edit {
            name,
            new_name,
            r#type,
            target,
            freq,
        } => {
            DhikrRepo::edit_custom(
                conn,
                name,
                new_name.as_deref(),
                r#type.as_deref(),
                *target,
                freq.as_deref(),
            )?;
            println_colored!(
                GREEN,
                "  ✓ Updated dhikr: {}",
                new_name.as_deref().unwrap_or(name)
            );
        }
        DhikrCommands::Deactivate { name } => {
            DhikrRepo::set_active(conn, name, false)?;
            println_colored!(GREEN, "  ✓ Deactivated dhikr: {}", name);
        }
        DhikrCommands::Activate { name } => {
            DhikrRepo::set_active(conn, name, true)?;
            println_colored!(GREEN, "  ✓ Activated dhikr: {}", name);
        }
        DhikrCommands::Reorder { name, order } => {
            DhikrRepo::reorder(conn, name, *order)?;
            println_colored!(GREEN, "  ✓ Reordered dhikr: {}", name);
        }
    }
    Ok(())
}

fn toggle_dhikr_by_name(
    conn: &Connection,
    name: &str,
    date: &str,
    extra_count: Option<i32>,
) -> Result<()> {
    let def = DhikrRepo::find_by_name(conn, name)?
        .ok_or_else(|| anyhow!("Dhikr '{}' not found", name))?;

    let log = DhikrRepo::get_log_for_date(conn, date)?;
    let current = log.iter().find(|l| l.dhikr_id == def.id);
    let period_label = match &def.frequency {
        DhikrFrequency::Daily => "today",
        DhikrFrequency::Weekly => "this week (Mon-Sun)",
    };
    if let Some(increment) = extra_count {
        validate_dhikr_increment(increment)?;
    }

    match def.dhikr_type {
        DhikrType::Checkbox => {
            let was_done = current.map(|l| l.completed).unwrap_or(false);
            let now_done = !was_done;
            DhikrRepo::upsert_log(conn, def.id, date, 1, now_done)?;
            if now_done {
                println_colored!(GREEN, "  ✓ {} — done ({})", def.name, period_label);
            } else {
                println_colored!(DIM, "  ○ {} — unmarked ({})", def.name, period_label);
            }
        }
        DhikrType::Counter => {
            let current_count = current.map(|l| l.count).unwrap_or(0);
            let add = extra_count.unwrap_or(1);
            let new_count = current_count
                .checked_add(add)
                .ok_or_else(|| anyhow!("Dhikr count exceeds the supported numeric range"))?;
            let completed = new_count >= def.target_count;
            DhikrRepo::upsert_log(conn, def.id, date, new_count, completed)?;
            if completed {
                println_colored!(
                    GREEN,
                    "  ✓ {} — {}/{} (complete, {})",
                    def.name,
                    new_count,
                    def.target_count,
                    period_label
                );
            } else {
                println_colored!(
                    AMBER,
                    "  ◑ {} — {}/{} ({})",
                    def.name,
                    new_count,
                    def.target_count,
                    period_label
                );
            }
        }
    }
    Ok(())
}

// ─── Quran ───────────────────────────────────────────────────────────────────

pub fn handle_quran(conn: &Connection, config: &AppConfig, pages: f64) -> Result<()> {
    let today = format_date(today_at_offset(config.salah.timezone_offset)?);
    QuranRepo::log_pages(conn, &today, pages)?;
    let total = QuranRepo::get_today(conn, &today)?;
    println_colored!(
        GREEN,
        "  ✓ Logged {} pages — today's total: {}",
        format_pages(pages),
        format_pages(total)
    );
    Ok(())
}

// ─── Stats ───────────────────────────────────────────────────────────────────

pub fn handle_stats(conn: &Connection, config: &AppConfig, week: bool) -> Result<()> {
    let today = today_at_offset(config.salah.timezone_offset)?;
    let today_str = format_date(today);

    // Streak
    let streak = StatsRepo::calculate_streak(conn, today)?;

    // Qada count
    let qada_count = QadaRepo::count_pending(conn)?;

    // Quran this week
    let week_start = today - chrono::Duration::days(6);
    let week_start_str = week_start.format("%Y-%m-%d").to_string();
    let quran_weekly = QuranRepo::get_weekly_total(conn, &week_start_str, &today_str)?;

    println!();
    println_colored!(GOLD, "  Statistics");
    println!();
    println_colored!(
        BOLD,
        "  Streak:      {} days current  |  {} days best",
        streak.current,
        streak.best
    );

    if qada_count == 0 {
        println_colored!(GREEN, "  Qada queue:  0 prayers ✓");
    } else {
        println_colored!(AMBER, "  Qada queue:  {} prayers", qada_count);
    }

    println!("  Quran (7d):  {} pages", format_pages(quran_weekly));

    if week {
        println!();
        println_colored!(
            DIM,
            "  Last 7 days  (● = 5/5, ◕ = 3-4, ◑ = 1-2, ○ = 0/5, · = no records)"
        );
        println!();
        print!("  ");
        let daily = StatsRepo::get_weekly_grid(conn, &week_start_str, &today_str)?;
        for stat in &daily {
            let icon = if stat.prayers_total == 0 {
                format!("{}·\x1b[0m ", DIM)
            } else {
                match stat.prayers_done {
                    5 => format!("{}●\x1b[0m ", GREEN),
                    3 | 4 => format!("{}◕\x1b[0m ", AMBER),
                    1 | 2 => format!("{}◑\x1b[0m ", AMBER),
                    _ => format!("{}○\x1b[0m ", DIM),
                }
            };
            print!("{}", icon);
        }
        println!();
    }

    println!();
    Ok(())
}

// ─── Export ──────────────────────────────────────────────────────────────────

pub fn handle_export(conn: &Connection, config: &AppConfig) -> Result<()> {
    let today = today_at_offset(config.salah.timezone_offset)?;
    let week_start = today - chrono::Duration::days(6);
    let today_str = format_date(today);
    let week_start_str = format_date(week_start);

    let streak = StatsRepo::calculate_streak(conn, today)?;
    let qada_count = QadaRepo::count_pending(conn)?;
    let quran_weekly = QuranRepo::get_weekly_total(conn, &week_start_str, &today_str)?;
    let daily = StatsRepo::get_weekly_grid(conn, &week_start_str, &today_str)?;

    println!("# sujood — Weekly Summary");
    println!("# {}", today_str);
    println!();
    println!("Location: {}", config.salah.location_name);
    println!("Method:   {}", config.salah.calc_method);
    println!();
    println!("## Prayer Completion (last 7 days)");
    for stat in &daily {
        if stat.prayers_total == 0 {
            println!("  {}  No records", stat.date);
            continue;
        }

        let bar = match stat.prayers_done {
            5 => "█████",
            4 => "████░",
            3 => "███░░",
            2 => "██░░░",
            1 => "█░░░░",
            _ => "░░░░░",
        };
        println!(
            "  {}  {}/{}  {}",
            stat.date, stat.prayers_done, stat.prayers_total, bar
        );
    }
    println!();
    println!("## Summary");
    println!(
        "  Streak:     {} days (best: {})",
        streak.current, streak.best
    );
    println!("  Qada owed:  {}", qada_count);
    println!("  Quran (7d): {} pages", format_pages(quran_weekly));
    Ok(())
}

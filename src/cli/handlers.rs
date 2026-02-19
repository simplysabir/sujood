use anyhow::{anyhow, Result};
use chrono::Local;
use rusqlite::Connection;
use std::io::{self, BufRead, Write};
use std::str::FromStr;

use crate::cli::args::{DhikrCommands, QadaCommands};
use crate::config::AppConfig;
use crate::db::repository::{DhikrRepo, MetaRepo, PrayerRepo, QadaRepo, QuranRepo, StatsRepo};
use crate::models::{DhikrType, PrayerType};
use crate::prayer_times::calculator::PrayerCalculator;
use crate::utils::format::{format_duration_secs, format_pages};

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

pub fn handle_setup(
    conn: &Connection,
    config: &mut AppConfig,
    reset: bool,
) -> Result<()> {
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
    let today = Local::now().date_naive();
    let today_str = today.format("%Y-%m-%d").to_string();
    let now_time = Local::now().time();

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
    if let Some((next_prayer, secs)) = calc.get_next_prayer(conn, today, now_time)? {
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
    prayer_str: &str,
    missed: bool,
) -> Result<()> {
    let prayer_type = PrayerType::from_str(prayer_str)
        .map_err(|_| anyhow!("Unknown prayer '{}'. Use: fajr, zuhr, asr, maghrib, isha", prayer_str))?;
    let today = Local::now().date_naive();
    let today_str = today.format("%Y-%m-%d").to_string();

    // Ensure rows exist
    PrayerRepo::ensure_today_rows(conn, &today_str)?;

    if missed {
        PrayerRepo::mark_status(conn, prayer_type.as_str(), &today_str, "missed")?;
        QadaRepo::add_entry(conn, prayer_type.as_str(), &today_str)?;
        println_colored!(
            RED,
            "  ✗ {} marked as missed — added to qada queue",
            prayer_type.display_name()
        );
    } else {
        PrayerRepo::mark_status(conn, prayer_type.as_str(), &today_str, "done")?;
        println_colored!(GREEN, "  ✓ {} marked as done", prayer_type.display_name());
    }
    Ok(())
}

// ─── Qada ────────────────────────────────────────────────────────────────────

pub fn handle_qada(conn: &Connection, action: &QadaCommands) -> Result<()> {
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
        QadaCommands::Add { prayer } => {
            let prayer_type = PrayerType::from_str(prayer)
                .map_err(|_| anyhow!("Unknown prayer '{}'", prayer))?;
            let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
            QadaRepo::add_entry(conn, prayer_type.as_str(), &today)?;
            println_colored!(AMBER, "  Added {} to qada queue", prayer_type.display_name());
        }
    }
    Ok(())
}

// ─── Dhikr ───────────────────────────────────────────────────────────────────

pub fn handle_dhikr(conn: &Connection, action: &DhikrCommands) -> Result<()> {
    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();

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
                let (count, completed) = log
                    .map(|l| (l.count, l.completed))
                    .unwrap_or((0, false));
                let status = if completed {
                    format!("{}✓\x1b[0m", GREEN)
                } else {
                    match def.dhikr_type {
                        DhikrType::Counter => {
                            format!("{}/{}", count, def.target_count)
                        }
                        DhikrType::Checkbox => format!("○"),
                    }
                };
                println!("  {:<30}  {}", def.name, status);
            }
            println!();
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

    match def.dhikr_type {
        DhikrType::Checkbox => {
            let was_done = current.map(|l| l.completed).unwrap_or(false);
            let now_done = !was_done;
            DhikrRepo::upsert_log(conn, def.id, date, 1, now_done)?;
            if now_done {
                println_colored!(GREEN, "  ✓ {} — done", def.name);
            } else {
                println_colored!(DIM, "  ○ {} — unmarked", def.name);
            }
        }
        DhikrType::Counter => {
            let current_count = current.map(|l| l.count).unwrap_or(0);
            let add = extra_count.unwrap_or(1);
            let new_count = current_count + add;
            let completed = new_count >= def.target_count;
            DhikrRepo::upsert_log(conn, def.id, date, new_count, completed)?;
            if completed {
                println_colored!(
                    GREEN,
                    "  ✓ {} — {}/{} (complete!)",
                    def.name,
                    new_count,
                    def.target_count
                );
            } else {
                println_colored!(
                    AMBER,
                    "  ◑ {} — {}/{}",
                    def.name,
                    new_count,
                    def.target_count
                );
            }
        }
    }
    Ok(())
}

// ─── Quran ───────────────────────────────────────────────────────────────────

pub fn handle_quran(conn: &Connection, pages: f64) -> Result<()> {
    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
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

pub fn handle_stats(conn: &Connection, week: bool) -> Result<()> {
    let today = Local::now().date_naive();
    let today_str = today.format("%Y-%m-%d").to_string();

    // Streak
    let streak = StatsRepo::calculate_streak(conn)?;

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

    println!(
        "  Quran (7d):  {} pages",
        format_pages(quran_weekly)
    );

    if week {
        println!();
        println_colored!(DIM, "  Last 7 days  (● = 5/5, ◕ = 3-4, ◑ = 1-2, ○ = 0/5)");
        println!();
        print!("  ");
        let daily = StatsRepo::get_weekly_grid(conn, &week_start_str, &today_str)?;
        for stat in &daily {
            let icon = match stat.prayers_done {
                5 => format!("{}●\x1b[0m ", GREEN),
                3 | 4 => format!("{}◕\x1b[0m ", AMBER),
                1 | 2 => format!("{}◑\x1b[0m ", AMBER),
                _ => format!("{}○\x1b[0m ", DIM),
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
    let today = Local::now().date_naive();
    let week_start = today - chrono::Duration::days(6);
    let today_str = today.format("%Y-%m-%d").to_string();
    let week_start_str = week_start.format("%Y-%m-%d").to_string();

    let streak = StatsRepo::calculate_streak(conn)?;
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
        let bar = match stat.prayers_done {
            5 => "█████",
            4 => "████░",
            3 => "███░░",
            2 => "██░░░",
            1 => "█░░░░",
            _ => "░░░░░",
        };
        println!("  {}  {}/5  {}", stat.date, stat.prayers_done, bar);
    }
    println!();
    println!("## Summary");
    println!("  Streak:     {} days (best: {})", streak.current, streak.best);
    println!("  Qada owed:  {}", qada_count);
    println!("  Quran (7d): {} pages", format_pages(quran_weekly));
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn prompt(message: &str) -> Result<String> {
    print!("{}", message);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().lock().read_line(&mut buf)?;
    Ok(buf.trim_end_matches('\n').trim_end_matches('\r').to_string())
}

/// Parse a UTC offset string into total minutes.
/// Accepts: "5:30", "+5:30", "-5:30", "5", "+5", "5.5"
fn parse_tz_offset(s: &str) -> Result<i32> {
    let s = s.trim_start_matches('+');
    let negative = s.starts_with('-');
    let s = s.trim_start_matches('-');
    let sign = if negative { -1 } else { 1 };

    let minutes = if s.contains(':') {
        let mut parts = s.splitn(2, ':');
        let hours: i32 = parts.next().unwrap_or("0").parse()?;
        let mins: i32 = parts.next().unwrap_or("0").parse()?;
        hours * 60 + mins
    } else if s.contains('.') {
        let hours: f64 = s.parse()?;
        (hours * 60.0).round() as i32
    } else {
        let hours: i32 = s.parse()?;
        hours * 60
    };

    Ok(sign * minutes)
}

/// Format total minutes as "+H:MM" string
fn format_tz_offset(minutes: i32) -> String {
    let sign = if minutes < 0 { "-" } else { "+" };
    let abs = minutes.abs();
    let h = abs / 60;
    let m = abs % 60;
    if m == 0 {
        format!("{}{}",sign, h)
    } else {
        format!("{}{}:{:02}", sign, h, m)
    }
}

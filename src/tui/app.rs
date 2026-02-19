use anyhow::Result;
use chrono::Local;
use crossterm::event::{KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};
use rusqlite::Connection;
use std::collections::HashMap;

use crate::config::AppConfig;
use crate::db::repository::{DhikrRepo, PrayerRepo, QadaRepo, QuranRepo, StatsRepo};
use crate::models::{DailyStats, DhikrDef, DhikrLog, DhikrType, Prayer, PrayerType, Streak};
use crate::utils::hijri::today_hijri_string;
use crate::prayer_times::PrayerCalculator;
use crate::tui::events::{Event, EventHandler};
use crate::tui::theme;
use crate::tui::widgets::{adhkar, header, next_prayer, prayers, qada, quran, statusbar, streak};

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Dashboard,
    Stats,
    Help,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FocusSection {
    Prayers,
    Dhikr,
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    QuranInput,
}

pub struct App {
    pub view: View,
    pub config: AppConfig,
    pub focus_section: FocusSection,
    pub focus_idx: usize,
    pub should_quit: bool,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub input_error: Option<String>,   // shown in quran popup on bad input
    pub show_qada_overlay: bool,       // `q` toggles this

    // Cached state (refreshed on tick/action)
    pub today_str: String,
    pub hijri_str: String,
    pub prayers: Vec<Prayer>,
    pub dhikr_defs: Vec<DhikrDef>,
    pub dhikr_logs: HashMap<i64, DhikrLog>,
    pub qada_count: i64,
    pub quran_today: f64,
    pub quran_weekly: f64,
    pub streak: Streak,
    pub weekly_grid: Vec<DailyStats>,
    pub next_prayer_info: Option<(PrayerType, i64)>,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let today = Local::now().date_naive();
        let today_str = today.format("%Y-%m-%d").to_string();
        let hijri_str = today_hijri_string(config.salah.hijri_offset);

        App {
            view: View::Dashboard,
            config,
            focus_section: FocusSection::Prayers,
            focus_idx: 0,
            should_quit: false,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            input_error: None,
            show_qada_overlay: false,
            today_str,
            hijri_str,
            prayers: Vec::new(),
            dhikr_defs: Vec::new(),
            dhikr_logs: HashMap::new(),
            qada_count: 0,
            quran_today: 0.0,
            quran_weekly: 0.0,
            streak: Streak::default(),
            weekly_grid: Vec::new(),
            next_prayer_info: None,
        }
    }

    pub fn load(&mut self, conn: &Connection) -> Result<()> {
        // Ensure today's prayer rows exist
        PrayerRepo::ensure_today_rows(conn, &self.today_str)?;

        // Load prayers + times from cache
        let calc = self.make_calculator()?;
        let today = Local::now().date_naive();
        let cached_times = calc.get_cached_or_compute(conn, today).ok();

        let mut db_prayers = PrayerRepo::get_by_date(conn, &self.today_str)?;
        if let Some(times) = &cached_times {
            for p in &mut db_prayers {
                p.time = Some(match p.prayer_type {
                    PrayerType::Fajr => times.fajr,
                    PrayerType::Zuhr => times.zuhr,
                    PrayerType::Asr => times.asr,
                    PrayerType::Maghrib => times.maghrib,
                    PrayerType::Isha => times.isha,
                });
            }
        }
        self.prayers = db_prayers;

        // Dhikr
        self.dhikr_defs = DhikrRepo::get_active_definitions(conn)?;
        let logs = DhikrRepo::get_log_for_date(conn, &self.today_str)?;
        self.dhikr_logs = logs.into_iter().map(|l| (l.dhikr_id, l)).collect();

        // Qada
        self.qada_count = QadaRepo::count_pending(conn)?;

        // Quran
        self.quran_today = QuranRepo::get_today(conn, &self.today_str)?;
        let week_start = (Local::now().date_naive() - chrono::Duration::days(6))
            .format("%Y-%m-%d")
            .to_string();
        self.quran_weekly = QuranRepo::get_weekly_total(conn, &week_start, &self.today_str)?;

        // Streak
        self.streak = StatsRepo::calculate_streak(conn)?;

        // Weekly grid
        let week_end = &self.today_str;
        self.weekly_grid = StatsRepo::get_weekly_grid(conn, &week_start, week_end)?;

        // Next prayer
        let now_time = Local::now().time();
        self.next_prayer_info = calc
            .get_next_prayer(conn, today, now_time)
            .ok()
            .flatten();

        Ok(())
    }

    pub fn tick(&mut self, conn: &Connection) {
        // Refresh countdown
        let today = Local::now().date_naive();
        let now_time = Local::now().time();
        if let Ok(calc) = self.make_calculator() {
            self.next_prayer_info = calc
                .get_next_prayer(conn, today, now_time)
                .ok()
                .flatten();
        }
    }

    fn make_calculator(&self) -> Result<PrayerCalculator> {
        PrayerCalculator::new(
            self.config.salah.latitude,
            self.config.salah.longitude,
            &self.config.salah.calc_method,
            &self.config.salah.madhab,
            self.config.salah.timezone_offset,
        )
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent, conn: &Connection) {
        // Only handle actual key presses — ignore release/repeat events from some terminals
        if key.kind != KeyEventKind::Press {
            return;
        }
        match self.input_mode {
            InputMode::QuranInput => self.handle_quran_input(key, conn),
            InputMode::Normal => self.handle_normal_key(key, conn),
        }
    }

    fn handle_normal_key(&mut self, key: crossterm::event::KeyEvent, conn: &Connection) {
        match self.view {
            View::Dashboard => self.handle_dashboard_key(key, conn),
            View::Stats => self.handle_stats_key(key),
            View::Help => self.handle_help_key(key),
        }
    }

    fn handle_dashboard_key(&mut self, key: crossterm::event::KeyEvent, conn: &Connection) {
        // If qada overlay is open, any key closes it (q toggles, others dismiss)
        if self.show_qada_overlay {
            self.show_qada_overlay = false;
            return;
        }

        match key.code {
            // Esc = quit, q = qada overlay (they are different)
            KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char('q') => {
                self.show_qada_overlay = true;
            }
            KeyCode::Char('?') => {
                self.view = View::Help;
            }
            KeyCode::Char('s') => {
                self.view = View::Stats;
            }
            KeyCode::Char('r') => {
                self.input_mode = InputMode::QuranInput;
                self.input_buffer.clear();
                self.input_error = None;
            }
            KeyCode::Up => {
                if self.focus_idx > 0 {
                    self.focus_idx -= 1;
                }
            }
            KeyCode::Down => {
                let max = match self.focus_section {
                    FocusSection::Prayers => self.prayers.len().saturating_sub(1),
                    FocusSection::Dhikr => self.dhikr_defs.len().saturating_sub(1),
                    FocusSection::None => 0,
                };
                if self.focus_idx < max {
                    self.focus_idx += 1;
                }
            }
            KeyCode::Tab => {
                self.focus_section = match self.focus_section {
                    FocusSection::Prayers => FocusSection::Dhikr,
                    FocusSection::Dhikr => FocusSection::Prayers,
                    FocusSection::None => FocusSection::Prayers,
                };
                self.focus_idx = 0;
            }
            // m / Enter marks focused prayer done (only when in Prayers section)
            KeyCode::Char('m') | KeyCode::Enter => {
                if self.focus_section == FocusSection::Prayers {
                    self.mark_focused_done(conn);
                }
            }
            KeyCode::Char('M') => {
                if self.focus_section == FocusSection::Prayers {
                    self.mark_focused_missed(conn);
                }
            }
            // d always works on dhikr — auto-switches to Dhikr section if needed
            KeyCode::Char('d') => {
                if self.focus_section != FocusSection::Dhikr {
                    self.focus_section = FocusSection::Dhikr;
                    self.focus_idx = 0;
                }
                self.toggle_focused_dhikr(conn);
            }
            _ => {}
        }
    }

    fn handle_stats_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('s') => {
                self.view = View::Dashboard;
            }
            _ => {}
        }
    }

    fn handle_help_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => {
                self.view = View::Dashboard;
            }
            _ => {}
        }
    }

    fn handle_quran_input(&mut self, key: crossterm::event::KeyEvent, conn: &Connection) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
                self.input_error = None;
            }
            KeyCode::Enter => {
                let trimmed = self.input_buffer.trim().to_string();
                if trimmed.is_empty() {
                    self.input_error = Some("Enter a number first (e.g. 2 or 0.5)".to_string());
                    return;
                }
                match trimmed.parse::<f64>() {
                    Ok(pages) if pages > 0.0 => {
                        let _ = QuranRepo::log_pages(conn, &self.today_str, pages);
                        let _ = self.load(conn);
                        self.input_mode = InputMode::Normal;
                        self.input_buffer.clear();
                        self.input_error = None;
                    }
                    Ok(_) => {
                        self.input_error = Some("Pages must be greater than 0".to_string());
                    }
                    Err(_) => {
                        self.input_error = Some(format!("'{}' is not a valid number", trimmed));
                    }
                }
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
                self.input_error = None;
            }
            KeyCode::Char(c) if c.is_ascii_digit() || c == '.' => {
                self.input_buffer.push(c);
                self.input_error = None;
            }
            _ => {}
        }
    }

    fn mark_focused_done(&mut self, conn: &Connection) {
        if self.focus_section == FocusSection::Prayers {
            if let Some(prayer) = self.prayers.get(self.focus_idx) {
                let _ = PrayerRepo::mark_status(
                    conn,
                    prayer.prayer_type.as_str(),
                    &self.today_str,
                    "done",
                );
                let _ = self.load(conn);
            }
        }
    }

    fn mark_focused_missed(&mut self, conn: &Connection) {
        if self.focus_section == FocusSection::Prayers {
            if let Some(prayer) = self.prayers.get(self.focus_idx) {
                let prayer_type = prayer.prayer_type.as_str().to_string();
                let date = self.today_str.clone();
                let _ = PrayerRepo::mark_status(conn, &prayer_type, &date, "missed");
                let _ = QadaRepo::add_entry(conn, &prayer_type, &date);
                let _ = self.load(conn);
            }
        }
    }

    fn toggle_focused_dhikr(&mut self, conn: &Connection) {
        // focus_section is guaranteed to be Dhikr by the caller
        if let Some(def) = self.dhikr_defs.get(self.focus_idx) {
            let log = self.dhikr_logs.get(&def.id);
            match def.dhikr_type {
                DhikrType::Checkbox => {
                    let was_done = log.map(|l| l.completed).unwrap_or(false);
                    let _ = DhikrRepo::upsert_log(conn, def.id, &self.today_str, 1, !was_done);
                }
                DhikrType::Counter => {
                    let count = log.map(|l| l.count).unwrap_or(0) + 1;
                    let completed = count >= def.target_count;
                    let _ = DhikrRepo::upsert_log(conn, def.id, &self.today_str, count, completed);
                }
            }
            let _ = self.load(conn);
        }
    }

    pub fn draw(&self, frame: &mut Frame) {
        match self.view {
            View::Dashboard => self.draw_dashboard(frame),
            View::Stats => self.draw_stats(frame),
            View::Help => {
                self.draw_dashboard(frame);
                self.draw_help_overlay(frame);
            }
        }

        if self.input_mode == InputMode::QuranInput {
            self.draw_quran_input(frame);
        }

        if self.show_qada_overlay {
            self.draw_qada_overlay(frame);
        }
    }

    fn draw_dashboard(&self, frame: &mut Frame) {
        let area = frame.area();

        // Clear background
        frame.render_widget(
            Block::default().style(theme::base()),
            area,
        );

        let outer_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5), // header
                Constraint::Min(0),    // body
                Constraint::Length(1), // status bar
            ])
            .split(area);

        // Header
        header::render(frame, outer_chunks[0], &self.hijri_str);

        // Status bar
        statusbar::render(frame, outer_chunks[2]);

        // Body split into columns
        let body = outer_chunks[1];
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(body);

        let left = columns[0];
        let right = columns[1];

        // Left column: Prayers + Adhkar + Quran
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(9),  // prayers
                Constraint::Length(8),  // adhkar
                Constraint::Length(3),  // quran
            ])
            .split(left);

        let focused_prayers = self.focus_section == FocusSection::Prayers;
        let focused_dhikr = self.focus_section == FocusSection::Dhikr;

        prayers::render(
            frame,
            left_chunks[0],
            &self.prayers,
            self.focus_idx,
            focused_prayers,
        );

        adhkar::render(
            frame,
            left_chunks[1],
            &self.dhikr_defs,
            &self.dhikr_logs,
            self.focus_idx,
            focused_dhikr,
        );

        quran::render(
            frame,
            left_chunks[2],
            self.quran_today,
            self.quran_weekly,
            self.config.quran.daily_target,
        );

        // Right column: Next Prayer + Streak + Qada
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(9),  // next prayer
                Constraint::Length(7),  // streak
                Constraint::Min(0),     // qada
            ])
            .split(right);

        next_prayer::render(frame, right_chunks[0], self.next_prayer_info.as_ref());
        streak::render(frame, right_chunks[1], &self.streak, &self.weekly_grid);
        qada::render(frame, right_chunks[2], self.qada_count);
    }

    fn draw_stats(&self, frame: &mut Frame) {
        let area = frame.area();
        frame.render_widget(Block::default().style(theme::base()), area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(area);

        // Simple title
        let title = Paragraph::new(Line::from(vec![
            Span::styled("  Stats  ", theme::gold().add_modifier(Modifier::BOLD)),
            Span::styled("  [Esc] back", theme::dim()),
        ]));
        frame.render_widget(title, chunks[0]);

        // Stats content
        let lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Streak (current):  ", theme::dim()),
                Span::styled(
                    format!("{} days", self.streak.current),
                    theme::green().add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Streak (best):     ", theme::dim()),
                Span::styled(format!("{} days", self.streak.best), theme::green()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Qada owed:         ", theme::dim()),
                Span::styled(format!("{}", self.qada_count), theme::amber()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Quran today:       ", theme::dim()),
                Span::styled(
                    format!("{} pages", self.quran_today),
                    theme::amber(),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Quran this week:   ", theme::dim()),
                Span::styled(format!("{} pages", self.quran_weekly), theme::amber()),
            ]),
            Line::from(""),
            Line::from(Span::styled("  Last 7 Days", theme::gold())),
            Line::from(""),
        ];

        let mut all_lines = lines;

        // Weekly heatmap
        for stat in &self.weekly_grid {
            let icon = match stat.prayers_done {
                5 => Span::styled("  ████████████  ", theme::green()),
                4 => Span::styled("  █████████░░░  ", theme::green()),
                3 => Span::styled("  ████████░░░░  ", theme::amber()),
                2 => Span::styled("  █████░░░░░░░  ", theme::amber()),
                1 => Span::styled("  ███░░░░░░░░░  ", theme::dim()),
                _ => Span::styled("  ░░░░░░░░░░░░  ", theme::dim()),
            };
            all_lines.push(Line::from(vec![
                icon,
                Span::styled(
                    format!("{}  {}/5", stat.date, stat.prayers_done),
                    theme::dim(),
                ),
            ]));
        }

        let paragraph = Paragraph::new(all_lines);
        frame.render_widget(paragraph, chunks[1]);
    }

    fn draw_help_overlay(&self, frame: &mut Frame) {
        let area = frame.area();

        // Center a help box
        let popup_area = Rect {
            x: area.width / 4,
            y: area.height / 4,
            width: area.width / 2,
            height: area.height / 2,
        };

        frame.render_widget(Clear, popup_area);

        let help_text = vec![
            Line::from(Span::styled(
                "  Keybindings",
                theme::gold().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("  [m] / Enter  ", theme::gold()),
                Span::styled("Mark prayer done", theme::dim()),
            ]),
            Line::from(vec![
                Span::styled("  [M]          ", theme::gold()),
                Span::styled("Mark prayer missed + qada", theme::dim()),
            ]),
            Line::from(vec![
                Span::styled("  [d]          ", theme::gold()),
                Span::styled("Toggle / increment dhikr", theme::dim()),
            ]),
            Line::from(vec![
                Span::styled("  [r]          ", theme::gold()),
                Span::styled("Log Quran pages", theme::dim()),
            ]),
            Line::from(vec![
                Span::styled("  [s]          ", theme::gold()),
                Span::styled("Stats view", theme::dim()),
            ]),
            Line::from(vec![
                Span::styled("  [Tab]        ", theme::gold()),
                Span::styled("Switch focus section", theme::dim()),
            ]),
            Line::from(vec![
                Span::styled("  [↑ ↓]        ", theme::gold()),
                Span::styled("Navigate items", theme::dim()),
            ]),
            Line::from(vec![
                Span::styled("  [?]          ", theme::gold()),
                Span::styled("Toggle help", theme::dim()),
            ]),
            Line::from(vec![
                Span::styled("  [Esc]        ", theme::gold()),
                Span::styled("Quit", theme::dim()),
            ]),
        ];

        let block = Block::default()
            .title(Span::styled(" Help ", theme::gold()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::gold())
            .style(theme::surface());

        let paragraph = Paragraph::new(help_text).block(block);
        frame.render_widget(paragraph, popup_area);
    }

    fn draw_quran_input(&self, frame: &mut Frame) {
        let area = frame.area();
        let height = if self.input_error.is_some() { 7 } else { 5 };

        let popup_area = Rect {
            x: area.width / 4,
            y: area.height / 2 - 3,
            width: area.width / 2,
            height,
        };

        frame.render_widget(Clear, popup_area);

        let mut text = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Pages read today: ", theme::dim()),
                Span::styled(self.input_buffer.as_str(), theme::gold().add_modifier(Modifier::BOLD)),
                Span::styled("█", theme::amber()),  // block cursor
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "  Type a number, then [Enter]  ·  [Esc] cancel",
                theme::dim(),
            )),
        ];

        if let Some(err) = &self.input_error {
            text.push(Line::from(""));
            text.push(Line::from(Span::styled(
                format!("  ✗ {}", err),
                theme::red(),
            )));
        }

        let border_style = if self.input_error.is_some() {
            theme::red()
        } else {
            theme::amber()
        };

        let block = Block::default()
            .title(Span::styled(" Log Quran Pages ", theme::gold()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style)
            .style(theme::surface());

        let paragraph = Paragraph::new(text).block(block);
        frame.render_widget(paragraph, popup_area);
    }

    fn draw_qada_overlay(&self, frame: &mut Frame) {
        let area = frame.area();

        let popup_area = Rect {
            x: area.width / 4,
            y: area.height / 4,
            width: area.width / 2,
            height: (area.height / 2).min(20),
        };

        frame.render_widget(Clear, popup_area);

        let mut lines = vec![Line::from("")];

        if self.qada_count == 0 {
            lines.push(Line::from(vec![
                Span::styled("  ", theme::dim()),
                Span::styled("✓ No qada prayers owed", theme::green()),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("  ", theme::dim()),
                Span::styled(
                    format!("{} prayers owed", self.qada_count),
                    theme::amber().add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Use `sujood qada list` to see details",
                theme::dim(),
            )));
            lines.push(Line::from(Span::styled(
                "  Use `sujood qada complete` to mark one done",
                theme::dim(),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  At 1/day: ~{} days to clear", self.qada_count),
                theme::dim(),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  [any key] close",
            theme::dim(),
        )));

        let block = Block::default()
            .title(Span::styled(" Qada Queue ", theme::gold()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::amber())
            .style(theme::surface());

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, popup_area);
    }
}

/// Run the TUI event loop.
pub fn run(conn: Connection, config: AppConfig) -> Result<()> {
    let mut app = App::new(config);
    app.load(&conn)?;

    let mut terminal = ratatui::init();
    let events = EventHandler::new(500);

    loop {
        terminal.draw(|frame| app.draw(frame))?;

        match events.next()? {
            Event::Key(key) => {
                app.handle_key(key, &conn);
                if app.should_quit {
                    break;
                }
            }
            Event::Tick => {
                app.tick(&conn);
            }
        }
    }

    ratatui::restore();
    Ok(())
}

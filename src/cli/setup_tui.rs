use anyhow::Result;
use crossterm::event::{KeyCode, KeyEventKind};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};
use rusqlite::Connection;

use crate::config::AppConfig;
use crate::db::repository::CacheRepo;
use crate::prayer_times::calculator::{PrayerCalculator, CALC_METHODS};
use crate::tui::theme;
use crate::tui::events::{Event, EventHandler};

// ─── Wizard steps ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Step {
    Welcome,
    LocationName,
    Latitude,
    Longitude,
    CalcMethod,
    Madhab,
    TimezoneOffset,
    HijriOffset,
    Confirm,
}

// ─── Wizard state ─────────────────────────────────────────────────────────────

struct SetupWizard {
    step: Step,
    input: String,
    error: Option<String>,
    list_state: ListState,

    // Collected values
    location_name: String,
    latitude: f64,
    longitude: f64,
    method_idx: usize,
    madhab_idx: usize, // 0 = Hanafi, 1 = Shafi
    tz_minutes: i32,
    hijri_idx: usize, // 0 = 0 days, 1 = -1 day

    should_quit: bool,
    confirmed: bool,
}

impl SetupWizard {
    fn new(existing: &AppConfig) -> Self {
        let method_idx = CALC_METHODS
            .iter()
            .position(|m| *m == existing.salah.calc_method)
            .unwrap_or(0);
        let madhab_idx = if existing.salah.madhab == "Shafi" { 1 } else { 0 };
        let hijri_idx = if existing.salah.hijri_offset < 0 { 1 } else { 0 };

        let mut list_state = ListState::default();
        list_state.select(Some(method_idx));

        Self {
            step: Step::Welcome,
            input: String::new(),
            error: None,
            list_state,

            location_name: existing.salah.location_name.clone(),
            latitude: existing.salah.latitude,
            longitude: existing.salah.longitude,
            method_idx,
            madhab_idx,
            tz_minutes: existing.salah.timezone_offset,
            hijri_idx,

            should_quit: false,
            confirmed: false,
        }
    }

    fn step_number(&self) -> usize {
        match self.step {
            Step::Welcome => 0,
            Step::LocationName => 1,
            Step::Latitude => 2,
            Step::Longitude => 3,
            Step::CalcMethod => 4,
            Step::Madhab => 5,
            Step::TimezoneOffset => 6,
            Step::HijriOffset => 7,
            Step::Confirm => 8,
        }
    }

    const TOTAL_STEPS: usize = 8;

    fn advance(&mut self) {
        self.error = None;
        self.step = match self.step {
            Step::Welcome => Step::LocationName,
            Step::LocationName => Step::Latitude,
            Step::Latitude => Step::Longitude,
            Step::Longitude => Step::CalcMethod,
            Step::CalcMethod => Step::Madhab,
            Step::Madhab => Step::TimezoneOffset,
            Step::TimezoneOffset => Step::HijriOffset,
            Step::HijriOffset => Step::Confirm,
            Step::Confirm => {
                self.confirmed = true;
                Step::Confirm
            }
        };
        // Pre-fill input with current value when entering a text step
        self.input = match self.step {
            Step::LocationName => self.location_name.clone(),
            Step::Latitude => format!("{}", self.latitude),
            Step::Longitude => format!("{}", self.longitude),
            Step::TimezoneOffset => format_tz(self.tz_minutes),
            _ => String::new(),
        };
    }

    fn go_back(&mut self) {
        self.error = None;
        self.step = match self.step {
            Step::Welcome => {
                self.should_quit = true;
                Step::Welcome
            }
            Step::LocationName => Step::Welcome,
            Step::Latitude => Step::LocationName,
            Step::Longitude => Step::Latitude,
            Step::CalcMethod => Step::Longitude,
            Step::Madhab => Step::CalcMethod,
            Step::TimezoneOffset => Step::Madhab,
            Step::HijriOffset => Step::TimezoneOffset,
            Step::Confirm => Step::HijriOffset,
        };
        self.input = match self.step {
            Step::LocationName => self.location_name.clone(),
            Step::Latitude => format!("{}", self.latitude),
            Step::Longitude => format!("{}", self.longitude),
            Step::TimezoneOffset => format_tz(self.tz_minutes),
            _ => String::new(),
        };
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        match &self.step {
            Step::Welcome => {
                if key.code == KeyCode::Esc {
                    self.should_quit = true;
                } else {
                    self.advance();
                }
            }

            Step::LocationName => self.handle_text_input(key, |s| {
                if s.trim().is_empty() {
                    Err("Please enter a city name".to_string())
                } else {
                    Ok(())
                }
            }),

            Step::Latitude => self.handle_text_input(key, |s| {
                s.parse::<f64>()
                    .map_err(|_| "Enter a valid latitude (e.g. 19.0748)".to_string())
                    .and_then(|v| {
                        if v < -90.0 || v > 90.0 {
                            Err("Latitude must be between -90 and 90".to_string())
                        } else {
                            Ok(())
                        }
                    })
            }),

            Step::Longitude => self.handle_text_input(key, |s| {
                s.parse::<f64>()
                    .map_err(|_| "Enter a valid longitude (e.g. 72.8856)".to_string())
                    .and_then(|v| {
                        if v < -180.0 || v > 180.0 {
                            Err("Longitude must be between -180 and 180".to_string())
                        } else {
                            Ok(())
                        }
                    })
            }),

            Step::CalcMethod => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if self.method_idx > 0 {
                        self.method_idx -= 1;
                        self.list_state.select(Some(self.method_idx));
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if self.method_idx + 1 < CALC_METHODS.len() {
                        self.method_idx += 1;
                        self.list_state.select(Some(self.method_idx));
                    }
                }
                KeyCode::Enter => self.advance(),
                KeyCode::Esc => self.go_back(),
                _ => {}
            },

            Step::Madhab => match key.code {
                KeyCode::Left | KeyCode::Char('1') | KeyCode::Char('h') => {
                    self.madhab_idx = 0;
                }
                KeyCode::Right | KeyCode::Char('2') | KeyCode::Char('l') => {
                    self.madhab_idx = 1;
                }
                KeyCode::Enter => self.advance(),
                KeyCode::Esc => self.go_back(),
                _ => {}
            },

            Step::TimezoneOffset => self.handle_text_input(key, |s| {
                parse_tz(s).map(|_| ()).map_err(|_| {
                    "Use format like +5:30, -3, or +5.5".to_string()
                })
            }),

            Step::HijriOffset => match key.code {
                KeyCode::Left | KeyCode::Char('1') | KeyCode::Char('h') => {
                    self.hijri_idx = 0;
                }
                KeyCode::Right | KeyCode::Char('2') | KeyCode::Char('l') => {
                    self.hijri_idx = 1;
                }
                KeyCode::Enter => self.advance(),
                KeyCode::Esc => self.go_back(),
                _ => {}
            },

            Step::Confirm => match key.code {
                KeyCode::Enter | KeyCode::Char('y') => {
                    self.confirmed = true;
                }
                KeyCode::Esc | KeyCode::Char('n') => self.go_back(),
                _ => {}
            },
        }
    }

    fn handle_text_input<F>(&mut self, key: crossterm::event::KeyEvent, validate: F)
    where
        F: Fn(&str) -> std::result::Result<(), String>,
    {
        match key.code {
            KeyCode::Esc => self.go_back(),
            KeyCode::Enter => {
                let val = self.input.trim().to_string();
                match validate(&val) {
                    Ok(()) => {
                        self.commit_text_input(&val);
                        self.advance();
                    }
                    Err(e) => {
                        self.error = Some(e);
                    }
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.error = None;
            }
            KeyCode::Tab => {
                // Reset to default value for this step
                self.input = match self.step {
                    Step::LocationName => "Mumbai".to_string(),
                    Step::Latitude => "19.0748".to_string(),
                    Step::Longitude => "72.8856".to_string(),
                    Step::TimezoneOffset => "+5:30".to_string(),
                    _ => self.input.clone(),
                };
                self.error = None;
            }
            KeyCode::Char(c) => {
                self.input.push(c);
                self.error = None;
            }
            _ => {}
        }
    }

    fn commit_text_input(&mut self, val: &str) {
        match self.step {
            Step::LocationName => {
                self.location_name = val.to_string();
            }
            Step::Latitude => {
                self.latitude = val.parse().unwrap_or(self.latitude);
            }
            Step::Longitude => {
                self.longitude = val.parse().unwrap_or(self.longitude);
            }
            Step::TimezoneOffset => {
                self.tz_minutes = parse_tz(val).unwrap_or(self.tz_minutes);
            }
            _ => {}
        }
    }

    fn build_config(&self, existing: &AppConfig) -> AppConfig {
        let mut config = existing.clone();
        config.salah.location_name = self.location_name.clone();
        config.salah.latitude = self.latitude;
        config.salah.longitude = self.longitude;
        config.salah.calc_method = CALC_METHODS[self.method_idx].to_string();
        config.salah.madhab = if self.madhab_idx == 0 {
            "Hanafi".to_string()
        } else {
            "Shafi".to_string()
        };
        config.salah.timezone_offset = self.tz_minutes;
        config.salah.hijri_offset = if self.hijri_idx == 0 { 0 } else { -1 };
        config
    }
}

// ─── Rendering ────────────────────────────────────────────────────────────────

fn draw(frame: &mut Frame, wizard: &mut SetupWizard) {
    let area = frame.area();

    // Dark background
    frame.render_widget(Block::default().style(theme::base()), area);

    // Center the wizard box
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(26),
            Constraint::Min(0),
        ])
        .split(area);

    let hchunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(64),
            Constraint::Min(0),
        ])
        .split(vchunks[1]);

    let box_area = hchunks[1];
    frame.render_widget(Clear, box_area);

    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::gold())
        .style(theme::surface())
        .title(Span::styled(
            "  سُجُود  sujood  —  Setup  ",
            theme::gold().add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center);

    frame.render_widget(outer_block, box_area);

    // Inner area
    let inner = Rect {
        x: box_area.x + 2,
        y: box_area.y + 1,
        width: box_area.width.saturating_sub(4),
        height: box_area.height.saturating_sub(2),
    };

    match wizard.step {
        Step::Welcome => draw_welcome(frame, inner),
        Step::CalcMethod => draw_method_list(frame, inner, wizard),
        Step::Madhab => draw_choice(
            frame,
            inner,
            4,
            "Madhab",
            "Affects Asr prayer time calculation",
            &["Hanafi  (later Asr)", "Shafi  (earlier Asr)"],
            wizard.madhab_idx,
            &wizard.error,
        ),
        Step::HijriOffset => draw_choice(
            frame,
            inner,
            7,
            "Hijri Date",
            "When does your region start each Islamic month?",
            &[
                "Same day as astronomical calculation",
                "One day after (local moon sighting — common in South Asia)",
            ],
            wizard.hijri_idx,
            &wizard.error,
        ),
        Step::Confirm => draw_confirm(frame, inner, wizard),
        _ => draw_text_step(frame, inner, wizard),
    }

    // Progress dots at the top of inner
    draw_progress(frame, inner, wizard.step_number(), SetupWizard::TOTAL_STEPS);
}

fn draw_progress(frame: &mut Frame, area: Rect, current: usize, total: usize) {
    let mut spans = vec![Span::styled("  ", theme::dim())];
    for i in 1..=total {
        if i < current {
            spans.push(Span::styled("● ", theme::green()));
        } else if i == current {
            spans.push(Span::styled("◉ ", theme::gold()));
        } else {
            spans.push(Span::styled("○ ", theme::dim()));
        }
    }
    let line = Line::from(spans);
    let para = Paragraph::new(line);
    let progress_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    frame.render_widget(para, progress_area);
}

fn draw_welcome(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "بِسۡمِ ٱللَّهِ ٱلرَّحۡمَٰنِ ٱلرَّحِيمِ",
            theme::gold().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "Welcome to sujood",
            theme::bold().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "A quiet terminal companion for your daily Islamic practice.",
            theme::dim(),
        )),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "This wizard will configure:",
            theme::dim(),
        )),
        Line::from(vec![
            Span::styled("  ●  ", theme::gold()),
            Span::styled("Your location for accurate prayer times", theme::dim()),
        ]),
        Line::from(vec![
            Span::styled("  ●  ", theme::gold()),
            Span::styled("Calculation method and madhab", theme::dim()),
        ]),
        Line::from(vec![
            Span::styled("  ●  ", theme::gold()),
            Span::styled("Timezone and Hijri date preference", theme::dim()),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "Press  Enter  to begin  ·  Esc  to cancel",
            theme::dim(),
        )),
    ];

    let para = Paragraph::new(lines).alignment(Alignment::Center);
    let content_area = Rect {
        x: area.x,
        y: area.y + 2,
        width: area.width,
        height: area.height.saturating_sub(2),
    };
    frame.render_widget(para, content_area);
}

fn draw_text_step(frame: &mut Frame, area: Rect, wizard: &SetupWizard) {
    let (title, subtitle, hint) = match wizard.step {
        Step::LocationName => (
            "City Name",
            "Where are you located? (used for display only)",
            "e.g.  Mumbai,  Karachi,  London",
        ),
        Step::Latitude => (
            "Latitude",
            "Your city's latitude — north/south position",
            "e.g.  19.0748  for Mumbai  ·  [Tab] to reset",
        ),
        Step::Longitude => (
            "Longitude",
            "Your city's longitude — east/west position",
            "e.g.  72.8856  for Mumbai  ·  [Tab] to reset",
        ),
        Step::TimezoneOffset => (
            "UTC Offset",
            "Your timezone offset from UTC",
            "e.g.  +5:30  for IST  ·  +3  for AST  ·  -5  for EST",
        ),
        _ => ("", "", ""),
    };

    let cursor = if wizard.input.len() < 40 { "█" } else { "" };

    let mut lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(title, theme::gold().add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled(subtitle, theme::dim())),
        Line::from(""),
        Line::from(""),
    ];

    // Input box
    let input_display = format!("  {}{}  ", wizard.input, cursor);
    let input_width = area.width.saturating_sub(8) as usize;
    let padded = format!("{:<width$}", input_display, width = input_width);

    let input_style = if wizard.error.is_some() {
        theme::red()
    } else {
        theme::amber()
    };

    lines.push(Line::from(Span::styled(padded, input_style.add_modifier(Modifier::BOLD))));
    lines.push(Line::from(""));

    if let Some(err) = &wizard.error {
        lines.push(Line::from(Span::styled(
            format!("  ✗  {}", err),
            theme::red(),
        )));
    } else {
        lines.push(Line::from(Span::styled(hint, theme::dim())));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter  confirm   ·   Esc  back",
        theme::dim(),
    )));

    let para = Paragraph::new(lines).alignment(Alignment::Center);
    let content_area = Rect {
        x: area.x,
        y: area.y + 2,
        width: area.width,
        height: area.height.saturating_sub(2),
    };
    frame.render_widget(para, content_area);
}

fn draw_method_list(frame: &mut Frame, area: Rect, wizard: &mut SetupWizard) {
    let header_lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Calculation Method",
            theme::gold().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Choose the authority for prayer time calculation",
            theme::dim(),
        )),
        Line::from(""),
    ];

    let header_para = Paragraph::new(header_lines).alignment(Alignment::Center);
    let header_area = Rect {
        x: area.x,
        y: area.y + 2,
        width: area.width,
        height: 5,
    };
    frame.render_widget(header_para, header_area);

    // Method list
    let list_area = Rect {
        x: area.x + 2,
        y: area.y + 8,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(12),
    };

    let items: Vec<ListItem> = CALC_METHODS
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let selected = i == wizard.method_idx;
            let line = if selected {
                Line::from(vec![
                    Span::styled("  ◉  ", theme::gold()),
                    Span::styled(*m, theme::gold().add_modifier(Modifier::BOLD)),
                ])
            } else {
                Line::from(vec![
                    Span::styled("  ○  ", theme::dim()),
                    Span::styled(*m, theme::dim()),
                ])
            };
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).style(theme::surface());
    frame.render_stateful_widget(list, list_area, &mut wizard.list_state);

    // Footer
    let footer = Paragraph::new(Line::from(Span::styled(
        "↑↓  navigate   ·   Enter  select   ·   Esc  back",
        theme::dim(),
    )))
    .alignment(Alignment::Center);
    let footer_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(3),
        width: area.width,
        height: 1,
    };
    frame.render_widget(footer, footer_area);
}

fn draw_choice(
    frame: &mut Frame,
    area: Rect,
    y_offset: u16,
    title: &str,
    subtitle: &str,
    options: &[&str],
    selected: usize,
    error: &Option<String>,
) {
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(title, theme::gold().add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled(subtitle, theme::dim())),
        Line::from(""),
        Line::from(""),
    ];

    for (i, opt) in options.iter().enumerate() {
        if i == selected {
            lines.push(Line::from(vec![
                Span::styled("  ◉  ", theme::gold()),
                Span::styled(*opt, theme::gold().add_modifier(Modifier::BOLD)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("  ○  ", theme::dim()),
                Span::styled(*opt, theme::dim()),
            ]));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(""));
    if let Some(err) = error {
        lines.push(Line::from(Span::styled(format!("  ✗  {}", err), theme::red())));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "←→  or  1 2  choose   ·   Enter  confirm   ·   Esc  back",
        theme::dim(),
    )));

    let para = Paragraph::new(lines).alignment(Alignment::Center);
    let content_area = Rect {
        x: area.x,
        y: area.y + 2,
        width: area.width,
        height: area.height.saturating_sub(2),
    };
    frame.render_widget(para, content_area);
}

fn draw_confirm(frame: &mut Frame, area: Rect, wizard: &SetupWizard) {
    let madhab = if wizard.madhab_idx == 0 { "Hanafi" } else { "Shafi" };
    let hijri = if wizard.hijri_idx == 0 {
        "Astronomical (default)"
    } else {
        "Local moon sighting (−1 day)"
    };

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("Confirm Settings", theme::gold().add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled("Review your configuration:", theme::dim())),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Location    ", theme::dim()),
            Span::styled(&wizard.location_name, theme::bold()),
        ]),
        Line::from(vec![
            Span::styled("  Coordinates ", theme::dim()),
            Span::styled(
                format!("{:.4},  {:.4}", wizard.latitude, wizard.longitude),
                theme::bold(),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Method      ", theme::dim()),
            Span::styled(CALC_METHODS[wizard.method_idx], theme::bold()),
        ]),
        Line::from(vec![
            Span::styled("  Madhab      ", theme::dim()),
            Span::styled(madhab, theme::bold()),
        ]),
        Line::from(vec![
            Span::styled("  UTC Offset  ", theme::dim()),
            Span::styled(format_tz(wizard.tz_minutes), theme::bold()),
        ]),
        Line::from(vec![
            Span::styled("  Hijri Date  ", theme::dim()),
            Span::styled(hijri, theme::bold()),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "Enter  save & cache 90 days   ·   Esc  go back",
            theme::dim(),
        )),
    ];

    let para = Paragraph::new(lines).alignment(Alignment::Center);
    let content_area = Rect {
        x: area.x,
        y: area.y + 2,
        width: area.width,
        height: area.height.saturating_sub(2),
    };
    frame.render_widget(para, content_area);
}

fn draw_caching(frame: &mut Frame) {
    let area = frame.area();
    frame.render_widget(Block::default().style(theme::base()), area);

    let lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "Calculating prayer times…",
            theme::gold().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Caching 90 days of prayer times offline.",
            theme::dim(),
        )),
        Line::from(Span::styled(
            "This only runs once per setup.",
            theme::dim(),
        )),
    ];

    let para = Paragraph::new(lines).alignment(Alignment::Center);
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(40), Constraint::Min(0)])
        .split(area);
    frame.render_widget(para, vchunks[1]);
}

// ─── Public entry point ──────────────────────────────────────────────────────

pub fn run_setup_tui(conn: &Connection, config: &mut AppConfig) -> Result<()> {
    let mut wizard = SetupWizard::new(config);
    let mut terminal = ratatui::init();
    let events = EventHandler::new(100);

    loop {
        terminal.draw(|frame| draw(frame, &mut wizard))?;

        match events.next()? {
            Event::Key(key) => {
                wizard.handle_key(key);
                if wizard.should_quit {
                    break;
                }
                if wizard.confirmed {
                    // Show caching screen
                    terminal.draw(|frame| draw_caching(frame))?;

                    // Build and save config
                    let new_config = wizard.build_config(config);
                    *config = new_config;
                    config.save()?;

                    // Clear stale cache and recompute
                    CacheRepo::clear_all(conn)?;
                    let calc = PrayerCalculator::new(
                        config.salah.latitude,
                        config.salah.longitude,
                        &config.salah.calc_method,
                        &config.salah.madhab,
                        config.salah.timezone_offset,
                    )?;
                    calc.ensure_cached(conn, 90)?;

                    // Mark setup done
                    use crate::db::repository::MetaRepo;
                    MetaRepo::set(conn, "setup_done", "1")?;

                    break;
                }
            }
            Event::Tick => {}
        }
    }

    ratatui::restore();
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn format_tz(minutes: i32) -> String {
    let sign = if minutes < 0 { "-" } else { "+" };
    let abs = minutes.abs();
    let h = abs / 60;
    let m = abs % 60;
    if m == 0 {
        format!("{}{}", sign, h)
    } else {
        format!("{}{}:{:02}", sign, h, m)
    }
}

fn parse_tz(s: &str) -> Result<i32> {
    let s = s.trim().trim_start_matches('+');
    let negative = s.starts_with('-');
    let s = s.trim_start_matches('-');
    let sign = if negative { -1 } else { 1 };

    let minutes = if s.contains(':') {
        let mut parts = s.splitn(2, ':');
        let h: i32 = parts.next().unwrap_or("0").parse()?;
        let m: i32 = parts.next().unwrap_or("0").parse()?;
        h * 60 + m
    } else if s.contains('.') {
        let h: f64 = s.parse()?;
        (h * 60.0).round() as i32
    } else {
        let h: i32 = s.parse()?;
        h * 60
    };

    Ok(sign * minutes)
}

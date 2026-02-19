use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::models::{DailyStats, Streak};
use crate::tui::theme;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    streak: &Streak,
    weekly: &[DailyStats],
) {
    let block = Block::default()
        .title(Span::styled(" Streak ", theme::gold()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(crate::tui::theme::BORDER))
        .style(theme::surface());

    // Weekly dots
    let mut dot_spans = vec![Span::styled("  ", theme::dim())];

    // Map daily stats to dots
    for i in 0..7 {
        let (dot, style) = if i < weekly.len() {
            let d = &weekly[i];
            match d.prayers_done {
                5 => ("●", theme::green().add_modifier(Modifier::BOLD)),
                3 | 4 => ("●", theme::amber()),
                1 | 2 => ("◑", theme::amber()),
                _ => ("○", theme::dim()),
            }
        } else {
            ("·", theme::dim())
        };
        dot_spans.push(Span::styled(dot, style));
        dot_spans.push(Span::styled("  ", theme::dim()));
    }

    let _dots_line = Line::from(dot_spans);

    // Streak bar (12 chars wide, filled proportional to streak/30)
    let bar_len = 12usize;
    let ratio = (streak.current as f64 / 30.0).min(1.0);
    let filled = (ratio * bar_len as f64).round() as usize;
    let empty = bar_len.saturating_sub(filled);
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

    let completed_this_week = weekly.iter().filter(|d| d.prayers_done >= 5).count();

    let streak_line = Line::from(vec![
        Span::styled("  ", theme::dim()),
        Span::styled(bar, theme::green()),
        Span::styled(
            format!("  {} days", streak.current),
            theme::green().add_modifier(Modifier::BOLD),
        ),
    ]);

    let meta_line = Line::from(vec![
        Span::styled(
            format!("  Best: {}  ·  Week: {}/7", streak.best, completed_this_week),
            theme::dim(),
        ),
    ]);

    let text = vec![Line::from(""), streak_line, Line::from(""), meta_line];
    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

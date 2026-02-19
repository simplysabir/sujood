use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::tui::theme;
use crate::utils::format::format_pages;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    today_pages: f64,
    weekly_pages: f64,
    daily_target: f64,
) {
    let block = Block::default()
        .title(Span::styled(" Quran ", theme::gold()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(crate::tui::theme::BORDER))
        .style(theme::surface());

    let inner_width = area.width.saturating_sub(4) as usize;
    let bar_width = inner_width.min(24);

    let ratio = if daily_target > 0.0 {
        (today_pages / daily_target).min(1.0)
    } else {
        0.0
    };
    let filled = (ratio * bar_width as f64).round() as usize;
    let empty = bar_width.saturating_sub(filled);

    let bar = format!("{}{}", "▓".repeat(filled), "░".repeat(empty));

    let progress_style = if today_pages >= daily_target {
        theme::green()
    } else {
        theme::amber()
    };

    let line = Line::from(vec![
        Span::styled("  ", theme::dim()),
        Span::styled(bar, progress_style),
        Span::styled(
            format!(
                "  {} / {} pages  ·  Week: {}",
                format_pages(today_pages),
                format_pages(daily_target),
                format_pages(weekly_pages)
            ),
            theme::dim(),
        ),
    ]);

    let paragraph = Paragraph::new(vec![Line::from(""), line]).block(block);
    frame.render_widget(paragraph, area);
}

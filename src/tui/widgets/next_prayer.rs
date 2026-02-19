use ratatui::{
    layout::{Alignment, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::models::PrayerType;
use crate::tui::theme;
use crate::utils::format::format_duration_secs;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    next_prayer: Option<&(PrayerType, i64)>,
) {
    let block = Block::default()
        .title(Span::styled(" Next Prayer ", theme::gold()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(crate::tui::theme::BORDER))
        .style(theme::surface());

    let content: Vec<Line> = match next_prayer {
        None => vec![
            Line::from(""),
            Line::from(Span::styled("  No data", theme::dim())),
        ],
        Some((prayer, secs)) => {
            let name = prayer.display_name().to_uppercase();
            let duration = format_duration_secs(*secs);
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("  {}", name),
                    theme::gold().add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  in  ", theme::dim()),
                    Span::styled(duration, theme::amber().add_modifier(Modifier::BOLD)),
                ]),
            ]
        }
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .alignment(Alignment::Left);

    frame.render_widget(paragraph, area);
}

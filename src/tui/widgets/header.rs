use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use crate::tui::theme;

pub fn render(frame: &mut Frame, area: Rect, hijri_str: &str, gregorian_str: &str) {
    let title_line = Line::from(vec![
        Span::styled("  سُجُود  ", theme::gold().add_modifier(Modifier::BOLD)),
        Span::styled("sujood", theme::gold()),
    ]);

    let date_line = Line::from(vec![
        Span::styled(hijri_str, theme::amber()),
        Span::styled("  ·  ", theme::dim()),
        Span::styled(gregorian_str, theme::dim()),
    ]);

    let text = vec![title_line, Line::from(""), date_line];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::gold().add_modifier(Modifier::BOLD))
        .style(theme::base());

    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Center);

    frame.render_widget(paragraph, area);
}

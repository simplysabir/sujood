use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::tui::theme;

pub fn render(frame: &mut Frame, area: Rect, qada_count: i64) {
    let block = Block::default()
        .title(Span::styled(" Qada ", theme::gold()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(crate::tui::theme::BORDER))
        .style(theme::surface());

    let content = if qada_count == 0 {
        vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  ", theme::dim()),
                Span::styled("0 prayers owed  ✓", theme::green()),
            ]),
        ]
    } else {
        vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  ", theme::dim()),
                Span::styled(
                    format!("{} prayers owed", qada_count),
                    theme::amber().add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                format!("  ~{} days to clear", qada_count),
                theme::dim(),
            )),
        ]
    };

    let paragraph = Paragraph::new(content).block(block);
    frame.render_widget(paragraph, area);
}

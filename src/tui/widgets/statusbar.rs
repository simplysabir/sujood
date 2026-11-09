use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::tui::theme;

pub fn render(frame: &mut Frame, area: Rect, message: Option<&str>, is_error: bool) {
    if let Some(message) = message {
        let style = if is_error {
            theme::red()
        } else {
            theme::green()
        };
        let paragraph =
            Paragraph::new(Line::from(Span::styled(message, style))).alignment(Alignment::Center);
        frame.render_widget(paragraph, area);
        return;
    }

    let hints = vec![
        ("[m]", " mark  "),
        ("[M]", " missed  "),
        ("[q]", " qada  "),
        ("[d]", " dhikr  "),
        ("[r]", " quran  "),
        ("[s]", " stats  "),
        ("[?]", " help  "),
        ("[Esc]", " quit"),
    ];

    let mut spans = Vec::new();
    for (key, label) in &hints {
        spans.push(Span::styled(*key, theme::gold()));
        spans.push(Span::styled(*label, theme::dim()));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}

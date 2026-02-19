use ratatui::{
    layout::{Alignment, Rect},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::tui::theme;

pub fn render(frame: &mut Frame, area: Rect) {
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

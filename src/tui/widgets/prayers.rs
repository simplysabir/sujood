use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem},
    Frame,
};

use crate::models::{Prayer, PrayerStatus};
use crate::tui::theme;

pub fn render(frame: &mut Frame, area: Rect, prayers: &[Prayer], focused_idx: usize, focused: bool) {
    let block = Block::default()
        .title(Span::styled(
            " Prayers ",
            theme::gold(),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            theme::gold()
        } else {
            ratatui::style::Style::default().fg(crate::tui::theme::BORDER)
        })
        .style(theme::surface());

    let items: Vec<ListItem> = prayers
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_focused = focused && i == focused_idx;

            let time_str = p
                .time
                .map(|t| t.format("%H:%M").to_string())
                .unwrap_or_else(|| "--:--".to_string());

            let (icon, status_style) = match p.status {
                PrayerStatus::Done => ("●", theme::green()),
                PrayerStatus::Missed => ("✗", theme::red()),
                PrayerStatus::Pending => ("○", theme::dim()),
            };

            let status_label = match p.status {
                PrayerStatus::Done => "done",
                PrayerStatus::Missed => "missed",
                PrayerStatus::Pending => "upcoming",
            };

            let name_style = if is_focused {
                theme::gold().add_modifier(Modifier::BOLD)
            } else {
                theme::bold()
            };

            let line = Line::from(vec![
                Span::styled(format!("  {:<8}", p.prayer_type.display_name()), name_style),
                Span::styled(format!("{:<7}", time_str), theme::dim()),
                Span::styled(icon, status_style),
                Span::styled(format!("  {}", status_label), theme::dim()),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

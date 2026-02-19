use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem},
    Frame,
};

use crate::models::{DhikrDef, DhikrLog, DhikrType};
use crate::tui::theme;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    defs: &[DhikrDef],
    logs: &std::collections::HashMap<i64, DhikrLog>,
    focus_idx: usize,
    focused: bool,
) {
    let block = Block::default()
        .title(Span::styled(" Adhkar ", theme::gold()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            theme::gold()
        } else {
            ratatui::style::Style::default().fg(crate::tui::theme::BORDER)
        })
        .style(theme::surface());

    let items: Vec<ListItem> = defs
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let log = logs.get(&def.id);
            let is_focused = focused && i == focus_idx;

            let name_style = if is_focused {
                theme::gold().add_modifier(Modifier::BOLD)
            } else {
                theme::bold()
            };

            let status_span = match &def.dhikr_type {
                DhikrType::Checkbox => {
                    let done = log.map(|l| l.completed).unwrap_or(false);
                    if done {
                        Span::styled("●", theme::green())
                    } else {
                        Span::styled("○", theme::dim())
                    }
                }
                DhikrType::Counter => {
                    let count = log.map(|l| l.count).unwrap_or(0);
                    let target = def.target_count;
                    let done = count >= target;

                    // Build a small progress bar (6 chars wide)
                    let ratio = (count as f64 / target as f64).min(1.0);
                    let filled = (ratio * 5.0).round() as usize;
                    let empty = 5usize.saturating_sub(filled);
                    let bar = format!("{}{}", "▓".repeat(filled), "░".repeat(empty));

                    let color = if done { theme::green() } else { theme::amber() };
                    let text = format!("{} {}/{}", bar, count, target);
                    Span::styled(text, color)
                }
            };

            let line = Line::from(vec![
                Span::styled(format!("  {:<28}", def.name), name_style),
                status_span,
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

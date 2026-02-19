use ratatui::style::{Color, Modifier, Style};

pub const BG: Color = Color::Rgb(18, 16, 14);
pub const SURFACE: Color = Color::Rgb(28, 25, 20);
pub const BORDER: Color = Color::Rgb(55, 48, 36);
pub const BORDER_FOCUS: Color = Color::Rgb(196, 160, 68);
pub const TEXT: Color = Color::Rgb(230, 218, 196);
pub const TEXT_DIM: Color = Color::Rgb(130, 118, 96);
pub const GOLD: Color = Color::Rgb(196, 160, 68);
pub const GREEN: Color = Color::Rgb(92, 148, 92);
pub const AMBER: Color = Color::Rgb(210, 138, 60);
pub const RED: Color = Color::Rgb(180, 82, 62);
pub const FILL: Color = Color::Rgb(70, 62, 48);
pub const EMPTY: Color = Color::Rgb(38, 34, 26);

pub fn base() -> Style {
    Style::default().fg(TEXT).bg(BG)
}

pub fn dim() -> Style {
    Style::default().fg(TEXT_DIM)
}

pub fn gold() -> Style {
    Style::default().fg(GOLD)
}

pub fn green() -> Style {
    Style::default().fg(GREEN)
}

pub fn amber() -> Style {
    Style::default().fg(AMBER)
}

pub fn red() -> Style {
    Style::default().fg(RED)
}

pub fn bold() -> Style {
    Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
}

pub fn surface() -> Style {
    Style::default().fg(TEXT).bg(SURFACE)
}

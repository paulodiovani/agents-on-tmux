use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;

/// Visual theme configuration for the TUI.
pub struct Theme {
    pub header_style: Style,
    pub footer_style: Style,
    pub footer_key_style: Style,
    pub card_border: Style,
    pub card_border_selected: Style,
    pub card_border_notification: Style,
    pub selected_border_set: symbols::border::Set,
    pub card_title: Style,
    pub card_detail: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            header_style: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            footer_style: Style::default().fg(Color::DarkGray),
            footer_key_style: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            card_border: Style::default().fg(Color::DarkGray),
            card_border_selected: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            card_border_notification: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            selected_border_set: symbols::border::DOUBLE,
            card_title: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            card_detail: Style::default().fg(Color::DarkGray),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_theme() {
        let theme = Theme::default();
        assert_eq!(theme.header_style.fg, Some(Color::White));
        assert_eq!(theme.footer_style.fg, Some(Color::DarkGray));
        assert_eq!(theme.footer_key_style.fg, Some(Color::White));
        assert_eq!(theme.card_border.fg, Some(Color::DarkGray));
        assert_eq!(theme.card_border_selected.fg, Some(Color::White));
        assert_eq!(theme.card_border_notification.fg, Some(Color::White));
        assert_eq!(theme.selected_border_set, symbols::border::DOUBLE);
        assert_eq!(theme.card_title.fg, Some(Color::White));
        assert_eq!(theme.card_detail.fg, Some(Color::DarkGray));
    }
}

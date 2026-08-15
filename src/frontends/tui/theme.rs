use ratatui::style::{Color, Modifier, Style};

/// Accent slot: the active-window marker and the tab underline.
const ACCENT: Color = Color::Blue;
/// Selection background: the terminal's own bright black, which every theme
/// renders as a shade of its background.
const SELECTION: Color = Color::DarkGray;

/// Visual theme for the TUI.
///
/// Every style is expressed with the terminal's own palette (named ANSI colors)
/// or plain attributes, never absolute colors, so the UI recolors itself when the
/// user changes the terminal theme. Three brightness tiers carry the hierarchy:
/// `title` (bold) > `normal` (default foreground) > `dim`.
pub struct Theme {
    pub accent: Style,
    pub dim: Style,
    pub footer_key: Style,
    pub header: Style,
    pub normal: Style,
    pub notification: Style,
    pub selection: Style,
    pub selection_pad: Style,
    pub tab_active: Style,
    pub title: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            accent: Style::default().fg(ACCENT),
            dim: Style::default().add_modifier(Modifier::DIM),
            footer_key: Style::default().add_modifier(Modifier::BOLD),
            header: Style::default().add_modifier(Modifier::BOLD),
            normal: Style::default(),
            notification: Style::default().fg(Color::Yellow),
            selection: Style::default().bg(SELECTION),
            // The padding half blocks are drawn on rows the selection does not
            // reach, so they carry its color as their foreground, over the
            // terminal's own background, and clear whatever was on the row.
            selection_pad: Style::default()
                .fg(SELECTION)
                .bg(Color::Reset)
                .remove_modifier(Modifier::all()),
            tab_active: Style::default().add_modifier(Modifier::BOLD),
            title: Style::default().add_modifier(Modifier::BOLD),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(theme: &Theme) -> Vec<Style> {
        vec![
            theme.accent,
            theme.dim,
            theme.footer_key,
            theme.header,
            theme.normal,
            theme.notification,
            theme.selection,
            theme.selection_pad,
            theme.tab_active,
            theme.title,
        ]
    }

    #[test]
    fn test_theme_tiers() {
        let theme = Theme::default();
        assert!(theme.title.add_modifier.contains(Modifier::BOLD));
        assert_eq!(theme.title.fg, None);
        assert_eq!(theme.normal, Style::default());
        assert!(theme.dim.add_modifier.contains(Modifier::DIM));
        assert_eq!(theme.dim.fg, None);
        assert!(theme.header.add_modifier.contains(Modifier::BOLD));
        assert!(theme.footer_key.add_modifier.contains(Modifier::BOLD));
        assert!(theme.tab_active.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_accent_is_blue() {
        assert_eq!(Theme::default().accent.fg, Some(Color::Blue));
    }

    #[test]
    fn test_notification_is_yellow() {
        assert_eq!(Theme::default().notification.fg, Some(Color::Yellow));
    }

    #[test]
    fn test_selection_is_a_bright_black_background() {
        let theme = Theme::default();
        assert_eq!(theme.selection.bg, Some(Color::DarkGray));
        assert!(!theme.selection.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn test_selection_padding_matches_the_selection() {
        let theme = Theme::default();
        assert_eq!(theme.selection_pad.fg, theme.selection.bg);
        assert_eq!(theme.selection_pad.bg, Some(Color::Reset));
        // The padding rows are drawn over rows the selection may have styled.
        assert!(theme.selection_pad.sub_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn test_theme_only_uses_terminal_palette_colors() {
        for style in styles(&Theme::default()) {
            for color in [style.fg, style.bg].into_iter().flatten() {
                assert!(
                    !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
                    "absolute color {color:?} would ignore the terminal theme"
                );
            }
        }
    }
}

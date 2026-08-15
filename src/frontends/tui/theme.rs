use ratatui::style::{Color, Modifier, Style};

use crate::backends::config::{AnsiColor, Config, Selection};

/// Accent slot used when the config does not name one.
const DEFAULT_ACCENT: Color = Color::Blue;
/// Selection background used when the config does not name one: the terminal's
/// own bright black, which every theme renders as a shade of its background.
const DEFAULT_SELECTION: Selection = Selection::Background(AnsiColor::DarkGray);

/// Maps a configured color name onto the matching terminal palette slot.
/// Only named (0-15) colors are ever produced: no Rgb, no Indexed.
fn to_color(color: AnsiColor) -> Color {
    match color {
        AnsiColor::Black => Color::Black,
        AnsiColor::Blue => Color::Blue,
        AnsiColor::Cyan => Color::Cyan,
        AnsiColor::DarkGray => Color::DarkGray,
        AnsiColor::Gray => Color::Gray,
        AnsiColor::Green => Color::Green,
        AnsiColor::LightBlue => Color::LightBlue,
        AnsiColor::LightCyan => Color::LightCyan,
        AnsiColor::LightGreen => Color::LightGreen,
        AnsiColor::LightMagenta => Color::LightMagenta,
        AnsiColor::LightRed => Color::LightRed,
        AnsiColor::LightYellow => Color::LightYellow,
        AnsiColor::Magenta => Color::Magenta,
        AnsiColor::Red => Color::Red,
        AnsiColor::White => Color::White,
        AnsiColor::Yellow => Color::Yellow,
    }
}

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
        Self::from_config(&Config::default())
    }
}

impl Theme {
    /// Builds the theme, honoring the two color options a user may set.
    pub fn from_config(config: &Config) -> Self {
        let accent = config.accent_color.map(to_color).unwrap_or(DEFAULT_ACCENT);

        // The padding half blocks are drawn on rows the selection does not reach,
        // so they carry the selection color as their foreground, over the
        // terminal's own background.
        let pad = Style::default()
            .bg(Color::Reset)
            .remove_modifier(Modifier::all());

        // Under REVERSED the selected block's background is the terminal's default
        // foreground, which is exactly what Color::Reset paints.
        let (selection, selection_pad) = match config.selection_bg.unwrap_or(DEFAULT_SELECTION) {
            Selection::Background(color) => {
                let bg = to_color(color);
                (Style::default().bg(bg), pad.fg(bg))
            }
            Selection::Reversed => (
                Style::default().add_modifier(Modifier::REVERSED),
                pad.fg(Color::Reset),
            ),
        };

        Self {
            accent: Style::default().fg(accent),
            dim: Style::default().add_modifier(Modifier::DIM),
            footer_key: Style::default().add_modifier(Modifier::BOLD),
            header: Style::default().add_modifier(Modifier::BOLD),
            normal: Style::default(),
            notification: Style::default().fg(Color::Yellow),
            selection,
            selection_pad,
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
    fn test_default_theme_tiers() {
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
    fn test_default_accent_is_blue() {
        assert_eq!(Theme::default().accent.fg, Some(Color::Blue));
    }

    #[test]
    fn test_notification_is_yellow() {
        assert_eq!(Theme::default().notification.fg, Some(Color::Yellow));
    }

    #[test]
    fn test_default_selection_is_a_bright_black_background() {
        let theme = Theme::default();
        assert_eq!(theme.selection.bg, Some(Color::DarkGray));
        assert!(!theme.selection.add_modifier.contains(Modifier::REVERSED));
        // The padding half blocks paint the selection color over the terminal's
        // own background.
        assert_eq!(theme.selection_pad.fg, Some(Color::DarkGray));
        assert_eq!(theme.selection_pad.bg, Some(Color::Reset));
    }

    #[test]
    fn test_reversed_selection_fallback() {
        let config = Config {
            selection_bg: Some(Selection::Reversed),
            ..Config::default()
        };
        let theme = Theme::from_config(&config);
        assert!(theme.selection.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(theme.selection.bg, None);
        // Reversed shows the terminal's default foreground as its background.
        assert_eq!(theme.selection_pad.fg, Some(Color::Reset));
        assert_eq!(theme.selection_pad.bg, Some(Color::Reset));
        assert!(
            theme
                .selection_pad
                .sub_modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn test_configured_accent_color() {
        let config = Config {
            accent_color: Some(AnsiColor::Magenta),
            ..Config::default()
        };
        assert_eq!(Theme::from_config(&config).accent.fg, Some(Color::Magenta));
    }

    #[test]
    fn test_configured_selection_bg_matches_padding() {
        let config = Config {
            selection_bg: Some(Selection::Background(AnsiColor::Blue)),
            ..Config::default()
        };
        let theme = Theme::from_config(&config);
        assert_eq!(theme.selection.bg, Some(Color::Blue));
        assert!(!theme.selection.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(theme.selection_pad.fg, Some(Color::Blue));
        assert_eq!(theme.selection_pad.bg, Some(Color::Reset));
    }

    #[test]
    fn test_theme_only_uses_terminal_palette_colors() {
        let config = Config {
            accent_color: Some(AnsiColor::LightCyan),
            selection_bg: Some(Selection::Background(AnsiColor::Black)),
            ..Config::default()
        };
        for theme in [Theme::default(), Theme::from_config(&config)] {
            for style in styles(&theme) {
                for color in [style.fg, style.bg].into_iter().flatten() {
                    assert!(
                        !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
                        "absolute color {color:?} would ignore the terminal theme"
                    );
                }
            }
        }
    }

    #[test]
    fn test_to_color_maps_every_variant_to_a_named_color() {
        for color in [
            AnsiColor::Black,
            AnsiColor::Blue,
            AnsiColor::Cyan,
            AnsiColor::DarkGray,
            AnsiColor::Gray,
            AnsiColor::Green,
            AnsiColor::LightBlue,
            AnsiColor::LightCyan,
            AnsiColor::LightGreen,
            AnsiColor::LightMagenta,
            AnsiColor::LightRed,
            AnsiColor::LightYellow,
            AnsiColor::Magenta,
            AnsiColor::Red,
            AnsiColor::White,
            AnsiColor::Yellow,
        ] {
            assert!(!matches!(
                to_color(color),
                Color::Rgb(..) | Color::Indexed(_) | Color::Reset
            ));
        }
    }
}

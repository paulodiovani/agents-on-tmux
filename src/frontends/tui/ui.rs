use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Styled;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::backends::{SESSION_NAME, Window};
use crate::frontends::tui::app::App;
use crate::frontends::tui::theme::Theme;

/// Renders the complete TUI layout: header, cards, and footer.
pub fn draw(frame: &mut Frame, app: &mut App, theme: &Theme) {
    let footer_height = calculate_footer_height(frame.area().width);
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(footer_height),
    ])
    .split(frame.area());

    draw_header(frame, chunks[0], theme);
    draw_cards(frame, app, chunks[1], theme);
    draw_footer(frame, app, chunks[2], theme);
}

/// Renders the header bar with the session name.
fn draw_header(frame: &mut Frame, area: ratatui::layout::Rect, theme: &Theme) {
    let header = Paragraph::new(Span::styled(SESSION_NAME, theme.header_style));
    frame.render_widget(header, area);
}

/// Renders the window cards in the main content area.
fn draw_cards(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect, theme: &Theme) {
    let windows: Vec<Window> = app.windows().to_vec();
    if windows.is_empty() {
        let empty = Paragraph::new("No windows").set_style(theme.card_detail);
        frame.render_widget(empty, area);
        return;
    }

    let card_height = 4u16;
    let visible_count = (area.height / card_height) as usize;

    app.ensure_visible(visible_count);

    let offset = app.list_state().offset();
    let visible_windows: Vec<(usize, &Window)> = windows
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_count)
        .collect();

    let constraints: Vec<Constraint> = visible_windows
        .iter()
        .map(|_| Constraint::Length(card_height))
        .collect();
    let card_areas = Layout::vertical(constraints).split(area);

    for (idx, (i, window)) in visible_windows.iter().enumerate() {
        let is_selected = *i == app.selected();
        let is_notification = window.notification_pending;

        let (border_style, border_set) = match (is_selected, is_notification) {
            (true, true) => (
                theme
                    .card_border_selected
                    .patch(theme.card_border_notification),
                theme.selected_border_set,
            ),
            (true, false) => (theme.card_border_selected, theme.selected_border_set),
            (false, true) => (
                theme.card_border_notification,
                ratatui::symbols::border::PLAIN,
            ),
            (false, false) => (theme.card_border, ratatui::symbols::border::PLAIN),
        };

        let block = Block::bordered()
            .border_style(border_style)
            .border_set(border_set);
        let inner = block.inner(card_areas[idx]);
        frame.render_widget(block, card_areas[idx]);

        let title = if is_notification {
            let name_width = window.name.chars().count();
            let inner_width = inner.width as usize;
            let padding = inner_width.saturating_sub(name_width + 1);
            Line::from(vec![
                Span::styled(&window.name, theme.card_title),
                Span::raw(" ".repeat(padding)),
                Span::styled("!", theme.card_title),
            ])
        } else {
            Line::from(Span::styled(&window.name, theme.card_title))
        };
        let time_str = format_elapsed(window.started_at);
        let dirname = std::path::Path::new(&window.current_dir)
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| format!("../{}", s))
            .unwrap_or_else(|| "n/a".to_string());

        let mut parts = vec![dirname, window.running_command.clone()];
        if !time_str.is_empty() {
            parts.push(time_str);
        }

        let detail_text = parts.join(" · ");
        let display_text = truncate_left(&detail_text, inner.width as usize);
        let detail = Line::from(Span::styled(display_text, theme.card_detail));

        let content = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);
        frame.render_widget(Paragraph::new(title), content[0]);
        frame.render_widget(Paragraph::new(detail), content[1]);
    }
}

/// Renders the footer with keybinding hints or confirmation message.
fn draw_footer(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect, theme: &Theme) {
    let footer = if app.pending_kill() {
        let msg = Line::from(vec![
            Span::styled("d", theme.footer_key_style),
            Span::styled(" kill this window", theme.footer_style),
        ]);
        Paragraph::new(msg)
    } else {
        let entries = build_footer_entries(theme);
        let lines = wrap_entries(&entries, area.width as usize);
        Paragraph::new(lines)
    };
    frame.render_widget(footer, area);
}

/// Builds styled footer keybinding entries.
fn build_footer_entries(theme: &Theme) -> Vec<(Vec<Span<'static>>, usize)> {
    let keys = [
        ("↑↓", "navigate"),
        ("⏎", "focus"),
        ("n", "new"),
        ("d", "kill"),
        ("q", "quit"),
    ];

    keys.iter()
        .map(|(key, desc)| {
            let spans = vec![
                Span::styled(key.to_string(), theme.footer_key_style),
                Span::styled(format!(" {}", desc), theme.footer_style),
            ];
            let width = key.chars().count() + 1 + desc.chars().count();
            (spans, width)
        })
        .collect()
}

/// Wraps footer entries into lines that fit the available width.
fn wrap_entries(entries: &[(Vec<Span<'static>>, usize)], width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width: usize = 0;

    for (spans, entry_width) in entries.iter() {
        let separator_width = if current_width > 0 { 2 } else { 0 };
        let needed = current_width + separator_width + entry_width;

        if needed > width && current_width > 0 {
            lines.push(Line::from(current_spans));
            current_spans = Vec::new();
            current_width = 0;
        }

        if !current_spans.is_empty() {
            current_spans.push(Span::raw("  "));
            current_width += 2;
        }

        current_spans.extend(spans.iter().cloned());
        current_width += entry_width;
    }

    if !current_spans.is_empty() {
        lines.push(Line::from(current_spans));
    }

    if lines.is_empty() {
        lines.push(Line::from(Vec::<Span<'static>>::new()));
    }

    lines
}

/// Formats an elapsed duration as a human-readable string.
fn format_elapsed(started_at: Option<Instant>) -> String {
    match started_at {
        Some(start) => {
            let duration = Instant::now().duration_since(start);
            let total_secs = duration.as_secs();
            let minutes = total_secs / 60;
            let seconds = total_secs % 60;
            if minutes > 0 {
                format!("{}m {}s", minutes, seconds)
            } else {
                format!("{}s", seconds)
            }
        }
        None => String::new(),
    }
}

/// Truncates text from the left, prepending ".." if needed.
fn truncate_left(text: &str, max_width: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_width {
        text.to_string()
    } else if max_width <= 2 {
        "..".to_string()
    } else {
        let take = max_width - 2;
        let truncated: String = text.chars().skip(char_count - take).collect();
        format!("..{}", truncated)
    }
}

/// Calculates how many lines the footer needs for the given width.
fn calculate_footer_height(width: u16) -> u16 {
    let keys = [
        ("↑↓", "navigate"),
        ("⏎", "focus"),
        ("n", "new"),
        ("d", "kill"),
        ("q", "quit"),
    ];

    let widths: Vec<usize> = keys
        .iter()
        .map(|(key, desc)| key.chars().count() + 1 + desc.chars().count())
        .collect();

    let mut lines = 1;
    let mut current_width = 0;

    for (i, &entry_width) in widths.iter().enumerate() {
        let separator_width = if current_width > 0 { 2 } else { 0 };
        let needed = current_width + separator_width + entry_width;

        if needed > width as usize && current_width > 0 {
            lines += 1;
            current_width = 0;
        }

        if current_width > 0 {
            current_width += 2;
        }
        current_width += entry_width;

        let _ = i;
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_format_elapsed_seconds_only() {
        let start = Instant::now() - Duration::from_secs(45);
        assert_eq!(format_elapsed(Some(start)), "45s");
    }

    #[test]
    fn test_format_elapsed_minutes_and_seconds() {
        let start = Instant::now() - Duration::from_secs(125);
        assert_eq!(format_elapsed(Some(start)), "2m 5s");
    }

    #[test]
    fn test_format_elapsed_none() {
        assert_eq!(format_elapsed(None), "");
    }

    #[test]
    fn test_format_elapsed_exact_minute() {
        let start = Instant::now() - Duration::from_secs(60);
        assert_eq!(format_elapsed(Some(start)), "1m 0s");
    }

    #[test]
    fn test_calculate_footer_height_wide() {
        assert_eq!(calculate_footer_height(120), 1);
    }

    #[test]
    fn test_calculate_footer_height_narrow() {
        assert_eq!(calculate_footer_height(30), 2);
    }

    #[test]
    fn test_truncate_left_fits() {
        assert_eq!(truncate_left("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_left_exact_fit() {
        assert_eq!(truncate_left("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_left_needs_truncation() {
        assert_eq!(truncate_left("/home/user/project", 10), "../project");
    }

    #[test]
    fn test_truncate_left_very_narrow() {
        assert_eq!(truncate_left("hello", 2), "..");
    }

    #[test]
    fn test_truncate_left_empty() {
        assert_eq!(truncate_left("", 5), "");
    }
}

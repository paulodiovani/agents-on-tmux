use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Styled;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::backends::SESSION_NAME;
use crate::frontends::tui::app::App;
use crate::frontends::tui::theme::Theme;

pub fn draw(frame: &mut Frame, app: &App, theme: &Theme) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .split(frame.area());

    draw_header(frame, chunks[0], theme);
    draw_cards(frame, app, chunks[1], theme);
    draw_footer(frame, chunks[2], theme);
}

fn draw_header(frame: &mut Frame, area: ratatui::layout::Rect, theme: &Theme) {
    let header = Paragraph::new(Span::styled(SESSION_NAME, theme.header_style));
    frame.render_widget(header, area);
}

fn draw_cards(frame: &mut Frame, app: &App, area: ratatui::layout::Rect, theme: &Theme) {
    let windows = app.windows();
    if windows.is_empty() {
        let empty = Paragraph::new("No windows").set_style(theme.card_detail);
        frame.render_widget(empty, area);
        return;
    }

    let constraints: Vec<Constraint> = windows.iter().map(|_| Constraint::Length(3)).collect();
    let card_areas = Layout::vertical(constraints).split(area);

    for (i, window) in windows.iter().enumerate() {
        let is_selected = i == app.selected();
        let border_style = if window.notification_pending {
            theme.card_border_notification
        } else if is_selected {
            theme.card_border_selected
        } else {
            theme.card_border
        };

        let block = Block::bordered().border_style(border_style);
        let inner = block.inner(card_areas[i]);
        frame.render_widget(block, card_areas[i]);

        let title = Line::from(Span::styled(&window.name, theme.card_title));
        let time_str = format_duration(window.running_time);
        let detail = if window.running_command.is_empty() {
            Line::from(Span::styled(time_str, theme.card_detail))
        } else {
            Line::from(vec![
                Span::styled(&window.running_command, theme.card_detail),
                Span::styled(" · ", theme.card_detail),
                Span::styled(time_str, theme.card_detail),
            ])
        };

        let content = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);
        frame.render_widget(Paragraph::new(title), content[0]);
        frame.render_widget(Paragraph::new(detail), content[1]);
    }
}

fn draw_footer(frame: &mut Frame, area: ratatui::layout::Rect, theme: &Theme) {
    let keys = [
        ("↑↓", "navigate"),
        ("⏎", "focus"),
        ("n", "new"),
        ("d", "kill"),
        ("q", "quit"),
    ];

    let spans: Vec<Span> = keys
        .iter()
        .enumerate()
        .flat_map(|(i, (key, desc))| {
            let mut items = vec![
                Span::styled(*key, theme.footer_key_style),
                Span::styled(format!(" {}", desc), theme.footer_style),
            ];
            if i < keys.len() - 1 {
                items.push(Span::styled("  ", theme.footer_style));
            }
            items
        })
        .collect();

    let footer = Paragraph::new(Line::from(spans));
    frame.render_widget(footer, area);
}

fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_seconds_only() {
        assert_eq!(format_duration(Duration::from_secs(45)), "45s");
    }

    #[test]
    fn test_format_duration_minutes_and_seconds() {
        assert_eq!(format_duration(Duration::from_secs(125)), "2m 5s");
    }

    #[test]
    fn test_format_duration_zero() {
        assert_eq!(format_duration(Duration::ZERO), "0s");
    }

    #[test]
    fn test_format_duration_exact_minute() {
        assert_eq!(format_duration(Duration::from_secs(60)), "1m 0s");
    }
}

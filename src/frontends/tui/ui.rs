use std::path::Path;
use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::backends::agents::{Agent, is_agent};
use crate::backends::tmux::{SESSION_NAME, Window};
use crate::frontends::tui::app::App;
use crate::frontends::tui::event::{PendingAction, Tab};
use crate::frontends::tui::icons::{agent_icon, notification_icon, window_icon};
use crate::frontends::tui::path::shorten_path;
use crate::frontends::tui::theme::Theme;

/// Rows one list item occupies: two content lines plus a blank separator.
const ROW_HEIGHT: u16 = 3;
/// Columns of breathing room kept on both sides, so text never touches the pane
/// border. Full-width decorations (the tab rule, the selection) ignore it.
const PANEL_PADDING: usize = 1;
/// Columns always reserved for the notification marker, so the elapsed times
/// never shift sideways when a notification appears or clears.
const NOTIFICATION_SLOT: usize = 2;
const ELLIPSIS: &str = "\u{2026}"; // …
const HALF_BLOCK_LOWER: &str = "\u{2584}"; // ▄
const HALF_BLOCK_UPPER: &str = "\u{2580}"; // ▀
const TAB_RULE: &str = "\u{2594}"; // ▔
const TAB_GAP: &str = "   ";

/// Shrinks an area horizontally by the panel padding.
fn padded(area: Rect) -> Rect {
    let padding = (PANEL_PADDING as u16).min(area.width / 2);
    Rect {
        x: area.x + padding,
        width: area.width - padding * 2,
        ..area
    }
}

/// Renders the complete TUI layout: header, tab bar, window rows, and footer.
pub fn draw(frame: &mut Frame, app: &mut App, theme: &Theme) {
    let footer_height = calculate_footer_height(padded(frame.area()).width, app, theme);
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(footer_height),
    ])
    .split(frame.area());

    draw_header(frame, chunks[0], theme);
    draw_tab_bar(frame, app, chunks[1], theme);
    draw_list(frame, app, chunks[2], theme);
    draw_footer(frame, app, chunks[3], theme);
}

/// Renders the header bar with the session name.
fn draw_header(frame: &mut Frame, area: Rect, theme: &Theme) {
    let header = Paragraph::new(Span::styled(SESSION_NAME, theme.header));
    frame.render_widget(header, padded(area));
}

/// Renders the two-line tab bar: labels with their counts, underlined by a rule
/// whose accent segment sits under the active label.
fn draw_tab_bar(frame: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    let mut labels: Vec<Span> = Vec::new();
    let mut cursor = 0usize;
    let mut accent_start = 0usize;
    let mut accent_width = 0usize;

    for (index, tab) in [Tab::Agents, Tab::Windows].iter().enumerate() {
        if index > 0 {
            labels.push(Span::raw(TAB_GAP));
            cursor += TAB_GAP.width();
        }

        let title = tab.title();
        let count = format!("({})", count_windows_for_tab(app, *tab));
        let is_active = app.active_tab() == *tab;
        let label_width = title.width() + 1 + count.width();
        if is_active {
            accent_start = cursor;
            accent_width = label_width;
        }

        let style = if is_active {
            theme.tab_active
        } else {
            theme.dim
        };
        labels.push(Span::styled(title, style));
        labels.push(Span::raw(" "));
        labels.push(Span::styled(count, style));
        cursor += label_width;
    }

    let width = area.width as usize;
    let lead = (accent_start + PANEL_PADDING).min(width);
    let accent = accent_width.min(width - lead);
    let rest = width - lead - accent;
    let rule = vec![
        Span::styled(TAB_RULE.repeat(lead), theme.dim),
        Span::styled(TAB_RULE.repeat(accent), theme.accent),
        Span::styled(TAB_RULE.repeat(rest), theme.dim),
    ];

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);
    frame.render_widget(Paragraph::new(Line::from(labels)), padded(rows[0]));
    frame.render_widget(Paragraph::new(Line::from(rule)), rows[1]);
}

/// Renders the windows of the active tab as two-line rows. The selection lives in
/// each row's spans, as `highlight_style` cannot drop the dim tier's DIM.
fn draw_list(frame: &mut Frame, app: &mut App, area: Rect, theme: &Theme) {
    let list_area = area;

    if app.current_tab_len() == 0 {
        let empty = Paragraph::new(Span::styled("No windows", theme.dim));
        frame.render_widget(empty, padded(list_area));
        return;
    }

    let visible_count = (list_area.height / ROW_HEIGHT) as usize;
    app.ensure_visible(visible_count);

    let offset = app.list_state().offset();
    let selected = app.current_selected();
    let active_tab = app.active_tab();
    let width = list_area.width as usize;

    let items: Vec<ListItem> = app
        .current_tab_windows()
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_count)
        .map(|(index, window)| {
            let styles = RowStyles::new(theme, index == selected);
            window_item(window, active_tab, &styles, width)
        })
        .collect();

    let selected_row = selected
        .checked_sub(offset)
        .filter(|row| *row < items.len());

    frame.render_widget(List::new(items), list_area);

    if let Some(row) = selected_row {
        let first_line = list_area.y + row as u16 * ROW_HEIGHT;
        draw_selection_padding(frame, area, first_line, theme);
    }
}

/// Pads the selected block with half a cell above and below. The first item has no
/// blank row above it, so the tab rule takes the selection instead.
fn draw_selection_padding(frame: &mut Frame, bounds: Rect, first_line: u16, theme: &Theme) {
    let width = bounds.width as usize;
    let bottom = bounds.y + bounds.height;

    if first_line > bounds.y {
        let pad = Paragraph::new(Span::styled(
            HALF_BLOCK_LOWER.repeat(width),
            theme.selection_pad,
        ));
        frame.render_widget(
            pad,
            Rect {
                y: first_line - 1,
                height: 1,
                ..bounds
            },
        );
    } else if first_line > 0 {
        let rule = Rect {
            y: first_line - 1,
            height: 1,
            ..bounds
        };
        frame.buffer_mut().set_style(rule, theme.selection);
    }

    let below = first_line + 2;
    if below < bottom {
        let pad = Paragraph::new(Span::styled(
            HALF_BLOCK_UPPER.repeat(width),
            theme.selection_pad,
        ));
        frame.render_widget(
            pad,
            Rect {
                y: below,
                height: 1,
                ..bounds
            },
        );
    }
}

/// Styles of one row, resolved for its selection state. A selected row loses the
/// dim tier and fills its full width, for a continuous background.
struct RowStyles {
    accent: Style,
    dim: Style,
    fill: Style,
    normal: Style,
    notification: Style,
    title: Style,
}

impl RowStyles {
    fn new(theme: &Theme, selected: bool) -> Self {
        if !selected {
            return Self {
                accent: theme.accent,
                dim: theme.dim,
                fill: Style::default(),
                normal: theme.normal,
                notification: theme.notification,
                title: theme.title,
            };
        }

        let selection = theme.selection;
        Self {
            accent: theme.accent.patch(selection),
            dim: theme.normal.patch(selection),
            fill: selection,
            normal: theme.normal.patch(selection),
            notification: theme.notification.patch(selection),
            title: theme.title.patch(selection),
        }
    }
}

/// Builds one list item: title line, detail line, and a blank separator. The
/// separator stays unstyled: it is where the selection padding is drawn.
fn window_item<'a>(window: &Window, tab: Tab, styles: &RowStyles, width: usize) -> ListItem<'a> {
    ListItem::new(vec![
        title_line(window, styles, width),
        detail_line(window, tab, styles, width),
        Line::default(),
    ])
}

/// Window title, an accent marker when this is the active window, then the
/// elapsed time and the notification slot pinned to the right edge.
fn title_line<'a>(window: &Window, styles: &RowStyles, width: usize) -> Line<'a> {
    let text_width = width.saturating_sub(PANEL_PADDING * 2);
    let time = format_elapsed(window.started_at);
    let marker = if window.is_active { "*" } else { "" };
    let right = time.width() + NOTIFICATION_SLOT;

    let budget = text_width.saturating_sub(right + marker.width() + 1);
    let title = truncate_end(&window.name, budget);
    let gap = text_width.saturating_sub(title.width() + marker.width() + right);

    let mut spans = vec![
        Span::styled(" ".repeat(PANEL_PADDING), styles.fill),
        Span::styled(title, styles.title),
    ];
    if !marker.is_empty() {
        spans.push(Span::styled(marker, styles.accent));
    }
    spans.push(Span::styled(" ".repeat(gap), styles.fill));
    spans.push(Span::styled(time, styles.dim));
    spans.push(Span::styled(" ", styles.fill));

    if window.notification_pending {
        let icon = notification_icon().to_string();
        let style = if icon == "!" {
            styles.notification.add_modifier(Modifier::BOLD)
        } else {
            styles.notification
        };
        spans.push(Span::styled(icon, style));
    } else {
        spans.push(Span::styled(" ", styles.fill));
    }

    spans.push(Span::styled(" ".repeat(PANEL_PADDING), styles.fill));
    Line::from(spans)
}

/// Agent icon and name (or the window icon), then the directory path, right-aligned
/// on the column where the elapsed time above it ends.
fn detail_line<'a>(window: &Window, tab: Tab, styles: &RowStyles, width: usize) -> Line<'a> {
    let text_width = width.saturating_sub(PANEL_PADDING * 2);
    let mut spans = vec![Span::styled(" ".repeat(PANEL_PADDING), styles.fill)];
    let mut used = 0usize;

    match tab {
        Tab::Agents => {
            if let Some(agent) = is_agent(&window.running_command) {
                let icon = agent_icon(agent.command()).to_string();
                if !icon.is_empty() {
                    used += icon.width() + 1;
                    spans.push(Span::styled(format!("{icon} "), styles.normal));
                }
                if window.name != agent.name() {
                    let name = agent.name().to_string();
                    used += name.width() + 2;
                    spans.push(Span::styled(name, styles.normal));
                    spans.push(Span::styled("  ", styles.fill));
                }
            }
        }
        Tab::Windows => {
            let icon = window_icon().to_string();
            used += icon.width() + 1;
            spans.push(Span::styled(format!("{icon} "), styles.dim));
        }
    }

    let right = text_width.saturating_sub(NOTIFICATION_SLOT);
    let path = shorten_path(Path::new(&window.current_dir), right.saturating_sub(used));
    let gap = right.saturating_sub(used + path.width());
    spans.push(Span::styled(" ".repeat(gap), styles.fill));
    spans.push(Span::styled(path, styles.dim));
    spans.push(Span::styled(
        " ".repeat(NOTIFICATION_SLOT + PANEL_PADDING),
        styles.fill,
    ));

    Line::from(spans)
}

/// Renders the footer with keybinding hints or confirmation message.
fn draw_footer(frame: &mut Frame, app: &mut App, area: Rect, theme: &Theme) {
    let footer = match app.pending_action() {
        Some(PendingAction::KillWindow) => {
            let msg = Line::from(vec![
                Span::styled("d", theme.footer_key),
                Span::styled(" kill this window", theme.dim),
            ]);
            Paragraph::new(msg)
        }
        Some(PendingAction::Quit) => {
            let msg = Line::from(vec![
                Span::styled("q", theme.footer_key),
                Span::styled(" quit", theme.dim),
            ]);
            Paragraph::new(msg)
        }
        None => {
            let entries = build_footer_entries(app, theme);
            let lines = wrap_entries(&entries, padded(area).width as usize);
            Paragraph::new(lines)
        }
    };
    frame.render_widget(footer, padded(area));
}

/// Builds the styled footer keybinding entries: the user's tmux bindings as
/// nested-session sequences while the pane is unfocused, the TUI keys
/// otherwise (also the fallback when tmux hints are unavailable).
fn build_footer_entries(app: &App, theme: &Theme) -> Vec<(Vec<Span<'static>>, usize)> {
    let entries: Vec<(String, String, bool)> = if !app.pane_active() && !app.tmux_hints().is_empty()
    {
        app.tmux_hints()
            .iter()
            .map(|(key, label)| (key.clone(), label.clone(), true))
            .collect()
    } else {
        let mut entries = vec![("↑↓".to_string(), "navigate".to_string(), true)];
        match app.active_tab() {
            Tab::Agents => entries.push((
                "→".to_string(),
                "windows".to_string(),
                !app.is_tab_empty(Tab::Windows),
            )),
            Tab::Windows => entries.push((
                "←".to_string(),
                "agents".to_string(),
                !app.is_tab_empty(Tab::Agents),
            )),
        }
        entries.extend([
            ("⏎".to_string(), "focus".to_string(), true),
            ("c".to_string(), "new".to_string(), true),
            (",".to_string(), "rename".to_string(), true),
            ("d".to_string(), "kill".to_string(), true),
            ("q".to_string(), "quit".to_string(), true),
        ]);
        entries
    };

    entries
        .iter()
        .map(|(key, desc, enabled)| {
            let key_style = if *enabled {
                theme.footer_key
            } else {
                theme.dim
            };
            let spans = vec![
                Span::styled(key.to_string(), key_style),
                Span::styled(format!(" {desc}"), theme.dim),
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

/// Formats an elapsed duration as a human-readable string, showing at most two units.
fn format_elapsed(started_at: Option<Instant>) -> String {
    match started_at {
        Some(start) => {
            let total_secs = Instant::now().duration_since(start).as_secs();
            let days = total_secs / 86_400;
            let rem = total_secs % 86_400;
            let hours = rem / 3_600;
            let rem = rem % 3_600;
            let minutes = rem / 60;
            let seconds = rem % 60;

            if days > 0 {
                format!("{}d {}h", days, hours)
            } else if hours > 0 {
                format!("{}h {}m", hours, minutes)
            } else if minutes > 0 {
                format!("{}m {}s", minutes, seconds)
            } else {
                format!("{}s", seconds)
            }
        }
        None => String::new(),
    }
}

/// Truncates text to a display width, marking the cut with an ellipsis.
fn truncate_end(text: &str, max_cols: usize) -> String {
    if text.width() <= max_cols {
        return text.to_string();
    }
    if max_cols == 0 {
        return String::new();
    }

    let mut truncated = String::new();
    let mut used = 0usize;
    for character in text.chars() {
        let char_width = character.width().unwrap_or(0);
        if used + char_width > max_cols - 1 {
            break;
        }
        truncated.push(character);
        used += char_width;
    }
    truncated.push_str(ELLIPSIS);
    truncated
}

/// Calculates how many lines the footer needs for the given width.
fn calculate_footer_height(width: u16, app: &App, theme: &Theme) -> u16 {
    let entries = build_footer_entries(app, theme);
    wrap_entries(&entries, width as usize).len() as u16
}

fn count_windows_for_tab(app: &App, tab: Tab) -> usize {
    app.windows()
        .iter()
        .filter(|w| match tab {
            Tab::Agents => is_agent(&w.running_command).is_some(),
            Tab::Windows => is_agent(&w.running_command).is_none(),
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::{KeyBinding, Tmux, TmuxError};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;
    use std::time::Duration;

    struct MockTmux {
        keys: Option<Vec<KeyBinding>>,
        next_id: std::cell::RefCell<u32>,
        prefix: Option<String>,
        windows: std::cell::RefCell<Vec<Window>>,
    }

    impl MockTmux {
        fn new() -> Self {
            Self::with_windows(vec![
                Window {
                    current_dir: "/home/user".to_string(),
                    id: 1,
                    is_active: false,
                    name: "w1".to_string(),
                    notification_pending: false,
                    running_command: "bash".to_string(),
                    started_at: Some(Instant::now() - Duration::from_secs(125)),
                },
                Window {
                    current_dir: "/home/user".to_string(),
                    id: 2,
                    is_active: false,
                    name: "w2".to_string(),
                    notification_pending: false,
                    running_command: "claude".to_string(),
                    started_at: Some(Instant::now() - Duration::from_secs(45)),
                },
            ])
        }

        fn with_windows(windows: Vec<Window>) -> Self {
            Self {
                keys: Some(default_key_bindings()),
                next_id: std::cell::RefCell::new(90),
                prefix: Some("C-b".to_string()),
                windows: std::cell::RefCell::new(windows),
            }
        }
    }

    fn default_key_bindings() -> Vec<KeyBinding> {
        [
            ("C-b", "send-prefix"),
            ("c", "new-window -c \"#{pane_current_path}\""),
            ("n", "next-window"),
            ("p", "previous-window"),
            ("l", "last-window"),
            (",", "command-prompt -I \"#W\" { rename-window \"%%\" }"),
        ]
        .into_iter()
        .map(|(key, command)| KeyBinding {
            key: key.to_string(),
            command: command.to_string(),
        })
        .collect()
    }

    impl Tmux for MockTmux {
        fn session_name(&self) -> &str {
            "agents-on-tmux"
        }

        fn create_session_if_not_exists(&self) -> Result<(), TmuxError> {
            Ok(())
        }
        fn attach_session(&self) -> Result<(), TmuxError> {
            Ok(())
        }
        fn list_windows(&self) -> Result<Vec<Window>, TmuxError> {
            Ok(self.windows.borrow().clone())
        }
        fn create_window(&self, name: &str) -> Result<Window, TmuxError> {
            let mut next_id = self.next_id.borrow_mut();
            let window = Window {
                current_dir: "/home/user".to_string(),
                id: *next_id,
                is_active: false,
                name: name.to_string(),
                notification_pending: false,
                running_command: String::new(),
                started_at: None,
            };
            *next_id += 1;
            self.windows.borrow_mut().push(window.clone());
            Ok(window)
        }
        fn kill_window(&self, id: u32) -> Result<(), TmuxError> {
            self.windows.borrow_mut().retain(|w| w.id != id);
            Ok(())
        }
        fn select_window(&self, _id: u32) -> Result<(), TmuxError> {
            Ok(())
        }
        fn last_pane(&self) -> Result<(), TmuxError> {
            Ok(())
        }
        fn split_window(&self, _command: &str, _width: u16) -> Result<String, TmuxError> {
            Ok("%99".to_string())
        }
        fn resize_pane(&self, _pane_id: &str, _width: u16) -> Result<(), TmuxError> {
            Ok(())
        }
        fn list_keys(&self, _table: &str) -> Result<Vec<KeyBinding>, TmuxError> {
            self.keys.clone().ok_or_else(|| command_failed("list-keys"))
        }
        fn show_options(&self, name: &str) -> Result<String, TmuxError> {
            match name {
                "prefix" => self
                    .prefix
                    .clone()
                    .ok_or_else(|| command_failed("show-options")),
                _ => Err(command_failed("show-options")),
            }
        }
        fn command_prompt(&self, _initial: &str, _template: &str) -> Result<(), TmuxError> {
            Ok(())
        }
    }

    fn command_failed(command: &str) -> TmuxError {
        TmuxError::CommandFailed {
            message: format!("{command} failed"),
            stderr: String::new(),
            code: Some(1),
        }
    }

    fn test_app() -> App {
        let nested_driver = MockTmux::new();
        let parent_driver = MockTmux::new();
        App::new(Box::new(nested_driver), Box::new(parent_driver), None, None).unwrap()
    }

    fn window(name: &str, command: &str, dir: &str, seconds: u64) -> Window {
        Window {
            current_dir: dir.to_string(),
            id: name.len() as u32 + command.len() as u32 * 100,
            is_active: false,
            name: name.to_string(),
            notification_pending: false,
            running_command: command.to_string(),
            started_at: Some(Instant::now() - Duration::from_secs(seconds)),
        }
    }

    /// Three agent windows, the first active, the second notifying. The directories
    /// sit outside any home, so rendering does not depend on who runs the tests.
    fn agents_app() -> App {
        let mut windows = vec![
            window(
                "fix-auth-bug",
                "claude",
                "/opt/work/Development/Rust/aot",
                29,
            ),
            window("docs-review", "opencode", "/opt/work/Development/docs", 25),
            window("billing-api", "pi", "/opt/clients/acme/billing-api", 17),
        ];
        windows[0].is_active = true;
        windows[1].notification_pending = true;
        app_with(windows)
    }

    fn app_with(windows: Vec<Window>) -> App {
        let driver = MockTmux::with_windows(windows.clone());
        App::new(
            Box::new(driver),
            Box::new(MockTmux::with_windows(windows)),
            None,
            None,
        )
        .unwrap()
    }

    /// Column of the elapsed time's last digit. Start times are re-stamped on refresh,
    /// so tests assert on placement, never on duration.
    fn time_column(row: &str) -> Option<usize> {
        row.chars()
            .enumerate()
            .filter(|(_, character)| character.is_ascii_digit())
            .map(|(column, _)| column)
            .last()
    }

    fn render(app: &mut App, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let theme = Theme::default();
        terminal.draw(|frame| draw(frame, app, &theme)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// The row as rendered, keeping leading columns so positions stay meaningful.
    fn row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// The row's text, without the panel padding around it.
    fn text(buffer: &Buffer, y: u16) -> String {
        row(buffer, y).trim().to_string()
    }

    fn selection_bg(buffer: &Buffer, x: u16, y: u16) -> bool {
        buffer[(x, y)].style().bg == Some(Color::DarkGray)
    }

    /// Column of the last path character on a detail line.
    fn path_end(buffer: &Buffer, y: u16) -> u16 {
        row(buffer, y).width() as u16 - 1
    }

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
    fn test_format_elapsed_hours_and_minutes() {
        let start = Instant::now() - Duration::from_secs(3 * 3_600 + 28 * 60);
        assert_eq!(format_elapsed(Some(start)), "3h 28m");
    }

    #[test]
    fn test_format_elapsed_days_and_hours() {
        let start = Instant::now() - Duration::from_secs(28 * 86_400 + 3 * 3_600);
        assert_eq!(format_elapsed(Some(start)), "28d 3h");
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
        let app = test_app();
        assert_eq!(calculate_footer_height(120, &app, &Theme::default()), 1);
    }

    #[test]
    fn test_calculate_footer_height_narrow() {
        let app = test_app();
        assert_eq!(calculate_footer_height(30, &app, &Theme::default()), 3);
    }

    /// An app whose pane is unfocused while tmux bindings were resolved.
    fn unfocused_app() -> App {
        let driver = MockTmux::new();
        let parent = MockTmux::new();
        let mut app = App::new(
            Box::new(driver),
            Box::new(parent),
            None,
            Some("%7".to_string()),
        )
        .unwrap();
        app.set_pane_active(false);
        app
    }

    /// Every rendered row joined by newlines, trailing blanks trimmed.
    fn all_text(buffer: &Buffer) -> String {
        (0..buffer.area.height)
            .map(|y| text(buffer, y))
            .collect::<Vec<String>>()
            .join("\n")
            .trim_end()
            .to_string()
    }

    #[test]
    fn test_unfocused_app_tracks_focus_and_hints() {
        let app = unfocused_app();
        assert!(!app.pane_active());
        assert_eq!(app.tmux_hints().len(), 6);
    }

    #[test]
    fn test_footer_shows_tui_keys_when_focused() {
        let mut app = test_app();
        let buffer = render(&mut app, 35, 12);
        assert!(all_text(&buffer).contains("quit"));
        assert!(!all_text(&buffer).contains("C-b"));
    }

    #[test]
    fn test_footer_shows_tmux_keys_when_unfocused() {
        let mut app = unfocused_app();
        let buffer = render(&mut app, 35, 12);
        let rendered = all_text(&buffer);
        assert!(rendered.contains("C-b C-b prefix"));
        assert!(rendered.contains("c new"));
        assert!(rendered.contains("n next"));
        assert!(rendered.contains("p prev"));
        assert!(rendered.contains("l last"));
        assert!(rendered.contains(", rename"));
        assert!(!rendered.contains("quit"));
    }

    #[test]
    fn test_footer_falls_back_to_tui_keys_without_hints() {
        let driver = MockTmux::new();
        let mut parent = MockTmux::new();
        parent.keys = None;
        let mut app = App::new(
            Box::new(driver),
            Box::new(parent),
            None,
            Some("%7".to_string()),
        )
        .unwrap();

        let buffer = render(&mut app, 35, 12);
        let rendered = all_text(&buffer);
        assert!(rendered.contains("quit"));
        assert!(!rendered.contains("C-b"));
    }

    #[test]
    fn test_footer_confirmation_wins_over_tmux_keys() {
        use crate::frontends::tui::event::Action;

        let mut app = unfocused_app();
        app.handle_action(Action::KillWindow);
        let buffer = render(&mut app, 35, 12);
        let rendered = all_text(&buffer);
        assert!(rendered.contains("kill this window"));
        assert!(!rendered.contains("C-b"));
    }

    #[test]
    fn test_footer_height_covers_tmux_keys() {
        let app = unfocused_app();
        let theme = Theme::default();
        // 31 usable columns: "C-b C-b prefix" + "c new" + "n next" fit on one
        // line, the rest wraps to a second.
        assert_eq!(calculate_footer_height(35, &app, &theme), 2);
        // 16 usable columns: one or two entries per line.
        assert_eq!(calculate_footer_height(20, &app, &theme), 4);
    }

    #[test]
    fn test_footer_height_matches_rendered_lines() {
        let mut app = unfocused_app();
        let theme = Theme::default();
        let expected = calculate_footer_height(35, &app, &theme);

        let buffer = render(&mut app, 35, 12);
        let footer_rows: Vec<String> = (0..buffer.area.height)
            .map(|y| text(&buffer, y))
            .skip(buffer.area.height as usize - expected as usize)
            .collect();
        assert_eq!(footer_rows.len(), expected as usize);
        assert!(footer_rows.iter().all(|row| !row.is_empty()));
    }

    #[test]
    fn test_truncate_end_fits() {
        assert_eq!(truncate_end("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_end_exact_fit() {
        assert_eq!(truncate_end("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_end_needs_truncation() {
        assert_eq!(truncate_end("fix-auth-bug", 6), "fix-a\u{2026}");
    }

    #[test]
    fn test_truncate_end_very_narrow() {
        assert_eq!(truncate_end("hello", 1), "\u{2026}");
        assert_eq!(truncate_end("hello", 0), "");
    }

    #[test]
    fn test_truncate_end_empty() {
        assert_eq!(truncate_end("", 5), "");
    }

    #[test]
    fn test_truncate_end_keeps_wide_characters_whole() {
        assert_eq!(truncate_end("\u{6587}\u{4ef6}ab", 3), "\u{6587}\u{2026}");
    }

    #[test]
    fn test_count_windows_for_tab() {
        let app = test_app();
        assert_eq!(count_windows_for_tab(&app, Tab::Agents), 1);
        assert_eq!(count_windows_for_tab(&app, Tab::Windows), 1);
    }

    #[test]
    fn test_tab_bar_labels_and_counts() {
        let mut app = agents_app();
        let buffer = render(&mut app, 80, 24);
        assert_eq!(text(&buffer, 0), "agents-on-tmux");
        assert_eq!(text(&buffer, 1), "Agents (3)   Windows (0)");
    }

    #[test]
    fn test_tab_bar_accent_sits_under_the_active_label() {
        let mut app = agents_app();
        let buffer = render(&mut app, 80, 24);

        let accent = Theme::default().accent.fg;
        let end = PANEL_PADDING + "Agents (3)".width();
        for x in PANEL_PADDING..end {
            assert_eq!(buffer[(x as u16, 2)].symbol(), TAB_RULE);
            assert_eq!(buffer[(x as u16, 2)].style().fg, accent, "column {x}");
        }
        for x in (0..PANEL_PADDING).chain(end..80) {
            assert_ne!(buffer[(x as u16, 2)].style().fg, accent, "column {x}");
        }
        assert_eq!(buffer[(79, 2)].symbol(), TAB_RULE);
    }

    #[test]
    fn test_tab_count_is_emphasized_with_its_label() {
        let mut app = agents_app();
        let buffer = render(&mut app, 80, 24);

        let active_count = (PANEL_PADDING + "Agents (".width()) as u16;
        assert_eq!(buffer[(active_count, 1)].symbol(), "3");
        assert!(
            buffer[(active_count, 1)]
                .style()
                .add_modifier
                .contains(Modifier::BOLD)
        );

        let idle_count =
            (PANEL_PADDING + "Agents (3)".width() + TAB_GAP.width() + "Windows (".width()) as u16;
        assert_eq!(buffer[(idle_count, 1)].symbol(), "0");
        assert!(
            buffer[(idle_count, 1)]
                .style()
                .add_modifier
                .contains(Modifier::DIM)
        );
    }

    #[test]
    fn test_tab_bar_accent_follows_the_active_tab() {
        let mut app = app_with(vec![
            window("fix-auth-bug", "claude", "/opt/work/aot", 29),
            window("shell", "zsh", "/opt/work/aot", 5),
        ]);
        app.switch_tab(Tab::Windows);
        assert_eq!(app.active_tab(), Tab::Windows);
        let buffer = render(&mut app, 80, 24);
        let accent = Theme::default().accent.fg;

        let start = PANEL_PADDING + "Agents (3)".width() + TAB_GAP.width();
        for x in 0..start {
            assert_ne!(buffer[(x as u16, 2)].style().fg, accent, "column {x}");
        }
        for x in start..start + "Windows (1)".width() {
            assert_eq!(buffer[(x as u16, 2)].style().fg, accent, "column {x}");
        }
    }

    #[test]
    fn test_paths_are_right_aligned_with_the_elapsed_times() {
        let mut app = agents_app();
        let buffer = render(&mut app, 80, 24);

        assert_eq!(path_end(&buffer, 4), path_end(&buffer, 7));
        assert_eq!(path_end(&buffer, 7), path_end(&buffer, 10));
        assert_eq!(
            Some(path_end(&buffer, 4) as usize),
            time_column(&row(&buffer, 3)).map(|column| column + 1)
        );
    }

    #[test]
    fn test_rows_are_two_lines_with_a_blank_separator() {
        let mut app = agents_app();
        let buffer = render(&mut app, 80, 24);

        assert!(text(&buffer, 3).starts_with("fix-auth-bug*"));
        assert!(text(&buffer, 4).ends_with("/opt/work/Development/Rust/aot"));
        assert!(text(&buffer, 6).starts_with("docs-review"));
        assert!(text(&buffer, 7).ends_with("/opt/work/Development/docs"));
        assert_eq!(text(&buffer, 8), "");
        assert!(text(&buffer, 9).starts_with("billing-api"));
        assert!(text(&buffer, 10).ends_with("/opt/clients/acme/billing-api"));
        assert_eq!(text(&buffer, 11), "");
    }

    #[test]
    fn test_active_window_marker_uses_the_accent() {
        let mut app = agents_app();
        let buffer = render(&mut app, 80, 24);
        let marker_x = (PANEL_PADDING + "fix-auth-bug".width()) as u16;
        assert_eq!(buffer[(marker_x, 3)].symbol(), "*");
        assert_eq!(buffer[(marker_x, 3)].style().fg, Theme::default().accent.fg);
        assert_ne!(
            buffer[((PANEL_PADDING + "docs-review".width()) as u16, 6)].symbol(),
            "*"
        );
    }

    #[test]
    fn test_notification_slot_keeps_the_time_column_fixed() {
        let mut app = agents_app();
        let buffer = render(&mut app, 80, 24);

        let quiet = row(&buffer, 3); // no notification
        let notifying = row(&buffer, 6); // notification pending
        assert_eq!(time_column(&quiet), time_column(&notifying));

        assert_eq!(buffer[(78, 6)].symbol(), "!");
        assert_eq!(buffer[(78, 6)].style().fg, Theme::default().notification.fg);
        assert!(
            buffer[(78, 6)]
                .style()
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(buffer[(78, 3)].symbol(), " ");
        assert_eq!(buffer[(77, 3)].symbol(), " ");
    }

    #[test]
    fn test_selection_covers_both_content_lines_edge_to_edge() {
        let mut app = agents_app();
        let buffer = render(&mut app, 80, 24);

        for y in [3u16, 4] {
            for x in 0..80u16 {
                assert!(
                    selection_bg(&buffer, x, y),
                    "cell ({x}, {y}) is not selected"
                );
            }
        }
        assert!(selection_bg(&buffer, 0, 2));
        assert!(!selection_bg(&buffer, 0, 5));
        assert_eq!(buffer[(0, 5)].style().fg, Some(Color::DarkGray));
        assert!(
            !buffer[(1, 3)]
                .style()
                .add_modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn test_selected_row_drops_the_dim_tier() {
        let mut app = agents_app();
        let buffer = render(&mut app, 80, 24);

        let time = time_column(&row(&buffer, 3)).unwrap() as u16;
        assert!(
            !buffer[(time, 3)]
                .style()
                .add_modifier
                .contains(Modifier::DIM)
        );
        assert!(
            buffer[(time, 6)]
                .style()
                .add_modifier
                .contains(Modifier::DIM)
        );

        let selected_path = path_end(&buffer, 4);
        let other_path = path_end(&buffer, 7);
        assert!(
            !buffer[(selected_path, 4)]
                .style()
                .add_modifier
                .contains(Modifier::DIM)
        );
        assert!(
            buffer[(other_path, 7)]
                .style()
                .add_modifier
                .contains(Modifier::DIM)
        );

        assert!(buffer[(1, 3)].style().add_modifier.contains(Modifier::BOLD));
        let marker = (PANEL_PADDING + "fix-auth-bug".width()) as u16;
        assert_eq!(buffer[(marker, 3)].style().fg, Theme::default().accent.fg);
    }

    #[test]
    fn test_selection_padding_half_blocks() {
        let mut app = agents_app();
        let buffer = render(&mut app, 80, 24);

        assert_eq!(row(&buffer, 2), TAB_RULE.repeat(80));
        assert!(selection_bg(&buffer, 0, 2));
        assert_eq!(buffer[(1, 2)].style().fg, Theme::default().accent.fg);
        assert!(selection_bg(&buffer, 1, 2));

        assert_eq!(row(&buffer, 5), HALF_BLOCK_UPPER.repeat(80));
        assert_eq!(buffer[(0, 5)].style().fg, Some(Color::DarkGray));
        assert_eq!(buffer[(0, 5)].style().bg, Some(Color::Reset));
    }

    #[test]
    fn test_selection_moves_without_shifting_rows() {
        let mut app = agents_app();
        let before = render(&mut app, 80, 24);
        app.navigate_down();
        let after = render(&mut app, 80, 24);

        for y in [3u16, 6, 9] {
            assert_eq!(
                row(&before, y).trim_start_matches(HALF_BLOCK_LOWER),
                row(&after, y).trim_start_matches(HALF_BLOCK_LOWER),
                "row {y} moved"
            );
        }
        assert!(selection_bg(&after, 0, 6));
        assert!(!selection_bg(&after, 0, 3));
        assert_eq!(row(&after, 5), HALF_BLOCK_LOWER.repeat(80));
        assert_eq!(row(&after, 8), HALF_BLOCK_UPPER.repeat(80));
        assert!(selection_bg(&before, 0, 2));
        assert!(!selection_bg(&after, 0, 2));
    }

    #[test]
    fn test_layout_is_identical_in_a_narrow_sidebar() {
        let mut app = agents_app();
        let buffer = render(&mut app, 30, 20);

        assert_eq!(text(&buffer, 1), "Agents (3)   Windows (0)");
        assert!(text(&buffer, 3).starts_with("fix-auth-bug*"));
        assert_eq!(time_column(&row(&buffer, 3)), Some(25));
        assert_eq!(row(&buffer, 5), HALF_BLOCK_UPPER.repeat(30));
        assert!(text(&buffer, 6).starts_with("docs-review"));
        assert!(
            text(&buffer, 4).ends_with("/o/w/D/R/aot"),
            "{:?}",
            text(&buffer, 4)
        );
        assert!(row(&buffer, 4).width() <= 30);
    }

    #[test]
    fn test_layout_is_identical_in_a_small_popup() {
        let mut app = agents_app();
        let buffer = render(&mut app, 40, 12);

        assert_eq!(text(&buffer, 1), "Agents (3)   Windows (0)");
        assert!(text(&buffer, 3).starts_with("fix-auth-bug*"));
        assert_eq!(time_column(&row(&buffer, 3)), Some(35));
        assert_eq!(row(&buffer, 5), HALF_BLOCK_UPPER.repeat(40));
        assert!(text(&buffer, 6).starts_with("docs-review"));
        assert!(text(&buffer, 7).ends_with("/o/w/D/docs"));
    }

    #[test]
    fn test_agent_row_shows_icon_and_name() {
        let mut app = agents_app();
        let buffer = render(&mut app, 80, 24);
        assert!(text(&buffer, 4).starts_with("[cc] Claude"));
        assert!(text(&buffer, 7).starts_with("[oc] OpenCode"));
    }

    #[test]
    fn test_agent_name_is_suppressed_when_it_is_the_window_title() {
        let mut app = app_with(vec![window("Claude", "claude", "/opt/project", 5)]);
        let buffer = render(&mut app, 80, 24);
        assert!(text(&buffer, 3).starts_with("Claude"));
        assert!(text(&buffer, 4).starts_with("[cc]"));
        assert!(text(&buffer, 4).ends_with("/opt/project"));
    }

    #[test]
    fn test_windows_tab_uses_the_window_icon_and_no_agent_name() {
        let mut app = app_with(vec![window("shell", "zsh", "/opt/work/aot", 240)]);
        assert_eq!(app.active_tab(), Tab::Windows);

        let buffer = render(&mut app, 80, 24);
        assert!(text(&buffer, 3).starts_with("shell"));
        assert_eq!(time_column(&row(&buffer, 3)), Some(75));
        assert!(text(&buffer, 4).starts_with("[w]"));
        assert!(text(&buffer, 4).ends_with("/opt/work/aot"));
    }

    #[test]
    fn test_empty_tab_message() {
        let mut app = app_with(vec![window("shell", "zsh", "/opt/work", 5)]);
        app.kill_window();
        assert_eq!(app.current_tab_len(), 0);

        let buffer = render(&mut app, 80, 24);
        assert_eq!(text(&buffer, 1), "Agents (0)   Windows (0)");
        assert_eq!(text(&buffer, 3), "No windows");
    }

    #[test]
    fn test_long_title_is_truncated_before_the_time() {
        let mut app = app_with(vec![window(
            "a-very-long-window-name-that-cannot-possibly-fit",
            "claude",
            "/opt/work",
            5,
        )]);
        let buffer = render(&mut app, 30, 20);
        let title_row = row(&buffer, 3);
        assert!(title_row.contains(ELLIPSIS), "{title_row:?}");
        assert_eq!(time_column(&title_row), Some(25));
        assert!(title_row.width() <= 30);
    }
}

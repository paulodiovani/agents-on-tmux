use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event;
use ratatui::DefaultTerminal;
use ratatui::layout::{Rect, Size};

use super::state::App;
use crate::backends::logger;
use crate::backends::signals;
use crate::backends::tmux::{EventProducer, TmuxEvent};
use crate::frontends::tui::event::{Action, PendingAction, key_to_action};
use crate::frontends::tui::mouse::MouseAction;
use crate::frontends::tui::theme::Theme;
use crate::frontends::tui::ui::{self, RegionAction};

impl App {
    /// Runs the main event loop, drawing the UI and handling input.
    /// Data refresh is event-driven (tmux control mode); the ~1s redraw tick
    /// only repaints so the uptime counters keep ticking.
    pub fn run(&mut self, mut terminal: DefaultTerminal) -> anyhow::Result<()> {
        let theme = Theme::default();

        // The parent session only reports pane focus, for the keybinding hints;
        // the nested one reports window changes.
        let (event_tx, event_rx) = mpsc::channel();
        self.parent_driver
            .watch_events(EventProducer::Parent, event_tx.clone());
        self.nested_driver
            .watch_events(EventProducer::Nested, event_tx);
        self.event_rx = Some(event_rx);

        // The terminal may have been resized between the split and TUI
        // startup.
        self.enforce_panel_width(terminal.size()?.width);

        let redraw_tick = Duration::from_secs(1);
        let mut last_draw = redraw_due(redraw_tick);
        while self.running {
            if last_draw.elapsed() >= redraw_tick {
                terminal.draw(|frame| ui::draw(frame, self, &theme))?;
                last_draw = redraw_due(redraw_tick);
            }

            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    event::Event::Key(key) if key.kind == event::KeyEventKind::Press => {
                        self.handle_keyboard_action(key_to_action(key));
                        last_draw = redraw_due(redraw_tick);
                    }

                    event::Event::Resize(width, _) => {
                        self.enforce_panel_width(width);
                        self.invalidate_clickable_regions();
                        last_draw = redraw_due(redraw_tick);
                    }

                    event::Event::Mouse(mouse_event) => {
                        let size = terminal.size()?;

                        if self.handle_mouse_action(mouse_event.into(), size) {
                            last_draw = redraw_due(redraw_tick);
                        }
                    }
                    _ => {}
                }
            }

            if self.process_tmux_events() {
                self.invalidate_clickable_regions();
                last_draw = redraw_due(redraw_tick);
            }

            if let Some((signo, name)) = signals::received() {
                logger::error!("app: received {name} ({signo}), shutting down");
                self.running = false;
            }
        }

        Ok(())
    }

    /// Re-asserts the side panel width after tmux rescaled the layout. No-op
    /// when enforcement is disabled or when the width already matches, which
    /// makes the enforcement converge without loops or redundant tmux calls.
    /// Failures (e.g. terminal narrower than the panel) are logged and ignored.
    fn enforce_panel_width(&self, current_width: u16) {
        if let Some(target) = self.panel_width
            && let Some(pane_id) = &self.pane_id
            && current_width != target
        {
            logger::debug!("app: enforce panel width {target} (was {current_width})");
            if let Err(error) = self.parent_driver.resize_pane(pane_id, target) {
                logger::error!("app: resize pane failed: {error}");
            }
        }
    }

    /// Drains pending control-mode events, refreshing the window list at most
    /// once per drain (events like %output arrive in bursts) and quitting on Exit.
    /// Returns whether the window list was refreshed (i.e. a redraw is due).
    fn process_tmux_events(&mut self) -> bool {
        let mut needs_refresh = false;
        let mut should_exit = false;
        let mut new_pane_id = None;

        if let Some(rx) = &self.event_rx {
            while let Ok(event) = rx.try_recv() {
                match event {
                    TmuxEvent::Exit => should_exit = true,
                    TmuxEvent::PaneChanged(id) => new_pane_id = Some(id),
                    TmuxEvent::Refresh => needs_refresh = true,
                }
            }
        }

        if let Some(id) = new_pane_id {
            if let Some(own_id) = &self.pane_id {
                self.set_pane_active(*own_id == id);
            }
            needs_refresh = true;
        }

        if needs_refresh && let Err(error) = self.refresh_windows() {
            logger::error!("app: refresh windows failed: {error}");
        }
        if should_exit {
            self.running = false;
        }
        needs_refresh
    }

    /// Dispatches a keyboard action to the appropriate handler.
    fn handle_keyboard_action(&mut self, action: Action) {
        match action {
            Action::KillWindow if self.pending_action == Some(PendingAction::KillWindow) => {
                self.kill_window();
                self.invalidate_clickable_regions();
                self.pending_action = None;
            }
            Action::KillWindow => self.pending_action = Some(PendingAction::KillWindow),
            Action::Quit if self.pending_action == Some(PendingAction::Quit) => {
                self.quit();
                self.pending_action = None;
            }
            Action::Quit => self.pending_action = Some(PendingAction::Quit),
            Action::None => self.pending_action = None,
            _ => {
                self.pending_action = None;
                match action {
                    Action::NavigateUp => self.navigate_up(),
                    Action::NavigateDown => self.navigate_down(),
                    Action::FocusWindow => self.focus_window(),
                    Action::CreateWindow => {
                        self.create_window();
                        self.invalidate_clickable_regions();
                    }
                    Action::RenameWindow => self.rename_window(),
                    Action::SwitchTabLeft => {
                        self.switch_tab(self.active_tab().left());
                        self.invalidate_clickable_regions();
                    }
                    Action::SwitchTabRight => {
                        self.switch_tab(self.active_tab().right());
                        self.invalidate_clickable_regions();
                    }
                    _ => {}
                }
            }
        }
    }

    /// Handles a mouse action. Returns `true` if the UI should be redrawn.
    fn handle_mouse_action(&mut self, action: MouseAction, terminal_size: Size) -> bool {
        let rect = Rect::new(0, 0, terminal_size.width, terminal_size.height);
        let regions = self.clickable_regions(rect);

        match action {
            MouseAction::Click(x, y)
                if let Some(RegionAction::SelectTab(tab)) = find_region_action(&regions, x, y) =>
            {
                self.switch_tab(*tab);
                self.invalidate_clickable_regions();
                false
            }

            MouseAction::Click(x, y)
                if let Some(RegionAction::SelectWindow(index)) =
                    find_region_action(&regions, x, y) =>
            {
                self.set_current_selected(*index);
                self.focus_window();
                false
            }

            MouseAction::Hover(x, y)
                if let Some(RegionAction::SelectWindow(index)) =
                    find_region_action(&regions, x, y)
                    && *index != self.current_selected() =>
            {
                self.set_current_selected(*index);
                false
            }

            MouseAction::Scroll(direction) => {
                let list_height = terminal_size.height.saturating_sub(4);
                let visible_count = (list_height / ui::ROW_HEIGHT) as usize;
                self.scroll(direction, visible_count);
                self.invalidate_clickable_regions();
                true
            }

            _ => false,
        }
    }
}

/// An instant old enough that a redraw with the given tick is already due.
fn redraw_due(tick: Duration) -> Instant {
    let now = Instant::now();
    now.checked_sub(tick).unwrap_or(now)
}

/// Returns the action of the clickable region at the given terminal coordinates, if any.
fn find_region_action(regions: &[ui::ClickableRegion], x: u16, y: u16) -> Option<&RegionAction> {
    regions
        .iter()
        .find(|region| ui::contains(region.area, x, y))
        .map(|region| &region.action)
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::{new_app, session, test_app};
    use super::*;
    use crate::backends::tmux::Window;
    use crate::frontends::tui::event::Tab;

    const SIZE: Size = Size {
        width: 40,
        height: 30,
    };

    #[test]
    fn test_click_on_tab_label_switches_tab() {
        let (mut app, _) = test_app();
        let windows_label = ui::clickable_regions(Rect::new(0, 0, SIZE.width, SIZE.height), &app)
            .into_iter()
            .find(|region| matches!(region.action, RegionAction::SelectTab(Tab::Windows)))
            .unwrap()
            .area;

        let redraw =
            app.handle_mouse_action(MouseAction::Click(windows_label.x, windows_label.y), SIZE);

        assert!(!redraw);
        assert_eq!(app.active_tab(), Tab::Windows);
    }

    #[test]
    fn test_click_on_row_selects_and_focuses_window() {
        let (mut app, tmux) = test_app();
        let second_row = row_area(&app, 1);

        app.handle_mouse_action(MouseAction::Click(second_row.x, second_row.y), SIZE);

        assert_eq!(app.current_selected(), 1);
        assert!(tmux.recorded().contains(&"select_window 4".to_string()));
    }

    #[test]
    fn test_hover_on_row_selects_it() {
        let (mut app, tmux) = test_app();
        let second_row = row_area(&app, 1);

        app.handle_mouse_action(MouseAction::Hover(second_row.x, second_row.y), SIZE);

        assert_eq!(app.current_selected(), 1);
        assert!(
            !tmux
                .recorded()
                .iter()
                .any(|c| c.starts_with("select_window"))
        );
    }

    #[test]
    fn test_scroll_requests_redraw() {
        let (mut app, _) = test_app();
        assert!(app.handle_mouse_action(MouseAction::Scroll(1), SIZE));
    }

    #[test]
    fn test_click_outside_regions_is_ignored() {
        let (mut app, _) = test_app();
        assert!(!app.handle_mouse_action(MouseAction::Click(0, SIZE.height - 1), SIZE));
        assert_eq!(app.current_selected(), 0);
        assert_eq!(app.active_tab(), Tab::Agents);
    }

    fn row_area(app: &App, index: usize) -> Rect {
        ui::clickable_regions(Rect::new(0, 0, SIZE.width, SIZE.height), app)
            .into_iter()
            .find(|region| matches!(region.action, RegionAction::SelectWindow(i) if i == index))
            .unwrap()
            .area
    }

    #[test]
    fn test_handle_action_triggers_rename() {
        let nested = session();
        let mut app = new_app(&nested, &session(), None, None);

        app.handle_keyboard_action(Action::RenameWindow);

        assert!(
            nested
                .recorded()
                .iter()
                .any(|call| call.starts_with("prompt_rename_window"))
        );
    }

    #[test]
    fn test_handle_action_quit() {
        let (mut app, _) = test_app();
        app.handle_keyboard_action(Action::Quit);
        assert!(app.running);
        app.handle_keyboard_action(Action::Quit);
        assert!(!app.running);
    }

    #[test]
    fn test_handle_action_navigate() {
        let (mut app, _) = test_app();
        app.handle_keyboard_action(Action::NavigateDown);
        assert_eq!(app.current_selected(), 1);
        app.handle_keyboard_action(Action::NavigateUp);
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_kill_window_requires_double_press() {
        let (mut app, _) = test_app();
        let initial_agents = app.current_tab_len();
        assert_eq!(app.pending_action(), None);
        app.handle_keyboard_action(Action::KillWindow);
        assert_eq!(app.pending_action(), Some(PendingAction::KillWindow));
        assert_eq!(app.current_tab_len(), initial_agents);
        app.handle_keyboard_action(Action::KillWindow);
        assert_eq!(app.pending_action(), None);
        assert_eq!(app.current_tab_len(), initial_agents - 1);
    }

    #[test]
    fn test_quit_requires_double_press() {
        let (mut app, _) = test_app();
        assert!(app.running);
        assert_eq!(app.pending_action(), None);
        app.handle_keyboard_action(Action::Quit);
        assert_eq!(app.pending_action(), Some(PendingAction::Quit));
        assert!(app.running);
        app.handle_keyboard_action(Action::Quit);
        assert_eq!(app.pending_action(), None);
        assert!(!app.running);
    }

    #[test]
    fn test_other_action_cancels_pending_kill() {
        let (mut app, _) = test_app();
        app.handle_keyboard_action(Action::KillWindow);
        assert_eq!(app.pending_action(), Some(PendingAction::KillWindow));
        app.handle_keyboard_action(Action::NavigateDown);
        assert_eq!(app.pending_action(), None);
    }

    #[test]
    fn test_other_action_cancels_pending_quit() {
        let (mut app, _) = test_app();
        app.handle_keyboard_action(Action::Quit);
        assert_eq!(app.pending_action(), Some(PendingAction::Quit));
        app.handle_keyboard_action(Action::NavigateDown);
        assert_eq!(app.pending_action(), None);
        assert!(app.running);
    }

    #[test]
    fn test_none_action_clears_pending() {
        let (mut app, _) = test_app();
        app.handle_keyboard_action(Action::KillWindow);
        assert_eq!(app.pending_action(), Some(PendingAction::KillWindow));
        app.handle_keyboard_action(Action::None);
        assert_eq!(app.pending_action(), None);

        app.handle_keyboard_action(Action::Quit);
        assert_eq!(app.pending_action(), Some(PendingAction::Quit));
        app.handle_keyboard_action(Action::None);
        assert_eq!(app.pending_action(), None);
    }

    #[test]
    fn test_app_new_has_no_event_receiver() {
        let (app, _) = test_app();
        assert!(app.event_rx.is_none());
    }

    #[test]
    fn test_process_tmux_events_without_receiver_is_noop() {
        let (mut app, _) = test_app();
        assert!(!app.process_tmux_events());
        assert!(app.running);
        assert_eq!(app.windows().len(), 4);
    }

    #[test]
    fn test_refresh_event_reloads_windows() {
        let (mut app, tmux) = test_app();
        let (tx, rx) = mpsc::channel();
        app.event_rx = Some(rx);

        // A structural change happened externally; the event tells us to reload.
        tmux.windows.lock().unwrap().push(Window {
            id: 99,
            name: "window-5".to_string(),
            pane_pid: 10005,
            running_command: "bash".to_string(),
            current_dir: "/home/user/project5".to_string(),
            is_active: false,
            notification_pending: false,
            started_at: None,
        });
        let _ = tx.send(TmuxEvent::Refresh);

        assert!(app.process_tmux_events());
        assert_eq!(app.windows().len(), 5);
    }

    #[test]
    fn test_multiple_events_refresh_once() {
        // Coalescing: many events, but list_windows is re-read a single time.
        let (mut app, tmux) = test_app();
        let (tx, rx) = mpsc::channel();
        app.event_rx = Some(rx);
        tmux.calls.lock().unwrap().clear();

        for _ in 0..10 {
            let _ = tx.send(TmuxEvent::Refresh);
        }
        app.process_tmux_events();

        let list_calls = tmux
            .recorded()
            .iter()
            .filter(|c| *c == "list_windows")
            .count();
        assert_eq!(list_calls, 1);
        assert!(app.running);
    }

    #[test]
    fn test_exit_event_quits_app() {
        let (mut app, _) = test_app();
        let (tx, rx) = mpsc::channel();
        app.event_rx = Some(rx);

        let _ = tx.send(TmuxEvent::Exit);
        assert!(!app.process_tmux_events());

        assert!(!app.running);
    }

    #[test]
    fn test_pane_changed_matching_own_pane_sets_active() {
        let (mut app, _) = test_app();
        app.pane_id = Some("%5".to_string());
        app.set_pane_active(false);
        let (tx, rx) = mpsc::channel();
        app.event_rx = Some(rx);

        let _ = tx.send(TmuxEvent::PaneChanged("%5".to_string()));
        assert!(app.process_tmux_events());
        assert!(app.pane_active());
    }

    #[test]
    fn test_pane_changed_not_matching_own_pane_sets_inactive() {
        let (mut app, _) = test_app();
        app.pane_id = Some("%5".to_string());
        let (tx, rx) = mpsc::channel();
        app.event_rx = Some(rx);

        let _ = tx.send(TmuxEvent::PaneChanged("%6".to_string()));
        assert!(app.process_tmux_events());
        assert!(!app.pane_active());
    }

    #[test]
    fn test_pane_changed_without_own_pane_id_keeps_active() {
        let (mut app, _) = test_app();
        app.pane_id = None;
        let (tx, rx) = mpsc::channel();
        app.event_rx = Some(rx);

        let _ = tx.send(TmuxEvent::PaneChanged("%6".to_string()));
        assert!(app.process_tmux_events());
        assert!(app.pane_active());
    }

    #[test]
    fn test_enforce_panel_width_resizes_when_width_differs() {
        let parent = session();
        let app = new_app(&session(), &parent, Some("%5"), Some(35));

        app.enforce_panel_width(50);

        assert_eq!(
            parent.recorded().as_slice(),
            ["resize_pane %5 35".to_string()]
        );
    }

    #[test]
    fn test_enforce_panel_width_noop_when_width_matches() {
        let parent = session();
        let app = new_app(&session(), &parent, Some("%5"), Some(35));

        app.enforce_panel_width(35);

        assert!(parent.recorded().is_empty());
    }

    #[test]
    fn test_enforce_panel_width_noop_without_panel_width() {
        let parent = session();
        let app = new_app(&session(), &parent, None, None);
        assert_eq!(app.panel_width, None);

        app.enforce_panel_width(50);

        assert!(parent.recorded().is_empty());
    }

    #[test]
    fn test_enforce_panel_width_logs_error_on_failure() {
        let path = crate::backends::logger::tests::temp_log_path();
        let _guard = crate::backends::logger::tests::init_for_test(&path);

        let mut parent = session();
        parent.resize_fails = true;
        let app = new_app(&session(), &parent, Some("%5"), Some(35));

        app.enforce_panel_width(50);

        let log_content = std::fs::read_to_string(&path).unwrap();
        assert!(log_content.contains("ERROR app: resize pane failed"));
    }

    #[test]
    fn test_redraw_due_is_one_tick_in_the_past() {
        let tick = Duration::from_millis(10);
        assert!(redraw_due(tick).elapsed() >= tick);
    }

    #[test]
    fn test_redraw_due_survives_unrepresentable_tick() {
        assert!(redraw_due(Duration::MAX).elapsed() < Duration::from_secs(60));
    }
}

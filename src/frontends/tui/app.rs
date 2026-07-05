use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossterm::event;
use ratatui::DefaultTerminal;

use crate::backends::{Tmux, Window};
use crate::frontends::tui::event::{Action, key_to_action};
use crate::frontends::tui::theme::Theme;
use crate::frontends::tui::ui;

const REFRESH_INTERVAL_SECS: u64 = 5;

/// Main application state for the TUI frontend.
pub struct App {
    running: bool,
    selected: usize,
    windows: Vec<Window>,
    pending_kill: bool,
    window_starts: HashMap<u32, Instant>,
    last_focused_id: Option<u32>,
}

impl App {
    /// Creates a new App, loading windows from the tmux driver.
    pub fn new<T: Tmux>(driver: &T) -> Result<Self, Box<dyn std::error::Error>> {
        let mut app = Self {
            running: true,
            selected: 0,
            windows: Vec::new(),
            pending_kill: false,
            window_starts: HashMap::new(),
            last_focused_id: None,
        };
        app.refresh_windows(driver)?;
        Ok(app)
    }

    /// Runs the main event loop, drawing the UI and handling input.
    pub fn run<T: Tmux>(
        &mut self,
        mut terminal: DefaultTerminal,
        driver: &T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let theme = Theme::default();
        let tick_rate = Duration::from_secs(REFRESH_INTERVAL_SECS);
        let mut last_draw = Instant::now() - tick_rate;
        while self.running {
            let should_redraw = last_draw.elapsed() >= tick_rate;
            if should_redraw {
                self.refresh_windows(driver)?;
                last_draw = Instant::now();
            }
            terminal.draw(|frame| ui::draw(frame, self, &theme))?;
            if event::poll(Duration::from_millis(100))?
                && let event::Event::Key(key) = event::read()?
                && key.kind == event::KeyEventKind::Press
            {
                self.handle_action(key_to_action(key), driver);
                last_draw = Instant::now();
            }
        }
        Ok(())
    }

    /// Dispatches a user action to the appropriate handler.
    pub fn handle_action<T: Tmux>(&mut self, action: Action, driver: &T) {
        match action {
            Action::KillWindow => {
                if self.pending_kill {
                    self.kill_window(driver);
                    self.pending_kill = false;
                } else {
                    self.pending_kill = true;
                }
            }
            Action::None => {}
            _ => {
                self.pending_kill = false;
                match action {
                    Action::Quit => self.quit(),
                    Action::NavigateUp => self.navigate_up(),
                    Action::NavigateDown => self.navigate_down(),
                    Action::FocusWindow => self.focus_window(driver),
                    Action::CreateWindow => self.create_window(driver),
                    _ => {}
                }
            }
        }
    }

    /// Signals the application to stop running.
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Moves the selection up by one window.
    pub fn navigate_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if let Some(window) = self.windows.get(self.selected) {
                self.last_focused_id = Some(window.id);
            }
        }
    }

    /// Moves the selection down by one window.
    pub fn navigate_down(&mut self) {
        if self.selected + 1 < self.windows.len() {
            self.selected += 1;
            if let Some(window) = self.windows.get(self.selected) {
                self.last_focused_id = Some(window.id);
            }
        }
    }

    /// Focuses the currently selected tmux window.
    pub fn focus_window<T: Tmux>(&self, driver: &T) {
        if let Some(window) = self.windows.get(self.selected) {
            let _ = driver.select_window(window.id);
        }
    }

    /// Creates a new tmux window and adds it to the list.
    pub fn create_window<T: Tmux>(&mut self, driver: &T) {
        let name = format!("agent-{}", self.windows.len() + 1);
        if let Ok(new_window) = driver.create_window(&name) {
            let _ = self.refresh_windows(driver);
            if let Some(index) = self.windows.iter().position(|w| w.id == new_window.id) {
                self.selected = index;
            }
        }
    }

    /// Kills the currently selected tmux window.
    pub fn kill_window<T: Tmux>(&mut self, driver: &T) {
        if let Some(window) = self.windows.get(self.selected) {
            let _ = driver.kill_window(window.id);
            let _ = self.refresh_windows(driver);
        }
    }

    /// Reloads the window list from the tmux driver and tracks start times.
    pub fn refresh_windows<T: Tmux>(
        &mut self,
        driver: &T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let windows = driver.list_windows()?;
        let now = Instant::now();

        let current_ids: std::collections::HashSet<u32> = windows.iter().map(|w| w.id).collect();

        for window in &windows {
            self.window_starts.entry(window.id).or_insert(now);
        }

        self.window_starts.retain(|id, _| current_ids.contains(id));

        let mut enriched_windows: Vec<Window> = windows;
        for window in &mut enriched_windows {
            window.started_at = self.window_starts.get(&window.id).copied();
        }

        self.windows = enriched_windows;

        if let Some(active_window) = self.windows.iter().find(|w| w.is_active) {
            if self.last_focused_id != Some(active_window.id)
                && let Some(index) = self.windows.iter().position(|w| w.id == active_window.id)
            {
                self.selected = index;
                self.last_focused_id = Some(active_window.id);
            }
        } else if self.selected >= self.windows.len() && self.selected > 0 {
            self.selected -= 1;
        }

        Ok(())
    }

    /// Returns the index of the currently selected window.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Returns a slice of the current window list.
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// Returns whether a kill action is pending confirmation.
    pub fn pending_kill(&self) -> bool {
        self.pending_kill
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::{Tmux, TmuxError, Window};
    use std::time::{Duration, Instant};

    struct MockTmux {
        windows: std::cell::RefCell<Vec<Window>>,
        next_id: std::cell::RefCell<u32>,
    }

    impl MockTmux {
        fn new() -> Self {
            Self {
                windows: std::cell::RefCell::new(vec![
                    Window {
                        id: 1,
                        name: "agent-1".to_string(),
                        running_command: "cargo build".to_string(),
                        started_at: Some(Instant::now() - Duration::from_secs(125)),
                        notification_pending: false,
                        is_active: false,
                    },
                    Window {
                        id: 2,
                        name: "agent-2".to_string(),
                        running_command: "npm test".to_string(),
                        started_at: Some(Instant::now() - Duration::from_secs(45)),
                        notification_pending: true,
                        is_active: false,
                    },
                    Window {
                        id: 3,
                        name: "agent-3".to_string(),
                        running_command: "python main.py".to_string(),
                        started_at: Some(Instant::now() - Duration::from_secs(300)),
                        notification_pending: false,
                        is_active: false,
                    },
                ]),
                next_id: std::cell::RefCell::new(4),
            }
        }
    }

    impl Tmux for MockTmux {
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
                id: *next_id,
                name: name.to_string(),
                running_command: String::new(),
                started_at: None,
                notification_pending: false,
                is_active: false,
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

        fn split_window(&self, _command: &str) -> Result<String, TmuxError> {
            Ok("%99".to_string())
        }
    }

    #[test]
    fn test_new() {
        let driver = MockTmux::new();
        let app = App::new(&driver).unwrap();
        assert!(app.running);
        assert_eq!(app.selected(), 0);
        assert_eq!(app.windows().len(), 3);
    }

    #[test]
    fn test_quit() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.quit();
        assert!(!app.running);
    }

    #[test]
    fn test_navigate_down() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        assert_eq!(app.selected(), 0);
        app.navigate_down();
        assert_eq!(app.selected(), 1);
        app.navigate_down();
        assert_eq!(app.selected(), 2);
        app.navigate_down();
        assert_eq!(app.selected(), 2);
    }

    #[test]
    fn test_navigate_up() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.navigate_down();
        app.navigate_down();
        assert_eq!(app.selected(), 2);
        app.navigate_up();
        assert_eq!(app.selected(), 1);
        app.navigate_up();
        assert_eq!(app.selected(), 0);
        app.navigate_up();
        assert_eq!(app.selected(), 0);
    }

    #[test]
    fn test_focus_window() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.navigate_down();
        app.focus_window(&driver);
    }

    #[test]
    fn test_create_window() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        let initial_len = app.windows().len();
        app.create_window(&driver);
        assert_eq!(app.windows().len(), initial_len + 1);
    }

    #[test]
    fn test_create_window_selects_new_window() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.navigate_down();
        assert_eq!(app.selected(), 1);

        let initial_len = app.windows().len();
        app.create_window(&driver);

        assert_eq!(app.windows().len(), initial_len + 1);
        assert_eq!(app.selected(), initial_len);
    }

    #[test]
    fn test_kill_window() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        let initial_len = app.windows().len();
        app.kill_window(&driver);
        assert_eq!(app.windows().len(), initial_len - 1);
    }

    #[test]
    fn test_kill_last_window_adjusts_selection() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.navigate_down();
        app.navigate_down();
        assert_eq!(app.selected(), 2);
        app.kill_window(&driver);
        assert_eq!(app.selected(), 1);
    }

    #[test]
    fn test_handle_action_quit() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.handle_action(Action::Quit, &driver);
        assert!(!app.running);
    }

    #[test]
    fn test_handle_action_navigate() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.handle_action(Action::NavigateDown, &driver);
        assert_eq!(app.selected(), 1);
        app.handle_action(Action::NavigateUp, &driver);
        assert_eq!(app.selected(), 0);
    }

    #[test]
    fn test_kill_window_requires_double_press() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        let initial_len = app.windows().len();
        assert!(!app.pending_kill());
        app.handle_action(Action::KillWindow, &driver);
        assert!(app.pending_kill());
        assert_eq!(app.windows().len(), initial_len);
        app.handle_action(Action::KillWindow, &driver);
        assert!(!app.pending_kill());
        assert_eq!(app.windows().len(), initial_len - 1);
    }

    #[test]
    fn test_other_action_cancels_pending_kill() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.handle_action(Action::KillWindow, &driver);
        assert!(app.pending_kill());
        app.handle_action(Action::NavigateDown, &driver);
        assert!(!app.pending_kill());
    }

    #[test]
    fn test_refresh_windows_new_windows_get_start_time() {
        let driver = MockTmux::new();
        let app = App::new(&driver).unwrap();
        assert!(app.windows().iter().all(|w| w.started_at.is_some()));
    }

    #[test]
    fn test_refresh_windows_existing_windows_keep_time() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        let first_times: Vec<Option<Instant>> =
            app.windows().iter().map(|w| w.started_at).collect();
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.refresh_windows(&driver).unwrap();
        let second_times: Vec<Option<Instant>> =
            app.windows().iter().map(|w| w.started_at).collect();
        assert_eq!(first_times, second_times);
    }

    #[test]
    fn test_refresh_windows_removed_windows_cleaned_up() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        assert_eq!(app.window_starts.len(), 3);
        let window_id = app.windows()[0].id;
        driver.kill_window(window_id).unwrap();
        app.refresh_windows(&driver).unwrap();
        assert_eq!(app.window_starts.len(), 2);
        assert!(!app.window_starts.contains_key(&window_id));
    }

    #[test]
    fn test_external_tmux_change_syncs_selection() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        assert_eq!(app.selected(), 0);

        driver.windows.borrow_mut()[1].is_active = true;
        app.refresh_windows(&driver).unwrap();
        assert_eq!(app.selected(), 1);
    }

    #[test]
    fn test_external_tmux_change_syncs_after_navigation() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        assert_eq!(app.selected(), 0);

        app.navigate_down();
        assert_eq!(app.selected(), 1);
        assert_eq!(app.last_focused_id, Some(2));

        driver.windows.borrow_mut()[0].is_active = true;
        app.refresh_windows(&driver).unwrap();
        assert_eq!(app.selected(), 0);
        assert_eq!(app.last_focused_id, Some(1));
    }
}

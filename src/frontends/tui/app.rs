use std::time::{Duration, Instant};

use crossterm::event;
use ratatui::DefaultTerminal;

use crate::backends::{Tmux, Window};
use crate::frontends::tui::event::{Action, key_to_action};
use crate::frontends::tui::theme::Theme;
use crate::frontends::tui::ui;

/// Main application state for the TUI frontend.
pub struct App {
    running: bool,
    selected: usize,
    windows: Vec<Window>,
}

impl App {
    /// Creates a new App, loading windows from the tmux driver.
    pub fn new<T: Tmux>(driver: &T) -> Result<Self, Box<dyn std::error::Error>> {
        let windows = driver.list_windows()?;
        Ok(Self {
            running: true,
            selected: 0,
            windows,
        })
    }

    /// Runs the main event loop, drawing the UI and handling input.
    pub fn run<T: Tmux>(
        &mut self,
        mut terminal: DefaultTerminal,
        driver: &T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let theme = Theme::default();
        let tick_rate = Duration::from_secs(60);
        let mut last_draw = Instant::now() - tick_rate;
        while self.running {
            let should_redraw = last_draw.elapsed() >= tick_rate;
            if should_redraw {
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
            Action::Quit => self.quit(),
            Action::NavigateUp => self.navigate_up(),
            Action::NavigateDown => self.navigate_down(),
            Action::FocusWindow => self.focus_window(driver),
            Action::CreateWindow => self.create_window(driver),
            Action::KillWindow => self.kill_window(driver),
            Action::None => {}
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
        }
    }

    /// Moves the selection down by one window.
    pub fn navigate_down(&mut self) {
        if self.selected + 1 < self.windows.len() {
            self.selected += 1;
        }
    }

    /// Focuses the currently selected tmux window.
    pub fn focus_window<T: Tmux>(&self, driver: &T) {
        if let Some(window) = self.windows.get(self.selected) {
            let _ = driver.select_window(&window.name);
        }
    }

    /// Creates a new tmux window and adds it to the list.
    pub fn create_window<T: Tmux>(&mut self, driver: &T) {
        let name = format!("agent-{}", self.windows.len() + 1);
        if let Ok(window) = driver.create_window(&name) {
            self.windows.push(window);
        }
    }

    /// Kills the currently selected tmux window.
    pub fn kill_window<T: Tmux>(&mut self, driver: &T) {
        if let Some(window) = self.windows.get(self.selected) {
            let _ = driver.kill_window(&window.name);
            self.windows.remove(self.selected);
            if self.selected >= self.windows.len() && self.selected > 0 {
                self.selected -= 1;
            }
        }
    }

    /// Reloads the window list from the tmux driver.
    #[allow(dead_code)]
    pub fn refresh<T: Tmux>(&mut self, driver: &T) {
        if let Ok(windows) = driver.list_windows() {
            self.windows = windows;
            if self.selected >= self.windows.len() && self.selected > 0 {
                self.selected -= 1;
            }
        }
    }

    /// Returns whether the application is still running.
    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Returns the index of the currently selected window.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Returns a slice of the current window list.
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::TmuxDriver;

    #[test]
    fn test_new() {
        let driver = TmuxDriver;
        let app = App::new(&driver).unwrap();
        assert!(app.is_running());
        assert_eq!(app.selected(), 0);
        assert_eq!(app.windows().len(), 3);
    }

    #[test]
    fn test_quit() {
        let driver = TmuxDriver;
        let mut app = App::new(&driver).unwrap();
        app.quit();
        assert!(!app.is_running());
    }

    #[test]
    fn test_navigate_down() {
        let driver = TmuxDriver;
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
        let driver = TmuxDriver;
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
        let driver = TmuxDriver;
        let mut app = App::new(&driver).unwrap();
        app.navigate_down();
        app.focus_window(&driver);
    }

    #[test]
    fn test_create_window() {
        let driver = TmuxDriver;
        let mut app = App::new(&driver).unwrap();
        let initial_len = app.windows().len();
        app.create_window(&driver);
        assert_eq!(app.windows().len(), initial_len + 1);
    }

    #[test]
    fn test_kill_window() {
        let driver = TmuxDriver;
        let mut app = App::new(&driver).unwrap();
        let initial_len = app.windows().len();
        app.kill_window(&driver);
        assert_eq!(app.windows().len(), initial_len - 1);
    }

    #[test]
    fn test_kill_last_window_adjusts_selection() {
        let driver = TmuxDriver;
        let mut app = App::new(&driver).unwrap();
        app.navigate_down();
        app.navigate_down();
        assert_eq!(app.selected(), 2);
        app.kill_window(&driver);
        assert_eq!(app.selected(), 1);
    }

    #[test]
    fn test_handle_action_quit() {
        let driver = TmuxDriver;
        let mut app = App::new(&driver).unwrap();
        app.handle_action(Action::Quit, &driver);
        assert!(!app.is_running());
    }

    #[test]
    fn test_handle_action_navigate() {
        let driver = TmuxDriver;
        let mut app = App::new(&driver).unwrap();
        app.handle_action(Action::NavigateDown, &driver);
        assert_eq!(app.selected(), 1);
        app.handle_action(Action::NavigateUp, &driver);
        assert_eq!(app.selected(), 0);
    }
}

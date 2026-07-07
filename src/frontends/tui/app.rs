use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossterm::event;
use ratatui::DefaultTerminal;
use ratatui::widgets::ListState;

use crate::backends::agents::is_agent;
use crate::backends::tmux::{Tmux, Window};
use crate::frontends::tui::event::{Action, PendingAction, Tab, key_to_action};
use crate::frontends::tui::theme::Theme;
use crate::frontends::tui::ui;

const REFRESH_INTERVAL_SECS: u64 = 5;

/// Main application state for the TUI frontend.
pub struct App {
    running: bool,
    active_tab: Tab,
    agents_selected: usize,
    windows_selected: usize,
    windows: Vec<Window>,
    pending_action: Option<PendingAction>,
    window_starts: HashMap<u32, Instant>,
    last_focused_id: Option<u32>,
    list_state: ListState,
}

impl App {
    /// Creates a new App, loading windows from the tmux driver.
    pub fn new<T: Tmux>(driver: &T) -> anyhow::Result<Self> {
        let mut app = Self {
            running: true,
            active_tab: Tab::Windows,
            agents_selected: 0,
            windows_selected: 0,
            windows: Vec::new(),
            pending_action: None,
            window_starts: HashMap::new(),
            last_focused_id: None,
            list_state: ListState::default(),
        };
        app.refresh_windows(driver)?;
        if !app.is_tab_empty(Tab::Agents) {
            app.active_tab = Tab::Agents;
        }
        Ok(app)
    }

    /// Runs the main event loop, drawing the UI and handling input.
    pub fn run<T: Tmux>(
        &mut self,
        mut terminal: DefaultTerminal,
        driver: &T,
    ) -> anyhow::Result<()> {
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
            Action::KillWindow if self.pending_action == Some(PendingAction::KillWindow) => {
                self.kill_window(driver);
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
                    Action::FocusWindow => self.focus_window(driver),
                    Action::CreateWindow => self.create_window(driver),
                    Action::SwitchTabLeft => self.switch_tab(self.active_tab.left()),
                    Action::SwitchTabRight => self.switch_tab(self.active_tab.right()),
                    _ => {}
                }
            }
        }
    }

    /// Signals the application to stop running.
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Moves the selection up by one window within the current tab.
    pub fn navigate_up(&mut self) {
        let selected = self.current_selected();
        if selected > 0 {
            self.set_current_selected(selected - 1);
            if let Some(window) = self.current_tab_window() {
                self.last_focused_id = Some(window.id);
            }
        }
    }

    /// Moves the selection down by one window within the current tab.
    pub fn navigate_down(&mut self) {
        let selected = self.current_selected();
        let len = self.current_tab_indices().len();
        if selected + 1 < len {
            self.set_current_selected(selected + 1);
            if let Some(window) = self.current_tab_window() {
                self.last_focused_id = Some(window.id);
            }
        }
    }

    /// Focuses the currently selected tmux window.
    pub fn focus_window<T: Tmux>(&self, driver: &T) {
        if let Some(window) = self.current_tab_window() {
            let _ = driver.select_window(window.id);
        }
    }

    /// Creates a new tmux window and adds it to the list.
    pub fn create_window<T: Tmux>(&mut self, driver: &T) {
        self.active_tab = Tab::Windows;
        let name = format!("agent-{}", self.windows.len() + 1);
        if let Ok(new_window) = driver.create_window(&name) {
            let _ = self.refresh_windows(driver);
            let indices = self.current_tab_indices();
            if let Some(pos) = indices
                .iter()
                .position(|&i| self.windows[i].id == new_window.id)
            {
                self.windows_selected = pos;
                self.list_state.select(Some(self.windows_selected));
            }
        }
    }

    /// Kills the currently selected tmux window.
    pub fn kill_window<T: Tmux>(&mut self, driver: &T) {
        if let Some(window) = self.current_tab_window() {
            let _ = driver.kill_window(window.id);
            let _ = self.refresh_windows(driver);
        }
    }

    /// Switches to the given tab if it is not empty.
    pub fn switch_tab(&mut self, tab: Tab) {
        if !self.is_tab_empty(tab) {
            self.active_tab = tab;
            self.set_selected_for_tab(tab, 0);
            self.list_state.select(Some(0));
        }
    }

    /// Reloads the window list from the tmux driver and tracks start times.
    pub fn refresh_windows<T: Tmux>(&mut self, driver: &T) -> anyhow::Result<()> {
        let windows = driver.list_windows()?;
        let now = Instant::now();

        let selected_window_id = self.current_tab_window().map(|w| w.id);

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

        if let Some(selected_id) = selected_window_id
            && let Some(window) = self.windows.iter().find(|w| w.id == selected_id)
        {
            let new_tab = if is_agent(&window.running_command).is_some() {
                Tab::Agents
            } else {
                Tab::Windows
            };

            if new_tab != self.active_tab {
                let indices = self.indices_for_tab(new_tab);
                if let Some(pos) = indices
                    .iter()
                    .position(|&i| self.windows[i].id == selected_id)
                {
                    self.active_tab = new_tab;
                    self.set_selected_for_tab(new_tab, pos);
                    self.list_state.select(Some(self.current_selected()));
                }
            }
        }

        let active_window_info = self
            .windows
            .iter()
            .find(|w| w.is_active)
            .map(|w| (w.id, w.running_command.clone()));

        if let Some((active_id, active_command)) = active_window_info {
            if self.last_focused_id != Some(active_id) {
                let target_tab = if is_agent(&active_command).is_some() {
                    Tab::Agents
                } else {
                    Tab::Windows
                };

                let indices = self.indices_for_tab(target_tab);
                if let Some(pos) = indices
                    .iter()
                    .position(|&i| self.windows[i].id == active_id)
                {
                    self.active_tab = target_tab;
                    self.set_selected_for_tab(target_tab, pos);
                    self.last_focused_id = Some(active_id);
                    self.list_state.select(Some(self.current_selected()));
                }
            }
        } else {
            self.clamp_selections();
            self.list_state.select(Some(self.current_selected()));
        }

        Ok(())
    }

    /// Returns the currently active tab.
    pub fn active_tab(&self) -> Tab {
        self.active_tab
    }

    /// Returns the selection index within the current tab.
    pub fn current_selected(&self) -> usize {
        match self.active_tab {
            Tab::Agents => self.agents_selected,
            Tab::Windows => self.windows_selected,
        }
    }

    /// Returns the window at the current selection, or None if the tab is empty.
    pub fn current_tab_window(&self) -> Option<&Window> {
        let indices = self.current_tab_indices();
        let selected = self.current_selected();
        indices.get(selected).map(|&i| &self.windows[i])
    }

    /// Returns the filtered windows for the current tab.
    pub fn current_tab_windows(&self) -> Vec<&Window> {
        self.current_tab_indices()
            .iter()
            .map(|&i| &self.windows[i])
            .collect()
    }

    /// Returns the total number of windows in the current tab.
    pub fn current_tab_len(&self) -> usize {
        self.current_tab_indices().len()
    }

    /// Returns whether a tab has no windows.
    pub fn is_tab_empty(&self, tab: Tab) -> bool {
        self.indices_for_tab(tab).is_empty()
    }

    /// Returns a slice of all windows.
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// Returns the pending action awaiting confirmation, if any.
    pub fn pending_action(&self) -> Option<PendingAction> {
        self.pending_action
    }

    /// Returns a reference to the list state.
    pub fn list_state(&self) -> &ListState {
        &self.list_state
    }

    /// Adjusts the scroll offset to ensure the selected window is visible.
    pub fn ensure_visible(&mut self, visible_count: usize) {
        if visible_count == 0 {
            return;
        }

        let selected = self.current_selected();
        let current_offset = self.list_state.offset();
        let new_offset = if selected < current_offset {
            selected
        } else if selected >= current_offset + visible_count {
            selected - visible_count + 1
        } else {
            current_offset
        };

        *self.list_state.offset_mut() = new_offset;
    }

    fn current_tab_indices(&self) -> Vec<usize> {
        self.indices_for_tab(self.active_tab)
    }

    fn indices_for_tab(&self, tab: Tab) -> Vec<usize> {
        self.windows
            .iter()
            .enumerate()
            .filter(|(_, w)| match tab {
                Tab::Agents => is_agent(&w.running_command).is_some(),
                Tab::Windows => is_agent(&w.running_command).is_none(),
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn set_current_selected(&mut self, idx: usize) {
        match self.active_tab {
            Tab::Agents => self.agents_selected = idx,
            Tab::Windows => self.windows_selected = idx,
        }
        self.list_state.select(Some(idx));
    }

    fn set_selected_for_tab(&mut self, tab: Tab, idx: usize) {
        match tab {
            Tab::Agents => self.agents_selected = idx,
            Tab::Windows => self.windows_selected = idx,
        }
    }

    fn clamp_selections(&mut self) {
        let agents_len = self.indices_for_tab(Tab::Agents).len();
        if self.agents_selected >= agents_len && agents_len > 0 {
            self.agents_selected = agents_len - 1;
        } else if agents_len == 0 {
            self.agents_selected = 0;
        }

        let windows_len = self.indices_for_tab(Tab::Windows).len();
        if self.windows_selected >= windows_len && windows_len > 0 {
            self.windows_selected = windows_len - 1;
        } else if windows_len == 0 {
            self.windows_selected = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::{Tmux, TmuxError, Window};
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
                        current_dir: "/home/user/project1".to_string(),
                    },
                    Window {
                        id: 2,
                        name: "agent-2".to_string(),
                        running_command: "claude".to_string(),
                        started_at: Some(Instant::now() - Duration::from_secs(45)),
                        notification_pending: true,
                        is_active: false,
                        current_dir: "/home/user/project2".to_string(),
                    },
                    Window {
                        id: 3,
                        name: "agent-3".to_string(),
                        running_command: "python main.py".to_string(),
                        started_at: Some(Instant::now() - Duration::from_secs(300)),
                        notification_pending: false,
                        is_active: false,
                        current_dir: "/home/user/project3".to_string(),
                    },
                    Window {
                        id: 4,
                        name: "agent-4".to_string(),
                        running_command: "opencode".to_string(),
                        started_at: Some(Instant::now() - Duration::from_secs(10)),
                        notification_pending: false,
                        is_active: false,
                        current_dir: "/home/user/project4".to_string(),
                    },
                ]),
                next_id: std::cell::RefCell::new(5),
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
                current_dir: "/home/user".to_string(),
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
        assert_eq!(app.active_tab(), Tab::Agents);
        assert_eq!(app.windows().len(), 4);
    }

    #[test]
    fn test_new_defaults_to_windows_when_no_agents() {
        let driver = MockTmux::new();
        driver.windows.borrow_mut()[1].running_command = "bash".to_string();
        driver.windows.borrow_mut()[3].running_command = "zsh".to_string();
        let app = App::new(&driver).unwrap();
        assert_eq!(app.active_tab(), Tab::Windows);
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
        assert_eq!(app.active_tab(), Tab::Agents);
        assert_eq!(app.current_tab_len(), 2);
        assert_eq!(app.current_selected(), 0);
        app.navigate_down();
        assert_eq!(app.current_selected(), 1);
        app.navigate_down();
        assert_eq!(app.current_selected(), 1);
    }

    #[test]
    fn test_navigate_up() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.navigate_down();
        assert_eq!(app.current_selected(), 1);
        app.navigate_up();
        assert_eq!(app.current_selected(), 0);
        app.navigate_up();
        assert_eq!(app.current_selected(), 0);
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
    fn test_create_window_switches_to_windows_tab() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        assert_eq!(app.active_tab(), Tab::Agents);
        app.create_window(&driver);
        assert_eq!(app.active_tab(), Tab::Windows);
    }

    #[test]
    fn test_create_window_selects_new_window() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.create_window(&driver);
        assert_eq!(app.active_tab(), Tab::Windows);
        let windows_tab_len = app.current_tab_len();
        assert_eq!(app.current_selected(), windows_tab_len - 1);
    }

    #[test]
    fn test_kill_window() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        let initial_agents = app.current_tab_len();
        app.kill_window(&driver);
        assert_eq!(app.current_tab_len(), initial_agents - 1);
    }

    #[test]
    fn test_kill_last_window_adjusts_selection() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.navigate_down();
        assert_eq!(app.current_selected(), 1);
        app.kill_window(&driver);
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_handle_action_quit() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.handle_action(Action::Quit, &driver);
        assert!(app.running);
        app.handle_action(Action::Quit, &driver);
        assert!(!app.running);
    }

    #[test]
    fn test_handle_action_navigate() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.handle_action(Action::NavigateDown, &driver);
        assert_eq!(app.current_selected(), 1);
        app.handle_action(Action::NavigateUp, &driver);
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_kill_window_requires_double_press() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        let initial_agents = app.current_tab_len();
        assert_eq!(app.pending_action(), None);
        app.handle_action(Action::KillWindow, &driver);
        assert_eq!(app.pending_action(), Some(PendingAction::KillWindow));
        assert_eq!(app.current_tab_len(), initial_agents);
        app.handle_action(Action::KillWindow, &driver);
        assert_eq!(app.pending_action(), None);
        assert_eq!(app.current_tab_len(), initial_agents - 1);
    }

    #[test]
    fn test_quit_requires_double_press() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        assert!(app.running);
        assert_eq!(app.pending_action(), None);
        app.handle_action(Action::Quit, &driver);
        assert_eq!(app.pending_action(), Some(PendingAction::Quit));
        assert!(app.running);
        app.handle_action(Action::Quit, &driver);
        assert_eq!(app.pending_action(), None);
        assert!(!app.running);
    }

    #[test]
    fn test_other_action_cancels_pending_kill() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.handle_action(Action::KillWindow, &driver);
        assert_eq!(app.pending_action(), Some(PendingAction::KillWindow));
        app.handle_action(Action::NavigateDown, &driver);
        assert_eq!(app.pending_action(), None);
    }

    #[test]
    fn test_other_action_cancels_pending_quit() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.handle_action(Action::Quit, &driver);
        assert_eq!(app.pending_action(), Some(PendingAction::Quit));
        app.handle_action(Action::NavigateDown, &driver);
        assert_eq!(app.pending_action(), None);
        assert!(app.running);
    }

    #[test]
    fn test_none_action_clears_pending() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.handle_action(Action::KillWindow, &driver);
        assert_eq!(app.pending_action(), Some(PendingAction::KillWindow));
        app.handle_action(Action::None, &driver);
        assert_eq!(app.pending_action(), None);

        app.handle_action(Action::Quit, &driver);
        assert_eq!(app.pending_action(), Some(PendingAction::Quit));
        app.handle_action(Action::None, &driver);
        assert_eq!(app.pending_action(), None);
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
        assert_eq!(app.window_starts.len(), 4);
        let window_id = app.windows()[0].id;
        driver.kill_window(window_id).unwrap();
        app.refresh_windows(&driver).unwrap();
        assert_eq!(app.window_starts.len(), 3);
        assert!(!app.window_starts.contains_key(&window_id));
    }

    #[test]
    fn test_external_tmux_change_syncs_selection() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        assert_eq!(app.current_selected(), 0);

        driver.windows.borrow_mut()[3].is_active = true;
        app.refresh_windows(&driver).unwrap();
        assert_eq!(app.current_selected(), 1);
    }

    #[test]
    fn test_external_tmux_change_switches_tab() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        assert_eq!(app.active_tab(), Tab::Agents);

        driver.windows.borrow_mut()[0].is_active = true;
        app.refresh_windows(&driver).unwrap();
        assert_eq!(app.active_tab(), Tab::Windows);
    }

    #[test]
    fn test_external_tmux_change_syncs_after_navigation() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        assert_eq!(app.current_selected(), 0);

        app.navigate_down();
        assert_eq!(app.current_selected(), 1);
        assert_eq!(app.last_focused_id, Some(4));

        driver.windows.borrow_mut()[0].is_active = true;
        app.refresh_windows(&driver).unwrap();
        assert_eq!(app.active_tab(), Tab::Windows);
        assert_eq!(app.current_selected(), 0);
        assert_eq!(app.last_focused_id, Some(1));
    }

    #[test]
    fn test_selected_window_moving_to_other_tab_switches_tab() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        assert_eq!(app.active_tab(), Tab::Agents);
        assert_eq!(app.current_tab_len(), 2);

        let selected_id = app.current_tab_window().unwrap().id;
        assert_eq!(selected_id, 2);

        driver.windows.borrow_mut()[1].running_command = "bash".to_string();
        app.refresh_windows(&driver).unwrap();

        assert_eq!(app.active_tab(), Tab::Windows);
        assert_eq!(app.current_tab_window().unwrap().id, selected_id);
    }

    #[test]
    fn test_switch_tab() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        assert_eq!(app.active_tab(), Tab::Agents);
        app.switch_tab(Tab::Windows);
        assert_eq!(app.active_tab(), Tab::Windows);
        app.switch_tab(Tab::Agents);
        assert_eq!(app.active_tab(), Tab::Agents);
    }

    #[test]
    fn test_switch_tab_blocked_when_empty() {
        let driver = MockTmux::new();
        driver.windows.borrow_mut()[1].running_command = "bash".to_string();
        driver.windows.borrow_mut()[3].running_command = "zsh".to_string();
        let mut app = App::new(&driver).unwrap();
        assert_eq!(app.active_tab(), Tab::Windows);
        assert!(app.is_tab_empty(Tab::Agents));
        app.switch_tab(Tab::Agents);
        assert_eq!(app.active_tab(), Tab::Windows);
    }

    #[test]
    fn test_switch_tab_resets_selection_to_first() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();

        app.navigate_down();
        assert_eq!(app.current_selected(), 1);

        app.switch_tab(Tab::Windows);
        assert_eq!(app.current_selected(), 0);

        app.navigate_down();
        assert_eq!(app.current_selected(), 1);

        app.switch_tab(Tab::Agents);
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_is_tab_empty() {
        let driver = MockTmux::new();
        let app = App::new(&driver).unwrap();
        assert!(!app.is_tab_empty(Tab::Agents));
        assert!(!app.is_tab_empty(Tab::Windows));
    }

    #[test]
    fn test_current_tab_windows() {
        let driver = MockTmux::new();
        let app = App::new(&driver).unwrap();
        let agent_windows = app.current_tab_windows();
        assert_eq!(agent_windows.len(), 2);
        assert!(is_agent(&agent_windows[0].running_command).is_some());
        assert!(is_agent(&agent_windows[1].running_command).is_some());
    }

    #[test]
    fn test_ensure_visible_no_change_when_already_visible() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        assert_eq!(app.list_state().offset(), 0);

        app.ensure_visible(3);
        assert_eq!(app.list_state().offset(), 0);
    }

    #[test]
    fn test_ensure_visible_scrolls_down_when_selected_below_visible() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.navigate_down();
        assert_eq!(app.current_selected(), 1);

        app.ensure_visible(1);
        assert_eq!(app.list_state().offset(), 1);
    }

    #[test]
    fn test_ensure_visible_scrolls_up_when_selected_above_visible() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.navigate_down();
        *app.list_state.offset_mut() = 1;

        app.navigate_up();
        assert_eq!(app.current_selected(), 0);

        app.ensure_visible(1);
        assert_eq!(app.list_state().offset(), 0);
    }

    #[test]
    fn test_ensure_visible_no_scroll_when_zero_visible_count() {
        let driver = MockTmux::new();
        let mut app = App::new(&driver).unwrap();
        app.navigate_down();

        app.ensure_visible(0);
        assert_eq!(app.list_state().offset(), 0);
    }
}

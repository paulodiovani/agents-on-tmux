use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event;
use crossterm::execute;
use ratatui::DefaultTerminal;
use ratatui::widgets::ListState;

use crate::backends::agents::is_agent;
use crate::backends::control_mode::{self, TmuxEvent};
use crate::backends::logger;
use crate::backends::tmux::{Tmux, Window};
use crate::frontends::tui::event::{Action, PendingAction, Tab, key_to_action};
use crate::frontends::tui::theme::Theme;
use crate::frontends::tui::ui;

/// The nested-session commands worth advertising, with their footer labels.
const HINTED_COMMANDS: [(&str, &str); 5] = [
    ("new-window", "new"),
    ("next-window", "next"),
    ("previous-window", "prev"),
    ("last-window", "last"),
    ("rename-window", "rename"),
];

/// Resolves the key sequences that reach the nested session from the user's
/// own tmux bindings: one entry for the prefix sequence (e.g. "C-b C-b",
/// label "prefix"), then the bare keys for the hinted commands (e.g. "c",
/// "new"). Empty when anything cannot be resolved, so the footer falls back
/// to the TUI's own keybindings.
fn resolve_tmux_hints(driver: &dyn Tmux) -> Vec<(String, String)> {
    let Ok(keys) = driver.list_keys("prefix") else {
        return Vec::new();
    };
    let prefix = driver.show_options("prefix").unwrap_or_default();
    if prefix.is_empty() {
        return Vec::new();
    }

    let send_prefix = keys
        .iter()
        .find(|b| b.command.split_whitespace().next() == Some("send-prefix"))
        .map(|b| b.key.clone())
        .unwrap_or_else(|| prefix.clone());

    let prefix_seq = format!("{prefix} {send_prefix}");

    let binding_for = |command: &str| {
        keys.iter()
            .find(|b| {
                let first_word = b.command.split_whitespace().next();
                if first_word == Some(command) {
                    return true;
                }
                if command == "rename-window"
                    && first_word == Some("command-prompt")
                    && b.command.contains("rename-window")
                {
                    return true;
                }
                false
            })
            .map(|b| b.key.clone())
    };

    let mut hints = vec![(prefix_seq, "prefix".to_string())];
    hints.extend(HINTED_COMMANDS.iter().filter_map(|(command, label)| {
        let key = binding_for(command)?;
        Some((key, (*label).to_string()))
    }));
    hints
}

/// Main application state for the TUI frontend.
pub struct App {
    active_tab: Tab,
    agents_selected: usize,
    event_rx: Option<mpsc::Receiver<TmuxEvent>>,
    last_focused_id: Option<u32>,
    list_state: ListState,
    nested_driver: Box<dyn Tmux>,
    pane_active: bool,
    panel: Option<(String, u16)>,
    parent_driver: Box<dyn Tmux>,
    pending_action: Option<PendingAction>,
    running: bool,
    tmux_hints: Vec<(String, String)>,
    window_starts: HashMap<u32, Instant>,
    windows: Vec<Window>,
    windows_selected: usize,
}

impl App {
    /// Creates a new App, loading windows from the tmux driver. `panel` is
    /// `Some((pane_id, width))` only when running as the split side panel: the
    /// width to re-assert on the pane whenever tmux rescales the layout.
    /// `pane_id` is the pane the TUI runs in, when known; it enables focus
    /// tracking.
    pub fn new(
        nested_driver: Box<dyn Tmux>,
        parent_driver: Box<dyn Tmux>,
        panel: Option<(String, u16)>,
        pane_id: Option<String>,
    ) -> anyhow::Result<Self> {
        // Bindings are read once: rebinds while aot runs are not picked up.
        let tmux_hints = if pane_id.is_some() {
            resolve_tmux_hints(parent_driver.as_ref())
        } else {
            Vec::new()
        };

        let mut app = Self {
            active_tab: Tab::Windows,
            agents_selected: 0,
            event_rx: None,
            last_focused_id: None,
            list_state: ListState::default(),
            nested_driver,
            pane_active: true,
            panel,
            parent_driver,
            pending_action: None,
            running: true,
            tmux_hints,
            window_starts: HashMap::new(),
            windows: Vec::new(),
            windows_selected: 0,
        };
        app.refresh_windows()?;
        if !app.is_tab_empty(Tab::Agents) {
            app.active_tab = Tab::Agents;
        }
        Ok(app)
    }

    /// Re-asserts the side panel width after tmux rescaled the layout. No-op
    /// outside panel mode or when the width already matches, which makes the
    /// enforcement converge without loops or redundant tmux calls. Failures
    /// (e.g. terminal narrower than the panel) are logged and ignored.
    fn enforce_panel_width(&self, current_width: u16) {
        if let Some((pane_id, target)) = &self.panel
            && current_width != *target
        {
            logger::debug(&format!(
                "app: enforce panel width {target} (was {current_width})"
            ));
            let _ = self.parent_driver.resize_pane(pane_id, *target);
        }
    }

    /// Runs the main event loop, drawing the UI and handling input.
    /// Data refresh is event-driven (tmux control mode); the ~1s redraw tick
    /// only repaints so the uptime counters keep ticking.
    pub fn run(&mut self, mut terminal: DefaultTerminal) -> anyhow::Result<()> {
        let theme = Theme::default();

        // Spawn control mode thread (only the session name crosses the thread boundary).
        let (event_tx, event_rx) = mpsc::channel();
        let session = self.nested_driver.session_name().to_string();
        logger::info(&format!("app: starting control mode: session={session}"));
        std::thread::spawn(move || {
            control_mode::control_mode_thread(session, event_tx);
        });
        self.event_rx = Some(event_rx);

        // The terminal may have been resized between the split and TUI
        // startup; re-assert the panel width before the first draw.
        let initial_width = terminal.size()?.width;
        self.enforce_panel_width(initial_width);

        // Ask the terminal/tmux to report pane focus changes. Without
        // focus-events on no events ever arrive, so pane_active stays true.
        let _ = execute!(std::io::stdout(), event::EnableFocusChange);

        let redraw_tick = Duration::from_secs(1);
        let mut last_draw = Instant::now() - redraw_tick;
        while self.running {
            if last_draw.elapsed() >= redraw_tick {
                terminal.draw(|frame| ui::draw(frame, self, &theme))?;
                last_draw = Instant::now();
            }

            // Poll terminal events (100ms). A keypress, resize, or focus
            // change forces an immediate redraw next iteration.
            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    event::Event::Key(key) if key.kind == event::KeyEventKind::Press => {
                        self.handle_action(key_to_action(key));
                        last_draw = Instant::now() - redraw_tick;
                    }
                    event::Event::Resize(width, _) => {
                        self.enforce_panel_width(width);
                        last_draw = Instant::now() - redraw_tick;
                    }
                    event::Event::FocusGained => {
                        self.set_pane_active(true);
                        last_draw = Instant::now() - redraw_tick;
                    }
                    event::Event::FocusLost => {
                        self.set_pane_active(false);
                        last_draw = Instant::now() - redraw_tick;
                    }
                    _ => {}
                }
            }

            // Drain tmux events (non-blocking). A refresh forces an immediate redraw.
            if self.process_tmux_events() {
                last_draw = Instant::now() - redraw_tick;
            }
        }

        let _ = execute!(std::io::stdout(), event::DisableFocusChange);
        Ok(())
    }

    /// Drains pending control-mode events, refreshing the window list at most
    /// once per drain (events like %output arrive in bursts) and quitting on Exit.
    /// Returns whether the window list was refreshed (i.e. a redraw is due).
    fn process_tmux_events(&mut self) -> bool {
        let mut needs_refresh = false;
        let mut should_exit = false;

        // Drain the channel; the borrow of event_rx ends before we touch &mut self.
        if let Some(rx) = &self.event_rx {
            while let Ok(event) = rx.try_recv() {
                match event {
                    TmuxEvent::Refresh => needs_refresh = true,
                    TmuxEvent::Exit => should_exit = true,
                }
            }
        }

        if needs_refresh {
            let _ = self.refresh_windows();
        }
        if should_exit {
            self.running = false;
        }
        needs_refresh
    }

    /// Dispatches a user action to the appropriate handler.
    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::KillWindow if self.pending_action == Some(PendingAction::KillWindow) => {
                self.kill_window();
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
                    Action::CreateWindow => self.create_window(),
                    Action::RenameWindow => self.rename_window(),
                    Action::SwitchTabLeft => self.switch_tab(self.active_tab.left()),
                    Action::SwitchTabRight => self.switch_tab(self.active_tab.right()),
                    _ => {}
                }
            }
        }
    }

    /// Signals the application to stop running.
    pub fn quit(&mut self) {
        logger::info("app: quit");
        self.running = false;
    }

    /// Updates the pane focus state from a terminal focus event.
    pub fn set_pane_active(&mut self, active: bool) {
        if self.pane_active != active {
            logger::debug(&format!("app: pane active {active}"));
            self.pane_active = active;
        }
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
    pub fn focus_window(&self) {
        if let Some(window) = self.current_tab_window() {
            logger::debug(&format!("app: focus window @{}", window.id));
            let _ = self.nested_driver.select_window(window.id);
            let _ = self.parent_driver.last_pane();
        }
    }

    /// Creates a new tmux window and adds it to the list.
    pub fn create_window(&mut self) {
        self.active_tab = Tab::Windows;
        let name = format!("agent-{}", self.windows.len() + 1);
        logger::debug(&format!("app: create window {name}"));
        if let Ok(new_window) = self.nested_driver.create_window(&name) {
            let _ = self.refresh_windows();
            let indices = self.current_tab_indices();
            if let Some(pos) = indices
                .iter()
                .position(|&i| self.windows[i].id == new_window.id)
            {
                self.windows_selected = pos;
                self.list_state.select(Some(self.windows_selected));
            }
            self.focus_window();
        }
    }

    /// Kills the currently selected tmux window.
    pub fn kill_window(&mut self) {
        if let Some(window) = self.current_tab_window() {
            logger::debug(&format!("app: kill window @{}", window.id));
            let _ = self.nested_driver.kill_window(window.id);
            let _ = self.refresh_windows();
        }
    }

    /// Renames the selected window: opens the tmux command prompt on the
    /// client the TUI inherited, pre-filled with the window's current name,
    /// targeting the window in the nested session. The rename itself arrives
    /// later through the usual control-mode refresh.
    pub fn rename_window(&self) {
        if let Some(window) = self.current_tab_window() {
            logger::debug(&format!("app: rename window @{}", window.id));
            let target = format!("{}:{}", self.nested_driver.session_name(), window.id);
            let template = format!("rename-window -t \"{target}\" \"%%\"");
            let _ = self.nested_driver.command_prompt(&window.name, &template);
        }
    }

    /// Switches to the given tab if it is not empty.
    pub fn switch_tab(&mut self, tab: Tab) {
        if !self.is_tab_empty(tab) {
            logger::debug(&format!("app: switch tab -> {tab:?}"));
            let current_dir = self
                .current_tab_window()
                .map(|w| w.current_dir.clone())
                .unwrap_or_default();

            let dest_indices = self.indices_for_tab(tab);
            let target_pos = if !current_dir.is_empty() {
                dest_indices
                    .iter()
                    .position(|&i| self.windows[i].current_dir == current_dir)
                    .unwrap_or(0)
            } else {
                0
            };

            self.active_tab = tab;
            self.set_selected_for_tab(tab, target_pos);
            self.list_state.select(Some(target_pos));
        }
    }

    /// Reloads the window list from the tmux driver and tracks start times.
    pub fn refresh_windows(&mut self) -> anyhow::Result<()> {
        let windows = self.nested_driver.list_windows()?;
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

    /// Returns whether the TUI pane is focused. Always true when focus
    /// tracking is inactive.
    pub fn pane_active(&self) -> bool {
        self.pane_active
    }

    /// Returns the tmux key sequences to advertise when the pane is not
    /// focused: (key sequence, label) pairs, e.g. ("C-b C-b", "prefix"),
    /// ("c", "new").
    pub fn tmux_hints(&self) -> &[(String, String)] {
        &self.tmux_hints
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
    use crate::backends::tmux::{KeyBinding, Tmux, TmuxError, Window};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    struct MockTmux {
        calls: Rc<std::cell::RefCell<Vec<String>>>,
        keys: Option<Vec<KeyBinding>>,
        next_id: Rc<std::cell::RefCell<u32>>,
        prefix: Option<String>,
        windows: Rc<std::cell::RefCell<Vec<Window>>>,
    }

    impl MockTmux {
        fn new() -> Self {
            Self {
                calls: Rc::new(std::cell::RefCell::new(Vec::new())),
                keys: Some(default_key_bindings()),
                next_id: Rc::new(std::cell::RefCell::new(5)),
                prefix: Some("C-b".to_string()),
                windows: Rc::new(std::cell::RefCell::new(vec![
                    Window {
                        current_dir: "/home/user/project1".to_string(),
                        id: 1,
                        is_active: false,
                        name: "agent-1".to_string(),
                        notification_pending: false,
                        running_command: "cargo build".to_string(),
                        started_at: Some(Instant::now() - Duration::from_secs(125)),
                    },
                    Window {
                        current_dir: "/home/user/project2".to_string(),
                        id: 2,
                        is_active: false,
                        name: "agent-2".to_string(),
                        notification_pending: true,
                        running_command: "claude".to_string(),
                        started_at: Some(Instant::now() - Duration::from_secs(45)),
                    },
                    Window {
                        current_dir: "/home/user/project3".to_string(),
                        id: 3,
                        is_active: false,
                        name: "agent-3".to_string(),
                        notification_pending: false,
                        running_command: "python main.py".to_string(),
                        started_at: Some(Instant::now() - Duration::from_secs(300)),
                    },
                    Window {
                        current_dir: "/home/user/project4".to_string(),
                        id: 4,
                        is_active: false,
                        name: "agent-4".to_string(),
                        notification_pending: false,
                        running_command: "opencode".to_string(),
                        started_at: Some(Instant::now() - Duration::from_secs(10)),
                    },
                ])),
            }
        }

        fn windows_rc(&self) -> Rc<std::cell::RefCell<Vec<Window>>> {
            self.windows.clone()
        }

        fn calls_rc(&self) -> Rc<std::cell::RefCell<Vec<String>>> {
            self.calls.clone()
        }
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
            self.calls.borrow_mut().push("list_windows".to_string());
            Ok(self.windows.borrow().clone())
        }

        fn create_window(&self, name: &str) -> Result<Window, TmuxError> {
            self.calls.borrow_mut().push("create_window".to_string());
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
            self.calls.borrow_mut().push("select_window".to_string());
            Ok(())
        }

        fn last_pane(&self) -> Result<(), TmuxError> {
            self.calls.borrow_mut().push("last_pane".to_string());
            Ok(())
        }

        fn split_window(&self, _command: &str, _width: u16) -> Result<String, TmuxError> {
            Ok("%99".to_string())
        }

        fn resize_pane(&self, pane_id: &str, width: u16) -> Result<(), TmuxError> {
            self.calls
                .borrow_mut()
                .push(format!("resize_pane {pane_id} {width}"));
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

        fn command_prompt(&self, initial: &str, template: &str) -> Result<(), TmuxError> {
            self.calls
                .borrow_mut()
                .push(format!("command_prompt {initial} {template}"));
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

    fn test_app() -> (
        App,
        Rc<std::cell::RefCell<Vec<Window>>>,
        Rc<std::cell::RefCell<Vec<String>>>,
    ) {
        let driver = MockTmux::new();
        let windows = driver.windows_rc();
        let calls = driver.calls_rc();
        let app = App::new(Box::new(driver), Box::new(MockTmux::new()), None, None).unwrap();
        (app, windows, calls)
    }

    #[test]
    fn test_new() {
        let (app, _, _) = test_app();
        assert!(app.running);
        assert_eq!(app.active_tab(), Tab::Agents);
        assert_eq!(app.windows().len(), 4);
    }

    #[test]
    fn test_new_defaults_to_windows_when_no_agents() {
        let driver = MockTmux::new();
        driver.windows.borrow_mut()[1].running_command = "bash".to_string();
        driver.windows.borrow_mut()[3].running_command = "zsh".to_string();
        let app = App::new(Box::new(driver), Box::new(MockTmux::new()), None, None).unwrap();
        assert_eq!(app.active_tab(), Tab::Windows);
    }

    #[test]
    fn test_quit() {
        let (mut app, _, _) = test_app();
        app.quit();
        assert!(!app.running);
    }

    #[test]
    fn test_navigate_down() {
        let (mut app, _, _) = test_app();
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
        let (mut app, _, _) = test_app();
        app.navigate_down();
        assert_eq!(app.current_selected(), 1);
        app.navigate_up();
        assert_eq!(app.current_selected(), 0);
        app.navigate_up();
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_focus_window() {
        let (mut app, _, _) = test_app();
        app.navigate_down();
        app.focus_window();
    }

    #[test]
    fn test_create_window() {
        let (mut app, _, _) = test_app();
        let initial_len = app.windows().len();
        app.create_window();
        assert_eq!(app.windows().len(), initial_len + 1);
    }

    #[test]
    fn test_create_window_switches_to_windows_tab() {
        let (mut app, _, _) = test_app();
        assert_eq!(app.active_tab(), Tab::Agents);
        app.create_window();
        assert_eq!(app.active_tab(), Tab::Windows);
    }

    #[test]
    fn test_create_window_selects_new_window() {
        let (mut app, _, _) = test_app();
        app.create_window();
        assert_eq!(app.active_tab(), Tab::Windows);
        let windows_tab_len = app.current_tab_len();
        assert_eq!(app.current_selected(), windows_tab_len - 1);
    }

    #[test]
    fn test_create_window_calls_focus_window() {
        let nested_driver = MockTmux::new();
        let parent_driver = MockTmux::new();
        let nested_calls = nested_driver.calls_rc();
        let parent_calls = parent_driver.calls_rc();
        let mut app =
            App::new(Box::new(nested_driver), Box::new(parent_driver), None, None).unwrap();
        app.create_window();
        let nested_recorded = nested_calls.borrow();
        assert!(nested_recorded.contains(&"select_window".to_string()));
        let parent_recorded = parent_calls.borrow();
        assert!(parent_recorded.contains(&"last_pane".to_string()));
    }

    #[test]
    fn test_kill_window() {
        let (mut app, _, _) = test_app();
        let initial_agents = app.current_tab_len();
        app.kill_window();
        assert_eq!(app.current_tab_len(), initial_agents - 1);
    }

    #[test]
    fn test_kill_last_window_adjusts_selection() {
        let (mut app, _, _) = test_app();
        app.navigate_down();
        assert_eq!(app.current_selected(), 1);
        app.kill_window();
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_rename_window_prompts_for_selected_window() {
        let nested = MockTmux::new();
        let calls = nested.calls_rc();
        let app = App::new(Box::new(nested), Box::new(MockTmux::new()), None, None).unwrap();

        // Agents tab, first selection: agent-2, the claude window (id 2).
        app.rename_window();

        assert_eq!(
            calls.borrow().last().unwrap(),
            "command_prompt agent-2 rename-window -t \"agents-on-tmux:2\" \"%%\""
        );
    }

    #[test]
    fn test_handle_action_triggers_rename() {
        let nested = MockTmux::new();
        let calls = nested.calls_rc();
        let mut app = App::new(Box::new(nested), Box::new(MockTmux::new()), None, None).unwrap();

        app.handle_action(Action::RenameWindow);

        assert!(
            calls
                .borrow()
                .iter()
                .any(|call| call.starts_with("command_prompt"))
        );
    }

    #[test]
    fn test_handle_action_quit() {
        let (mut app, _, _) = test_app();
        app.handle_action(Action::Quit);
        assert!(app.running);
        app.handle_action(Action::Quit);
        assert!(!app.running);
    }

    #[test]
    fn test_handle_action_navigate() {
        let (mut app, _, _) = test_app();
        app.handle_action(Action::NavigateDown);
        assert_eq!(app.current_selected(), 1);
        app.handle_action(Action::NavigateUp);
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_kill_window_requires_double_press() {
        let (mut app, _, _) = test_app();
        let initial_agents = app.current_tab_len();
        assert_eq!(app.pending_action(), None);
        app.handle_action(Action::KillWindow);
        assert_eq!(app.pending_action(), Some(PendingAction::KillWindow));
        assert_eq!(app.current_tab_len(), initial_agents);
        app.handle_action(Action::KillWindow);
        assert_eq!(app.pending_action(), None);
        assert_eq!(app.current_tab_len(), initial_agents - 1);
    }

    #[test]
    fn test_quit_requires_double_press() {
        let (mut app, _, _) = test_app();
        assert!(app.running);
        assert_eq!(app.pending_action(), None);
        app.handle_action(Action::Quit);
        assert_eq!(app.pending_action(), Some(PendingAction::Quit));
        assert!(app.running);
        app.handle_action(Action::Quit);
        assert_eq!(app.pending_action(), None);
        assert!(!app.running);
    }

    #[test]
    fn test_other_action_cancels_pending_kill() {
        let (mut app, _, _) = test_app();
        app.handle_action(Action::KillWindow);
        assert_eq!(app.pending_action(), Some(PendingAction::KillWindow));
        app.handle_action(Action::NavigateDown);
        assert_eq!(app.pending_action(), None);
    }

    #[test]
    fn test_other_action_cancels_pending_quit() {
        let (mut app, _, _) = test_app();
        app.handle_action(Action::Quit);
        assert_eq!(app.pending_action(), Some(PendingAction::Quit));
        app.handle_action(Action::NavigateDown);
        assert_eq!(app.pending_action(), None);
        assert!(app.running);
    }

    #[test]
    fn test_none_action_clears_pending() {
        let (mut app, _, _) = test_app();
        app.handle_action(Action::KillWindow);
        assert_eq!(app.pending_action(), Some(PendingAction::KillWindow));
        app.handle_action(Action::None);
        assert_eq!(app.pending_action(), None);

        app.handle_action(Action::Quit);
        assert_eq!(app.pending_action(), Some(PendingAction::Quit));
        app.handle_action(Action::None);
        assert_eq!(app.pending_action(), None);
    }

    #[test]
    fn test_app_new_has_no_event_receiver() {
        let (app, _, _) = test_app();
        assert!(app.event_rx.is_none());
    }

    fn default_key_bindings() -> Vec<KeyBinding> {
        [
            ("C-b", "send-prefix"),
            ("c", "new-window"),
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

    fn hints_app(parent: MockTmux) -> App {
        let parent = parent;
        let driver = MockTmux::new();
        App::new(
            Box::new(driver),
            Box::new(parent),
            None,
            Some("%7".to_string()),
        )
        .unwrap()
    }

    #[test]
    fn test_tmux_hints_resolved_when_tracking() {
        let app = hints_app(MockTmux::new());
        assert_eq!(
            app.tmux_hints(),
            [
                ("C-b C-b".to_string(), "prefix".to_string()),
                ("c".to_string(), "new".to_string()),
                ("n".to_string(), "next".to_string()),
                ("p".to_string(), "prev".to_string()),
                ("l".to_string(), "last".to_string()),
                (",".to_string(), "rename".to_string()),
            ]
        );
    }

    #[test]
    fn test_tmux_hints_empty_without_pane_id() {
        let (app, _, _) = test_app();
        assert!(app.tmux_hints().is_empty());
    }

    #[test]
    fn test_tmux_hints_skip_unbound_commands() {
        let mut parent = MockTmux::new();
        parent.keys = Some(
            default_key_bindings()
                .into_iter()
                .filter(|b| b.command != "last-window")
                .collect(),
        );
        let app = hints_app(parent);
        assert_eq!(app.tmux_hints().len(), 5);
        assert!(!app.tmux_hints().iter().any(|(_, label)| label == "last"));
    }

    #[test]
    fn test_tmux_hints_send_prefix_falls_back_to_prefix() {
        let mut parent = MockTmux::new();
        parent.prefix = Some("C-a".to_string());
        parent.keys = Some(
            default_key_bindings()
                .into_iter()
                .filter(|b| b.command != "send-prefix")
                .collect(),
        );
        let app = hints_app(parent);
        assert_eq!(
            app.tmux_hints()[0],
            ("C-a C-a".to_string(), "prefix".to_string())
        );
        assert!(
            app.tmux_hints()
                .iter()
                .any(|(key, label)| key == "c" && label == "new")
        );
    }

    #[test]
    fn test_tmux_hints_use_bound_send_prefix_key() {
        let mut parent = MockTmux::new();
        parent.prefix = Some("C-Space".to_string());
        let mut keys = default_key_bindings();
        keys[0].key = "C-b".to_string();
        parent.keys = Some(keys);
        let app = hints_app(parent);
        assert_eq!(
            app.tmux_hints()[0],
            ("C-Space C-b".to_string(), "prefix".to_string())
        );
    }

    #[test]
    fn test_tmux_hints_match_first_word_of_command() {
        let mut parent = MockTmux::new();
        let mut keys = default_key_bindings();
        keys[1].command = "new-window -c #{pane_current_path}".to_string();
        parent.keys = Some(keys);
        let app = hints_app(parent);
        assert_eq!(app.tmux_hints()[1], ("c".to_string(), "new".to_string()));
    }

    #[test]
    fn test_tmux_hints_match_command_prompt_with_rename() {
        let parent = MockTmux::new();
        let app = hints_app(parent);
        assert!(
            app.tmux_hints()
                .iter()
                .any(|(key, label)| key == "," && label == "rename")
        );
    }

    #[test]
    fn test_tmux_hints_do_not_match_menu_commands() {
        let mut parent = MockTmux::new();
        let mut keys = default_key_bindings();
        keys.push(KeyBinding {
            key: "<".to_string(),
            command: "display-menu -T \"Window menu\" #{window_index} rename-window".to_string(),
        });
        parent.keys = Some(keys);
        let app = hints_app(parent);
        let rename_hint = app.tmux_hints().iter().find(|(_, label)| label == "rename");
        assert!(rename_hint.is_some());
        assert_ne!(rename_hint.unwrap().0, "<");
    }

    #[test]
    fn test_tmux_hints_empty_when_list_keys_fails() {
        let mut parent = MockTmux::new();
        parent.keys = None;
        let app = hints_app(parent);
        assert!(app.tmux_hints().is_empty());
    }

    #[test]
    fn test_tmux_hints_empty_when_prefix_fails() {
        let mut parent = MockTmux::new();
        parent.prefix = None;
        let app = hints_app(parent);
        assert!(app.tmux_hints().is_empty());
    }

    #[test]
    fn test_tmux_hints_empty_when_prefix_blank() {
        let mut parent = MockTmux::new();
        parent.prefix = Some(String::new());
        let app = hints_app(parent);
        assert!(app.tmux_hints().is_empty());
    }

    #[test]
    fn test_new_without_pane_id_assumes_focused() {
        let driver = MockTmux::new();
        let app = App::new(Box::new(driver), Box::new(MockTmux::new()), None, None).unwrap();
        assert!(app.pane_active());
    }

    #[test]
    fn test_set_pane_active_toggles() {
        let mut app = App::new(
            Box::new(MockTmux::new()),
            Box::new(MockTmux::new()),
            None,
            Some("%7".to_string()),
        )
        .unwrap();
        assert!(app.pane_active());
        app.set_pane_active(false);
        assert!(!app.pane_active());
        app.set_pane_active(true);
        assert!(app.pane_active());
    }

    #[test]
    fn test_process_tmux_events_without_receiver_is_noop() {
        let (mut app, _, _) = test_app();
        assert!(!app.process_tmux_events());
        assert!(app.running);
        assert_eq!(app.windows().len(), 4);
    }

    #[test]
    fn test_refresh_event_reloads_windows() {
        let (mut app, windows, _) = test_app();
        let (tx, rx) = mpsc::channel();
        app.event_rx = Some(rx);

        // A structural change happened externally; the event tells us to reload.
        windows.borrow_mut().push(Window {
            current_dir: "/home/user/project5".to_string(),
            id: 99,
            is_active: false,
            name: "agent-5".to_string(),
            notification_pending: false,
            running_command: "bash".to_string(),
            started_at: None,
        });
        let _ = tx.send(TmuxEvent::Refresh);

        assert!(app.process_tmux_events());
        assert_eq!(app.windows().len(), 5);
    }

    #[test]
    fn test_multiple_events_refresh_once() {
        // Coalescing: many events, but list_windows is re-read a single time.
        let (mut app, _, calls) = test_app();
        let (tx, rx) = mpsc::channel();
        app.event_rx = Some(rx);
        calls.borrow_mut().clear();

        for _ in 0..10 {
            let _ = tx.send(TmuxEvent::Refresh);
        }
        app.process_tmux_events();

        let list_calls = calls
            .borrow()
            .iter()
            .filter(|c| *c == "list_windows")
            .count();
        assert_eq!(list_calls, 1);
        assert!(app.running);
    }

    #[test]
    fn test_exit_event_quits_app() {
        let (mut app, _, _) = test_app();
        let (tx, rx) = mpsc::channel();
        app.event_rx = Some(rx);

        let _ = tx.send(TmuxEvent::Exit);
        assert!(!app.process_tmux_events());

        assert!(!app.running);
    }

    #[test]
    fn test_refresh_windows_new_windows_get_start_time() {
        let (app, _, _) = test_app();
        assert!(app.windows().iter().all(|w| w.started_at.is_some()));
    }

    #[test]
    fn test_refresh_windows_existing_windows_keep_time() {
        let (mut app, _, _) = test_app();
        let first_times: Vec<Option<Instant>> =
            app.windows().iter().map(|w| w.started_at).collect();
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.refresh_windows().unwrap();
        let second_times: Vec<Option<Instant>> =
            app.windows().iter().map(|w| w.started_at).collect();
        assert_eq!(first_times, second_times);
    }

    #[test]
    fn test_refresh_windows_removed_windows_cleaned_up() {
        let (mut app, windows, _) = test_app();
        assert_eq!(app.window_starts.len(), 4);
        let window_id = app.windows()[0].id;
        windows.borrow_mut().retain(|w| w.id != window_id);
        app.refresh_windows().unwrap();
        assert_eq!(app.window_starts.len(), 3);
        assert!(!app.window_starts.contains_key(&window_id));
    }

    #[test]
    fn test_external_tmux_change_syncs_selection() {
        let (mut app, windows, _) = test_app();
        assert_eq!(app.current_selected(), 0);

        windows.borrow_mut()[3].is_active = true;
        app.refresh_windows().unwrap();
        assert_eq!(app.current_selected(), 1);
    }

    #[test]
    fn test_external_tmux_change_switches_tab() {
        let (mut app, windows, _) = test_app();
        assert_eq!(app.active_tab(), Tab::Agents);

        windows.borrow_mut()[0].is_active = true;
        app.refresh_windows().unwrap();
        assert_eq!(app.active_tab(), Tab::Windows);
    }

    #[test]
    fn test_external_tmux_change_syncs_after_navigation() {
        let (mut app, windows, _) = test_app();
        assert_eq!(app.current_selected(), 0);

        app.navigate_down();
        assert_eq!(app.current_selected(), 1);
        assert_eq!(app.last_focused_id, Some(4));

        windows.borrow_mut()[0].is_active = true;
        app.refresh_windows().unwrap();
        assert_eq!(app.active_tab(), Tab::Windows);
        assert_eq!(app.current_selected(), 0);
        assert_eq!(app.last_focused_id, Some(1));
    }

    #[test]
    fn test_selected_window_moving_to_other_tab_switches_tab() {
        let (mut app, windows, _) = test_app();
        assert_eq!(app.active_tab(), Tab::Agents);
        assert_eq!(app.current_tab_len(), 2);

        let selected_id = app.current_tab_window().unwrap().id;
        assert_eq!(selected_id, 2);

        windows.borrow_mut()[1].running_command = "bash".to_string();
        app.refresh_windows().unwrap();

        assert_eq!(app.active_tab(), Tab::Windows);
        assert_eq!(app.current_tab_window().unwrap().id, selected_id);
    }

    #[test]
    fn test_switch_tab() {
        let (mut app, _, _) = test_app();
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
        let mut app = App::new(Box::new(driver), Box::new(MockTmux::new()), None, None).unwrap();
        assert_eq!(app.active_tab(), Tab::Windows);
        assert!(app.is_tab_empty(Tab::Agents));
        app.switch_tab(Tab::Agents);
        assert_eq!(app.active_tab(), Tab::Windows);
    }

    #[test]
    fn test_switch_tab_selects_first_when_no_match() {
        let (mut app, _, _) = test_app();

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
    fn test_switch_tab_selects_matching_current_dir() {
        let driver = MockTmux::new();
        driver.windows.borrow_mut()[1].current_dir = "/home/user/shared".to_string();
        driver.windows.borrow_mut()[2].current_dir = "/home/user/shared".to_string();
        let mut app = App::new(Box::new(driver), Box::new(MockTmux::new()), None, None).unwrap();

        assert_eq!(app.active_tab(), Tab::Agents);
        assert_eq!(app.current_selected(), 0);

        app.switch_tab(Tab::Windows);
        assert_eq!(app.current_selected(), 1);
    }

    #[test]
    fn test_switch_tab_selects_first_match_when_multiple() {
        let driver = MockTmux::new();
        driver.windows.borrow_mut()[1].current_dir = "/home/user/shared".to_string();
        driver.windows.borrow_mut()[0].current_dir = "/home/user/shared".to_string();
        driver.windows.borrow_mut()[2].current_dir = "/home/user/shared".to_string();
        let mut app = App::new(Box::new(driver), Box::new(MockTmux::new()), None, None).unwrap();

        assert_eq!(app.active_tab(), Tab::Agents);
        assert_eq!(app.current_selected(), 0);

        app.switch_tab(Tab::Windows);
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_switch_tab_selects_first_when_current_dir_empty() {
        let driver = MockTmux::new();
        driver.windows.borrow_mut()[0].current_dir = String::new();
        driver.windows.borrow_mut()[2].current_dir = "/home/user/project3".to_string();
        let mut app = App::new(Box::new(driver), Box::new(MockTmux::new()), None, None).unwrap();

        assert_eq!(app.active_tab(), Tab::Agents);
        assert_eq!(app.current_selected(), 0);

        app.switch_tab(Tab::Windows);
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_is_tab_empty() {
        let (app, _, _) = test_app();
        assert!(!app.is_tab_empty(Tab::Agents));
        assert!(!app.is_tab_empty(Tab::Windows));
    }

    #[test]
    fn test_current_tab_windows() {
        let (app, _, _) = test_app();
        let agent_windows = app.current_tab_windows();
        assert_eq!(agent_windows.len(), 2);
        assert!(is_agent(&agent_windows[0].running_command).is_some());
        assert!(is_agent(&agent_windows[1].running_command).is_some());
    }

    #[test]
    fn test_ensure_visible_no_change_when_already_visible() {
        let (mut app, _, _) = test_app();
        assert_eq!(app.list_state().offset(), 0);

        app.ensure_visible(3);
        assert_eq!(app.list_state().offset(), 0);
    }

    #[test]
    fn test_ensure_visible_scrolls_down_when_selected_below_visible() {
        let (mut app, _, _) = test_app();
        app.navigate_down();
        assert_eq!(app.current_selected(), 1);

        app.ensure_visible(1);
        assert_eq!(app.list_state().offset(), 1);
    }

    #[test]
    fn test_ensure_visible_scrolls_up_when_selected_above_visible() {
        let (mut app, _, _) = test_app();
        app.navigate_down();
        *app.list_state.offset_mut() = 1;

        app.navigate_up();
        assert_eq!(app.current_selected(), 0);

        app.ensure_visible(1);
        assert_eq!(app.list_state().offset(), 0);
    }

    #[test]
    fn test_ensure_visible_no_scroll_when_zero_visible_count() {
        let (mut app, _, _) = test_app();
        app.navigate_down();

        app.ensure_visible(0);
        assert_eq!(app.list_state().offset(), 0);
    }

    #[test]
    fn test_enforce_panel_width_resizes_when_width_differs() {
        let parent = MockTmux::new();
        let parent_calls = parent.calls_rc();
        let app = App::new(
            Box::new(MockTmux::new()),
            Box::new(parent),
            Some(("%5".to_string(), 35)),
            None,
        )
        .unwrap();

        app.enforce_panel_width(50);

        assert_eq!(
            parent_calls.borrow().as_slice(),
            ["resize_pane %5 35".to_string()]
        );
    }

    #[test]
    fn test_enforce_panel_width_noop_when_width_matches() {
        let parent = MockTmux::new();
        let parent_calls = parent.calls_rc();
        let app = App::new(
            Box::new(MockTmux::new()),
            Box::new(parent),
            Some(("%5".to_string(), 35)),
            None,
        )
        .unwrap();

        app.enforce_panel_width(35);

        assert!(parent_calls.borrow().is_empty());
    }

    #[test]
    fn test_enforce_panel_width_noop_outside_panel_mode() {
        let parent = MockTmux::new();
        let parent_calls = parent.calls_rc();
        let app = App::new(Box::new(MockTmux::new()), Box::new(parent), None, None).unwrap();
        assert_eq!(app.panel, None);

        app.enforce_panel_width(50);

        assert!(parent_calls.borrow().is_empty());
    }
}

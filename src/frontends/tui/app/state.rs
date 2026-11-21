use std::sync::mpsc;
use std::time::Instant;

use ratatui::layout::Rect;
use ratatui::widgets::ListState;

use super::hints::resolve_tmux_hints;
use crate::backends::agents::is_agent;
use crate::backends::logger;
use crate::backends::process::Processes;
use crate::backends::tmux::{Tmux, TmuxEvent, Window};
use crate::frontends::tui::event::{PendingAction, Tab};
use crate::frontends::tui::ui;

/// Main application state for the TUI frontend.
pub struct App {
    pub(super) active_tab: Tab,
    pub(super) agents_selected: usize,
    pub(super) event_rx: Option<mpsc::Receiver<TmuxEvent>>,
    pub(super) last_focused_id: Option<u32>,
    pub(super) list_state: ListState,
    pub(super) nested_driver: Box<dyn Tmux>,
    pub(super) pane_active: bool,
    pub(super) pane_id: Option<String>,
    pub(super) panel_width: Option<u16>,
    pub(super) parent_driver: Box<dyn Tmux>,
    pub(super) processes: Box<dyn Processes>,
    pub(super) pending_action: Option<PendingAction>,
    pub(super) regions: Option<Vec<ui::ClickableRegion>>,
    pub(super) running: bool,
    pub(super) tmux_hints: Vec<(String, String)>,
    pub(super) windows: Vec<Window>,
    pub(super) windows_selected: usize,
}

impl App {
    /// Creates a new App, loading windows from the tmux driver. `processes`
    /// tells when each window's program started. `pane_id` is the pane the
    /// TUI runs in, when known; it enables focus tracking.
    /// `panel_width` is the width to re-assert on the pane whenever tmux
    /// rescales the layout; `None` disables enforcement.
    pub fn new(
        nested_driver: Box<dyn Tmux>,
        parent_driver: Box<dyn Tmux>,
        processes: Box<dyn Processes>,
        pane_id: Option<String>,
        panel_width: Option<u16>,
    ) -> anyhow::Result<Self> {
        // Bindings are read once: rebinds while aot runs are not picked up.
        let tmux_hints = if pane_id.is_some() {
            resolve_tmux_hints(nested_driver.as_ref(), parent_driver.as_ref())
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
            pane_active: panel_width.is_none(),
            pane_id,
            panel_width,
            parent_driver,
            pending_action: None,
            processes,
            regions: None,
            running: true,
            tmux_hints,
            windows: Vec::new(),
            windows_selected: 0,
        };
        app.refresh_windows()?;
        Ok(app)
    }

    /// Signals the application to stop running.
    pub(super) fn quit(&mut self) {
        logger::info!("app: quit");
        self.running = false;
    }

    /// Updates the pane focus state.
    pub fn set_pane_active(&mut self, active: bool) {
        if self.pane_active != active {
            logger::debug!("app: pane active {active}");
            self.pane_active = active;
        }
    }

    /// Moves the selection up by one window within the current tab.
    pub(super) fn navigate_up(&mut self) {
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
    pub(super) fn focus_window(&self) {
        if let Some(window) = self.current_tab_window() {
            logger::debug!("app: focus window @{}", window.id);
            if let Err(error) = self.nested_driver.select_window(window.id) {
                logger::error!("app: select window failed: {error}");
            }
            if let Err(error) = self.parent_driver.last_pane() {
                logger::error!("app: last pane failed: {error}");
            }
        }
    }

    /// Creates a new tmux window and adds it to the list.
    pub(super) fn create_window(&mut self) {
        self.active_tab = Tab::Windows;
        logger::debug!("app: create window");
        match self.nested_driver.create_window(None, None, None, false) {
            Ok(new_window) => {
                if let Err(error) = self.refresh_windows() {
                    logger::error!("app: refresh windows failed: {error}");
                }
                let indices = self.current_tab_indices();
                if let Some(pos) = indices
                    .iter()
                    .position(|&i| self.window_id_at(i) == Some(new_window.id))
                {
                    self.windows_selected = pos;
                    self.list_state.select(Some(self.windows_selected));
                }
                self.focus_window();
            }
            Err(error) => {
                logger::error!("app: create window failed: {error}");
            }
        }
    }

    /// Kills the currently selected tmux window.
    pub fn kill_window(&mut self) {
        if let Some(window) = self.current_tab_window() {
            logger::debug!("app: kill window @{}", window.id);
            if let Err(error) = self.nested_driver.kill_window(window.id) {
                logger::error!("app: kill window failed: {error}");
            }
            if let Err(error) = self.refresh_windows() {
                logger::error!("app: refresh windows failed: {error}");
            }
        }
    }

    /// Renames the selected window: opens the tmux command prompt on the
    /// client the TUI inherited, pre-filled with the window's current name,
    /// targeting the window in the nested session. The rename itself arrives
    /// later through the usual control-mode refresh.
    pub(super) fn rename_window(&self) {
        if let Some(window) = self.current_tab_window() {
            logger::debug!("app: rename window @{}", window.id);
            if let Err(error) = self.parent_driver.last_pane() {
                logger::error!("app: last pane failed: {error}");
            }
            if let Err(error) = self
                .nested_driver
                .prompt_rename_window(window.id, &window.name)
            {
                logger::error!("app: rename window failed: {error}");
            }
            if let Err(error) = self.parent_driver.last_pane() {
                logger::error!("app: last pane failed: {error}");
            }
        }
    }

    /// Switches to the given tab if it is not empty.
    pub fn switch_tab(&mut self, tab: Tab) {
        if !self.is_tab_empty(tab) {
            logger::debug!("app: switch tab -> {tab:?}");
            let current_dir = self
                .current_tab_window()
                .map(|w| w.current_dir.clone())
                .unwrap_or_default();

            let dest_indices = self.indices_for_tab(tab);
            let target_pos = if !current_dir.is_empty() {
                dest_indices
                    .iter()
                    .position(|&i| {
                        self.windows
                            .get(i)
                            .is_some_and(|w| w.current_dir == current_dir)
                    })
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
    pub(super) fn refresh_windows(&mut self) -> anyhow::Result<()> {
        let windows = self.nested_driver.list_windows()?;

        let selected_window_id = self.current_tab_window().map(|w| w.id);
        let is_first_refresh = self.windows.is_empty();

        let enriched_windows: Vec<Window> = windows
            .into_iter()
            .map(|window| {
                let started_at = self
                    .windows
                    .iter()
                    .find(|w| w.id == window.id)
                    .and_then(|old| {
                        if old.pane_pid == window.pane_pid {
                            old.started_at
                        } else {
                            None
                        }
                    })
                    .or_else(|| self.processes.started_at(window.pane_pid))
                    .or(Some(Instant::now()));

                Window {
                    started_at,
                    ..window
                }
            })
            .collect();

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
                    .position(|&i| self.window_id_at(i) == Some(selected_id))
                {
                    self.active_tab = new_tab;
                    self.set_selected_for_tab(new_tab, pos);
                    self.list_state.select(Some(self.current_selected()));
                }
            }
        }

        self.clamp_selections();
        self.list_state.select(Some(self.current_selected()));

        if is_first_refresh || !self.pane_active {
            let active_window_info = self
                .windows
                .iter()
                .find(|w| w.is_active)
                .map(|w| (w.id, w.running_command.clone()));

            if let Some((active_id, active_command)) = active_window_info {
                let current_selected_id = self.current_tab_window().map(|w| w.id);
                if current_selected_id != Some(active_id) {
                    let target_tab = if is_agent(&active_command).is_some() {
                        Tab::Agents
                    } else {
                        Tab::Windows
                    };

                    let indices = self.indices_for_tab(target_tab);
                    if let Some(pos) = indices
                        .iter()
                        .position(|&i| self.window_id_at(i) == Some(active_id))
                    {
                        self.active_tab = target_tab;
                        self.set_selected_for_tab(target_tab, pos);
                        self.last_focused_id = Some(active_id);
                        self.list_state.select(Some(self.current_selected()));
                    }
                }
            } else if is_first_refresh && !self.is_tab_empty(Tab::Agents) {
                self.active_tab = Tab::Agents;
            }
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

    /// Returns cached clickable regions for the given terminal size.
    /// Recomputes only if cache is empty (invalidated).
    pub(super) fn clickable_regions(&mut self, size: Rect) -> Vec<ui::ClickableRegion> {
        match &self.regions {
            Some(regions) => regions.clone(),
            None => {
                let regions = ui::clickable_regions(size, self);
                self.regions = Some(regions.clone());
                regions
            }
        }
    }

    /// Invalidates the cached clickable regions, for when the layout changes
    /// (e.g. resize, scroll position, tab switch).
    pub(super) fn invalidate_clickable_regions(&mut self) {
        self.regions = None;
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
    fn current_tab_window(&self) -> Option<&Window> {
        let indices = self.current_tab_indices();
        let selected = self.current_selected();
        indices.get(selected).and_then(|&i| self.windows.get(i))
    }

    /// Returns the id of the window at the given position in the full list.
    fn window_id_at(&self, index: usize) -> Option<u32> {
        self.windows.get(index).map(|window| window.id)
    }

    /// Returns the filtered windows for the current tab.
    pub fn current_tab_windows(&self) -> Vec<&Window> {
        self.current_tab_indices()
            .iter()
            .filter_map(|&i| self.windows.get(i))
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

    /// Scrolls the list by the given direction (-1 for up, 1 for down).
    /// The visible_count is the number of items that fit in the viewport.
    pub(super) fn scroll(&mut self, direction: i8, visible_count: usize) {
        if visible_count == 0 {
            return;
        }

        let total_items = self.current_tab_len();
        if total_items == 0 {
            return;
        }

        let current_offset = self.list_state.offset();
        let max_offset = total_items.saturating_sub(visible_count);

        let new_offset = if direction > 0 {
            (current_offset + 1).min(max_offset)
        } else if direction < 0 {
            current_offset.saturating_sub(1)
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

    pub(super) fn set_current_selected(&mut self, idx: usize) {
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
    use super::super::test_fixtures::{new_app, session, test_app};
    use super::*;
    use crate::backends::process::MockProcesses;
    use crate::frontends::tui::test_fake_tmux::FakeTmux;
    use mockall::predicate::eq;
    use std::time::Duration;

    fn app_with_processes(nested: &FakeTmux, processes: MockProcesses) -> App {
        App::new(
            Box::new(nested.mock()),
            Box::new(session().mock()),
            Box::new(processes),
            None,
            None,
        )
        .unwrap()
    }

    #[test]
    fn test_refresh_windows_takes_start_time_from_process() {
        let started = Instant::now() - Duration::from_secs(600);
        let mut processes = MockProcesses::new();
        processes
            .expect_started_at()
            .returning(move |_| Some(started));

        let app = app_with_processes(&session(), processes);

        assert!(app.windows().iter().all(|w| w.started_at == Some(started)));
    }

    #[test]
    fn test_refresh_windows_looks_up_start_time_again_when_pid_changes() {
        let tmux = session();
        let restarted = Instant::now() - Duration::from_secs(5);
        let mut processes = MockProcesses::new();
        processes.expect_started_at().times(4).returning(|_| None);
        processes
            .expect_started_at()
            .with(eq(20001))
            .times(1)
            .returning(move |_| Some(restarted));
        let mut app = app_with_processes(&tmux, processes);

        tmux.windows.lock().unwrap()[0].pane_pid = 20001;
        app.refresh_windows().unwrap();

        assert_eq!(app.windows()[0].started_at, Some(restarted));
    }

    #[test]
    fn test_new() {
        let (app, _) = test_app();
        assert!(app.running);
        assert_eq!(app.active_tab(), Tab::Agents);
        assert_eq!(app.windows().len(), 4);
    }

    #[test]
    fn test_new_defaults_to_windows_when_no_agents() {
        let driver = session();
        driver.windows.lock().unwrap()[0].is_active = true;
        driver.windows.lock().unwrap()[0].running_command = "bash".to_string();
        driver.windows.lock().unwrap()[1].running_command = "bash".to_string();
        driver.windows.lock().unwrap()[3].running_command = "zsh".to_string();
        let app = new_app(&driver, &session(), None, None);
        assert_eq!(app.active_tab(), Tab::Windows);
    }

    #[test]
    fn test_quit() {
        let (mut app, _) = test_app();
        app.quit();
        assert!(!app.running);
    }

    #[test]
    fn test_navigate_down() {
        let (mut app, _) = test_app();
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
        let (mut app, _) = test_app();
        app.navigate_down();
        assert_eq!(app.current_selected(), 1);
        app.navigate_up();
        assert_eq!(app.current_selected(), 0);
        app.navigate_up();
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_focus_window() {
        let (mut app, _) = test_app();
        app.navigate_down();
        app.focus_window();
    }

    #[test]
    fn test_create_window() {
        let (mut app, _) = test_app();
        let initial_len = app.windows().len();
        app.create_window();
        assert_eq!(app.windows().len(), initial_len + 1);
    }

    #[test]
    fn test_create_window_leaves_naming_to_tmux() {
        let (mut app, tmux) = test_app();
        app.create_window();
        assert_eq!(tmux.windows.lock().unwrap().last().unwrap().name, "");
    }

    #[test]
    fn test_create_window_switches_to_windows_tab() {
        let (mut app, _) = test_app();
        assert_eq!(app.active_tab(), Tab::Agents);
        app.create_window();
        assert_eq!(app.active_tab(), Tab::Windows);
    }

    #[test]
    fn test_create_window_selects_new_window() {
        let (mut app, _) = test_app();
        app.create_window();
        assert_eq!(app.active_tab(), Tab::Windows);
        let windows_tab_len = app.current_tab_len();
        assert_eq!(app.current_selected(), windows_tab_len - 1);
    }

    #[test]
    fn test_create_window_calls_focus_window() {
        let nested_driver = session();
        let parent_driver = session();
        let mut app = new_app(&nested_driver, &parent_driver, None, None);
        app.create_window();
        let nested_recorded = nested_driver.recorded();
        assert!(
            nested_recorded
                .iter()
                .any(|call| call.starts_with("select_window"))
        );
        let parent_recorded = parent_driver.recorded();
        assert!(parent_recorded.contains(&"last_pane".to_string()));
    }

    #[test]
    fn test_kill_window() {
        let (mut app, _) = test_app();
        let initial_agents = app.current_tab_len();
        app.kill_window();
        assert_eq!(app.current_tab_len(), initial_agents - 1);
    }

    #[test]
    fn test_kill_last_window_adjusts_selection() {
        let (mut app, _) = test_app();
        app.navigate_down();
        assert_eq!(app.current_selected(), 1);
        app.kill_window();
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_rename_window_prompts_for_selected_window() {
        let nested = session();
        let parent = session();
        let app = new_app(&nested, &parent, None, None);

        // Agents tab, first selection: agent-2, the claude window (id 2).
        app.rename_window();

        assert_eq!(
            nested.recorded().last().unwrap(),
            "prompt_rename_window 2 agent-2"
        );
        let parent_recorded = parent.recorded();
        assert_eq!(parent_recorded.len(), 2);
        assert_eq!(parent_recorded[0], "last_pane");
        assert_eq!(parent_recorded[1], "last_pane");
    }

    #[test]
    fn test_new_without_panel_width_assumes_focused() {
        let driver = session();
        let app = new_app(&driver, &session(), None, None);
        assert!(app.pane_active());
    }

    #[test]
    fn test_set_pane_active_toggles() {
        let mut app = new_app(&session(), &session(), Some("%7"), None);
        assert!(app.pane_active());
        app.set_pane_active(false);
        assert!(!app.pane_active());
        app.set_pane_active(true);
        assert!(app.pane_active());
    }

    #[test]
    fn test_refresh_windows_new_windows_get_start_time() {
        let (app, _) = test_app();
        assert!(app.windows().iter().all(|w| w.started_at.is_some()));
    }

    #[test]
    fn test_refresh_windows_existing_windows_keep_time() {
        let (mut app, _) = test_app();
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
        let (mut app, tmux) = test_app();
        assert_eq!(app.windows().len(), 4);
        let window_id = app.windows()[0].id;
        tmux.windows.lock().unwrap().retain(|w| w.id != window_id);
        app.refresh_windows().unwrap();
        assert_eq!(app.windows().len(), 3);
        assert!(!app.windows().iter().any(|w| w.id == window_id));
    }

    #[test]
    fn test_external_tmux_change_syncs_selection() {
        let (mut app, tmux) = test_app();
        app.set_pane_active(false);
        assert_eq!(app.current_selected(), 0);

        tmux.windows.lock().unwrap()[1].is_active = false;
        tmux.windows.lock().unwrap()[3].is_active = true;
        app.refresh_windows().unwrap();
        assert_eq!(app.current_selected(), 1);
    }

    #[test]
    fn test_external_tmux_change_switches_tab() {
        let (mut app, tmux) = test_app();
        app.set_pane_active(false);
        assert_eq!(app.active_tab(), Tab::Agents);

        tmux.windows.lock().unwrap()[0].is_active = true;
        app.refresh_windows().unwrap();
        assert_eq!(app.active_tab(), Tab::Windows);
    }

    #[test]
    fn test_external_tmux_change_syncs_after_navigation() {
        let (mut app, tmux) = test_app();
        app.set_pane_active(false);
        assert_eq!(app.current_selected(), 0);

        app.navigate_down();
        assert_eq!(app.current_selected(), 1);
        assert_eq!(app.last_focused_id, Some(4));

        tmux.windows.lock().unwrap()[0].is_active = true;
        app.refresh_windows().unwrap();
        assert_eq!(app.active_tab(), Tab::Windows);
        assert_eq!(app.current_selected(), 0);
        assert_eq!(app.last_focused_id, Some(1));
    }

    #[test]
    fn test_refresh_windows_does_not_override_selection_when_pane_active() {
        let (mut app, tmux) = test_app();
        assert_eq!(app.current_selected(), 0);

        app.navigate_down();
        assert_eq!(app.current_selected(), 1);

        tmux.windows.lock().unwrap()[0].is_active = true;
        app.refresh_windows().unwrap();
        assert_eq!(app.current_selected(), 1);
    }

    #[test]
    fn test_selected_window_moving_to_other_tab_switches_tab() {
        let (mut app, tmux) = test_app();
        assert_eq!(app.active_tab(), Tab::Agents);
        assert_eq!(app.current_tab_len(), 2);

        let selected_id = app.current_tab_window().unwrap().id;
        assert_eq!(selected_id, 2);

        tmux.windows.lock().unwrap()[1].running_command = "bash".to_string();
        app.refresh_windows().unwrap();

        assert_eq!(app.active_tab(), Tab::Windows);
        assert_eq!(app.current_tab_window().unwrap().id, selected_id);
    }

    #[test]
    fn test_switch_tab() {
        let (mut app, _) = test_app();
        assert_eq!(app.active_tab(), Tab::Agents);
        app.switch_tab(Tab::Windows);
        assert_eq!(app.active_tab(), Tab::Windows);
        app.switch_tab(Tab::Agents);
        assert_eq!(app.active_tab(), Tab::Agents);
    }

    #[test]
    fn test_switch_tab_blocked_when_empty() {
        let driver = session();
        driver.windows.lock().unwrap()[1].running_command = "bash".to_string();
        driver.windows.lock().unwrap()[3].running_command = "zsh".to_string();
        let mut app = new_app(&driver, &session(), None, None);
        assert_eq!(app.active_tab(), Tab::Windows);
        assert!(app.is_tab_empty(Tab::Agents));
        app.switch_tab(Tab::Agents);
        assert_eq!(app.active_tab(), Tab::Windows);
    }

    #[test]
    fn test_switch_tab_selects_first_when_no_match() {
        let (mut app, _) = test_app();

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
        let driver = session();
        driver.windows.lock().unwrap()[1].current_dir = "/home/user/shared".to_string();
        driver.windows.lock().unwrap()[2].current_dir = "/home/user/shared".to_string();
        let mut app = new_app(&driver, &session(), None, None);

        assert_eq!(app.active_tab(), Tab::Agents);
        assert_eq!(app.current_selected(), 0);

        app.switch_tab(Tab::Windows);
        assert_eq!(app.current_selected(), 1);
    }

    #[test]
    fn test_switch_tab_selects_first_match_when_multiple() {
        let driver = session();
        driver.windows.lock().unwrap()[1].current_dir = "/home/user/shared".to_string();
        driver.windows.lock().unwrap()[0].current_dir = "/home/user/shared".to_string();
        driver.windows.lock().unwrap()[2].current_dir = "/home/user/shared".to_string();
        let mut app = new_app(&driver, &session(), None, None);

        assert_eq!(app.active_tab(), Tab::Agents);
        assert_eq!(app.current_selected(), 0);

        app.switch_tab(Tab::Windows);
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_switch_tab_selects_first_when_current_dir_empty() {
        let driver = session();
        driver.windows.lock().unwrap()[0].current_dir = String::new();
        driver.windows.lock().unwrap()[2].current_dir = "/home/user/project3".to_string();
        let mut app = new_app(&driver, &session(), None, None);

        assert_eq!(app.active_tab(), Tab::Agents);
        assert_eq!(app.current_selected(), 0);

        app.switch_tab(Tab::Windows);
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_is_tab_empty() {
        let (app, _) = test_app();
        assert!(!app.is_tab_empty(Tab::Agents));
        assert!(!app.is_tab_empty(Tab::Windows));
    }

    #[test]
    fn test_current_tab_windows() {
        let (app, _) = test_app();
        let agent_windows = app.current_tab_windows();
        assert_eq!(agent_windows.len(), 2);
        assert!(is_agent(&agent_windows[0].running_command).is_some());
        assert!(is_agent(&agent_windows[1].running_command).is_some());
    }

    #[test]
    fn test_ensure_visible_no_change_when_already_visible() {
        let (mut app, _) = test_app();
        assert_eq!(app.list_state().offset(), 0);

        app.ensure_visible(3);
        assert_eq!(app.list_state().offset(), 0);
    }

    #[test]
    fn test_ensure_visible_scrolls_down_when_selected_below_visible() {
        let (mut app, _) = test_app();
        app.navigate_down();
        assert_eq!(app.current_selected(), 1);

        app.ensure_visible(1);
        assert_eq!(app.list_state().offset(), 1);
    }

    #[test]
    fn test_ensure_visible_scrolls_up_when_selected_above_visible() {
        let (mut app, _) = test_app();
        app.navigate_down();
        *app.list_state.offset_mut() = 1;

        app.navigate_up();
        assert_eq!(app.current_selected(), 0);

        app.ensure_visible(1);
        assert_eq!(app.list_state().offset(), 0);
    }

    #[test]
    fn test_ensure_visible_no_scroll_when_zero_visible_count() {
        let (mut app, _) = test_app();
        app.navigate_down();

        app.ensure_visible(0);
        assert_eq!(app.list_state().offset(), 0);
    }

    #[test]
    fn test_cursor_follows_active_window_after_navigation_then_lose_focus() {
        let (mut app, tmux) = test_app();

        app.navigate_down();
        assert_eq!(app.current_selected(), 1);
        assert_eq!(app.last_focused_id, Some(4));

        app.set_pane_active(false);

        tmux.windows.lock().unwrap()[0].is_active = true;
        app.refresh_windows().unwrap();

        assert_eq!(app.active_tab(), Tab::Windows);
        assert_eq!(app.current_selected(), 0);
    }

    #[test]
    fn test_cursor_follows_active_window_after_mouse_hover_then_lose_focus() {
        // Scenario: User focuses window with keyboard, then hovers with mouse
        // (changing selection but not last_focused_id), then pane becomes inactive.
        // Selection should sync back to the active window, not stay at hover position.
        let (mut app, tmux) = test_app();

        // Initial state: Agents tab has 2 windows (id=2 at index 0, id=4 at index 1)
        // Active window is id=2
        assert_eq!(app.current_selected(), 0);
        assert_eq!(app.windows()[1].id, 2);
        assert!(app.windows()[1].is_active);

        // User navigates down with keyboard to selection 1 (id=4)
        app.navigate_down();
        assert_eq!(app.current_selected(), 1);
        assert_eq!(app.last_focused_id, Some(4));

        // Simulate that the active window is now id=4 (the one we focused)
        tmux.windows.lock().unwrap()[1].is_active = false; // id=2 is no longer active
        tmux.windows.lock().unwrap()[3].is_active = true; // id=4 is now active

        // User hovers back to selection 0 (id=2) with mouse
        app.set_current_selected(0);
        assert_eq!(app.current_selected(), 0);
        assert_eq!(app.last_focused_id, Some(4)); // Still points to id=4

        // Pane becomes inactive (mouse moved to main pane)
        app.set_pane_active(false);

        // refresh_windows should sync selection back to active window (id=4, index 1)
        app.refresh_windows().unwrap();

        assert_eq!(app.current_selected(), 1); // Should sync back to active window (id=4)
    }
}

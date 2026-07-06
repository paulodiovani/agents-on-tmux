use crossterm::event::{KeyCode, KeyEvent};

/// Tab categories for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Agents,
    Windows,
}

impl Tab {
    pub fn index(self) -> usize {
        match self {
            Tab::Agents => 0,
            Tab::Windows => 1,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tab::Agents => "Agents",
            Tab::Windows => "Windows",
        }
    }

    pub fn left(self) -> Self {
        match self {
            Tab::Windows => Tab::Agents,
            Tab::Agents => Tab::Agents,
        }
    }

    pub fn right(self) -> Self {
        match self {
            Tab::Agents => Tab::Windows,
            Tab::Windows => Tab::Windows,
        }
    }
}

/// User actions that can be triggered by keyboard input.
pub enum Action {
    Quit,
    NavigateUp,
    NavigateDown,
    FocusWindow,
    CreateWindow,
    KillWindow,
    SwitchTabLeft,
    SwitchTabRight,
    None,
}

/// Actions that require double-press confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingAction {
    KillWindow,
    Quit,
}

/// Maps a key event to the corresponding application action.
pub fn key_to_action(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Up | KeyCode::Char('k') => Action::NavigateUp,
        KeyCode::Down | KeyCode::Char('j') => Action::NavigateDown,
        KeyCode::Enter => Action::FocusWindow,
        KeyCode::Char('n') => Action::CreateWindow,
        KeyCode::Char('d') => Action::KillWindow,
        KeyCode::Left | KeyCode::Char('h') => Action::SwitchTabLeft,
        KeyCode::Right | KeyCode::Char('l') => Action::SwitchTabRight,
        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key_event(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_quit_keys() {
        assert!(matches!(
            key_to_action(key_event(KeyCode::Char('q'))),
            Action::Quit
        ));
    }

    #[test]
    fn test_navigate_up_keys() {
        assert!(matches!(
            key_to_action(key_event(KeyCode::Up)),
            Action::NavigateUp
        ));
        assert!(matches!(
            key_to_action(key_event(KeyCode::Char('k'))),
            Action::NavigateUp
        ));
    }

    #[test]
    fn test_navigate_down_keys() {
        assert!(matches!(
            key_to_action(key_event(KeyCode::Down)),
            Action::NavigateDown
        ));
        assert!(matches!(
            key_to_action(key_event(KeyCode::Char('j'))),
            Action::NavigateDown
        ));
    }

    #[test]
    fn test_focus_window() {
        assert!(matches!(
            key_to_action(key_event(KeyCode::Enter)),
            Action::FocusWindow
        ));
    }

    #[test]
    fn test_create_window() {
        assert!(matches!(
            key_to_action(key_event(KeyCode::Char('n'))),
            Action::CreateWindow
        ));
    }

    #[test]
    fn test_kill_window() {
        assert!(matches!(
            key_to_action(key_event(KeyCode::Char('d'))),
            Action::KillWindow
        ));
    }

    #[test]
    fn test_none_action() {
        assert!(matches!(
            key_to_action(key_event(KeyCode::Char('x'))),
            Action::None
        ));
    }

    #[test]
    fn test_switch_tab_left_keys() {
        assert!(matches!(
            key_to_action(key_event(KeyCode::Left)),
            Action::SwitchTabLeft
        ));
        assert!(matches!(
            key_to_action(key_event(KeyCode::Char('h'))),
            Action::SwitchTabLeft
        ));
    }

    #[test]
    fn test_switch_tab_right_keys() {
        assert!(matches!(
            key_to_action(key_event(KeyCode::Right)),
            Action::SwitchTabRight
        ));
        assert!(matches!(
            key_to_action(key_event(KeyCode::Char('l'))),
            Action::SwitchTabRight
        ));
    }

    #[test]
    fn test_tab_index() {
        assert_eq!(Tab::Agents.index(), 0);
        assert_eq!(Tab::Windows.index(), 1);
    }

    #[test]
    fn test_tab_title() {
        assert_eq!(Tab::Agents.title(), "Agents");
        assert_eq!(Tab::Windows.title(), "Windows");
    }

    #[test]
    fn test_tab_left() {
        assert_eq!(Tab::Windows.left(), Tab::Agents);
        assert_eq!(Tab::Agents.left(), Tab::Agents);
    }

    #[test]
    fn test_tab_right() {
        assert_eq!(Tab::Agents.right(), Tab::Windows);
        assert_eq!(Tab::Windows.right(), Tab::Windows);
    }
}

use crossterm::event::{KeyCode, KeyEvent};

pub enum Action {
    Quit,
    NavigateUp,
    NavigateDown,
    FocusWindow,
    CreateWindow,
    KillWindow,
    None,
}

pub fn key_to_action(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
        KeyCode::Up | KeyCode::Char('k') => Action::NavigateUp,
        KeyCode::Down | KeyCode::Char('j') => Action::NavigateDown,
        KeyCode::Enter => Action::FocusWindow,
        KeyCode::Char('n') => Action::CreateWindow,
        KeyCode::Char('d') => Action::KillWindow,
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
        assert!(matches!(
            key_to_action(key_event(KeyCode::Esc)),
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
}

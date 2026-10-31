use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

/// Semantic interpretation of a mouse event (click, hover, or scroll).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Click(u16, u16), // x, y
    Hover(u16, u16), // x, y
    Scroll(i8),
    None,
}

impl From<MouseEvent> for MouseAction {
    fn from(mouse: MouseEvent) -> Self {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => MouseAction::Click(mouse.column, mouse.row),
            MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                MouseAction::Hover(mouse.column, mouse.row)
            }
            MouseEventKind::ScrollUp => MouseAction::Scroll(-1),
            MouseEventKind::ScrollDown => MouseAction::Scroll(1),
            _ => MouseAction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }
    }

    // ── MouseAction::from ─────────────────────────────────────────────

    #[test]
    fn left_click_becomes_click() {
        let event = mouse_event(MouseEventKind::Down(MouseButton::Left), 7, 5);
        assert_eq!(MouseAction::from(event), MouseAction::Click(7, 5));
    }

    #[test]
    fn right_click_becomes_none() {
        let event = mouse_event(MouseEventKind::Down(MouseButton::Right), 7, 5);
        assert_eq!(MouseAction::from(event), MouseAction::None);
    }

    #[test]
    fn middle_click_becomes_none() {
        let event = mouse_event(MouseEventKind::Down(MouseButton::Middle), 3, 3);
        assert_eq!(MouseAction::from(event), MouseAction::None);
    }

    #[test]
    fn moved_becomes_hover() {
        let event = mouse_event(MouseEventKind::Moved, 4, 2);
        assert_eq!(MouseAction::from(event), MouseAction::Hover(4, 2));
    }

    #[test]
    fn drag_becomes_hover() {
        let event = mouse_event(MouseEventKind::Drag(MouseButton::Left), 9, 1);
        assert_eq!(MouseAction::from(event), MouseAction::Hover(9, 1));
    }

    #[test]
    fn scroll_up_becomes_scroll_negative() {
        let event = mouse_event(MouseEventKind::ScrollUp, 0, 0);
        assert_eq!(MouseAction::from(event), MouseAction::Scroll(-1));
    }

    #[test]
    fn scroll_down_becomes_scroll_positive() {
        let event = mouse_event(MouseEventKind::ScrollDown, 0, 0);
        assert_eq!(MouseAction::from(event), MouseAction::Scroll(1));
    }

    #[test]
    fn button_up_becomes_none() {
        let event = mouse_event(MouseEventKind::Up(MouseButton::Left), 5, 5);
        assert_eq!(MouseAction::from(event), MouseAction::None);
    }
}

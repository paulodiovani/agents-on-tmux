use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

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

/// Checks whether a point (column, row) is within the given rectangle.
pub fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.x + area.width && row >= area.y && row < area.y + area.height
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

    fn rect(x: u16, y: u16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
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

    // ── contains ──────────────────────────────────────────────────────

    #[test]
    fn contains_inside_returns_true() {
        assert!(contains(rect(5, 3, 10, 4), 7, 5));
    }

    #[test]
    fn contains_outside_returns_false() {
        assert!(!contains(rect(5, 3, 10, 4), 20, 5));
    }

    #[test]
    fn contains_top_left_corner_returns_true() {
        assert!(contains(rect(5, 3, 10, 4), 5, 3));
    }

    #[test]
    fn contains_past_right_edge_returns_false() {
        assert!(!contains(rect(5, 3, 10, 4), 15, 5));
    }

    #[test]
    fn contains_past_bottom_edge_returns_false() {
        assert!(!contains(rect(5, 3, 10, 4), 7, 7));
    }

    #[test]
    fn contains_bottom_right_corner_returns_false() {
        // x + width and y + height are exclusive bounds
        assert!(!contains(rect(5, 3, 10, 4), 15, 7));
    }

    #[test]
    fn contains_bottom_right_inside_returns_true() {
        assert!(contains(rect(5, 3, 10, 4), 14, 6));
    }

    // ── Click + contains (integration) ────────────────────────────────

    #[test]
    fn click_inside_rect_can_be_found() {
        let r = rect(0, 0, 10, 3);
        let event = mouse_event(MouseEventKind::Down(MouseButton::Left), 5, 1);
        if let MouseAction::Click(x, y) = MouseAction::from(event) {
            assert!(contains(r, x, y));
        } else {
            panic!("expected Click");
        }
    }

    #[test]
    fn click_outside_rect_is_not_found() {
        let r = rect(0, 0, 10, 3);
        let event = mouse_event(MouseEventKind::Down(MouseButton::Left), 15, 1);
        if let MouseAction::Click(x, y) = MouseAction::from(event) {
            assert!(!contains(r, x, y));
        } else {
            panic!("expected Click");
        }
    }
}

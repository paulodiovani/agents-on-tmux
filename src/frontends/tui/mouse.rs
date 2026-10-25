use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

/// Checks whether a mouse event is a left-click within the given rectangle.
/// Returns true only for left-button presses inside the area; other buttons,
/// drags, scrolls, and releases return false.
pub fn is_clicked(mouse: MouseEvent, area: Rect) -> bool {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            mouse.column >= area.x
                && mouse.column < area.x + area.width
                && mouse.row >= area.y
                && mouse.row < area.y + area.height
        }
        _ => false,
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

    fn rect(x: u16, y: u16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn left_click_inside_rect_returns_true() {
        let r = rect(5, 3, 10, 4);
        assert!(is_clicked(
            mouse_event(MouseEventKind::Down(MouseButton::Left), 7, 5),
            r,
        ));
    }

    #[test]
    fn left_click_outside_rect_returns_false() {
        let r = rect(5, 3, 10, 4);
        assert!(!is_clicked(
            mouse_event(MouseEventKind::Down(MouseButton::Left), 20, 5),
            r,
        ));
    }

    #[test]
    fn right_click_inside_rect_returns_false() {
        let r = rect(5, 3, 10, 4);
        assert!(!is_clicked(
            mouse_event(MouseEventKind::Down(MouseButton::Right), 7, 5),
            r,
        ));
    }

    #[test]
    fn click_on_exact_boundary_returns_true() {
        let r = rect(5, 3, 10, 4);
        // Top-left corner
        assert!(is_clicked(
            mouse_event(MouseEventKind::Down(MouseButton::Left), 5, 3),
            r,
        ));
    }

    #[test]
    fn click_just_past_boundary_returns_false() {
        let r = rect(5, 3, 10, 4);
        // Just past right edge (x + width = 15)
        assert!(!is_clicked(
            mouse_event(MouseEventKind::Down(MouseButton::Left), 15, 5),
            r,
        ));
        // Just past bottom edge (y + height = 7)
        assert!(!is_clicked(
            mouse_event(MouseEventKind::Down(MouseButton::Left), 7, 7),
            r,
        ));
    }

    #[test]
    fn drag_returns_false() {
        let r = rect(0, 0, 10, 10);
        assert!(!is_clicked(
            mouse_event(MouseEventKind::Drag(MouseButton::Left), 5, 5),
            r,
        ));
    }

    #[test]
    fn scroll_returns_false() {
        let r = rect(0, 0, 10, 10);
        assert!(!is_clicked(
            mouse_event(MouseEventKind::ScrollDown, 5, 5),
            r
        ));
    }

    #[test]
    fn move_returns_false() {
        let r = rect(0, 0, 10, 10);
        assert!(!is_clicked(mouse_event(MouseEventKind::Moved, 5, 5), r));
    }

    #[test]
    fn button_up_returns_false() {
        let r = rect(0, 0, 10, 10);
        assert!(!is_clicked(
            mouse_event(MouseEventKind::Up(MouseButton::Left), 5, 5),
            r,
        ));
    }
}

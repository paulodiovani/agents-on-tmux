use std::time::{Duration, Instant};

use super::App;
use crate::backends::process::MockProcesses;
use crate::backends::tmux::Window;
use crate::frontends::tui::fake_tmux::FakeTmux;

/// An app over the given sessions whose process lookups find nothing.
pub fn new_app(
    nested: &FakeTmux,
    parent: &FakeTmux,
    pane_id: Option<&str>,
    panel_width: Option<u16>,
) -> App {
    let mut processes = MockProcesses::new();
    processes.expect_started_at().returning(|_| None);
    App::new(
        Box::new(nested.mock()),
        Box::new(parent.mock()),
        Box::new(processes),
        pane_id.map(str::to_string),
        panel_width,
    )
    .unwrap()
}

/// An app over `session()`, returned with its nested session.
pub fn test_app() -> (App, FakeTmux) {
    let tmux = session();
    let app = new_app(&tmux, &session(), None, None);
    (app, tmux)
}

/// Four windows: two agents (ids 2 and 4, the first active) and two shells.
pub fn session() -> FakeTmux {
    FakeTmux::with_windows(vec![
        Window {
            id: 1,
            name: "agent-1".to_string(),
            pane_pid: 10001,
            running_command: "cargo build".to_string(),
            current_dir: "/home/user/project1".to_string(),
            is_active: false,
            notification_pending: false,
            started_at: Some(Instant::now() - Duration::from_secs(125)),
        },
        Window {
            id: 2,
            name: "agent-2".to_string(),
            pane_pid: 10002,
            running_command: "claude".to_string(),
            current_dir: "/home/user/project2".to_string(),
            is_active: true,
            notification_pending: true,
            started_at: Some(Instant::now() - Duration::from_secs(45)),
        },
        Window {
            id: 3,
            name: "agent-3".to_string(),
            pane_pid: 10003,
            running_command: "python main.py".to_string(),
            current_dir: "/home/user/project3".to_string(),
            is_active: false,
            notification_pending: false,
            started_at: Some(Instant::now() - Duration::from_secs(300)),
        },
        Window {
            id: 4,
            name: "agent-4".to_string(),
            pane_pid: 10004,
            running_command: "opencode".to_string(),
            current_dir: "/home/user/project4".to_string(),
            is_active: false,
            notification_pending: false,
            started_at: Some(Instant::now() - Duration::from_secs(10)),
        },
    ])
}

# tmux Control Mode Integration

**Date:** 2026-07-24  
**Status:** Planned  
**Goal:** Replace 5-second polling with event-driven updates using tmux control mode

## Overview

Current implementation polls `tmux list-windows` every 5 seconds to detect state changes. This plan replaces polling with event-driven updates using tmux's control mode (`tmux -C`), which provides real-time notifications for window and session changes.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│              App (synchronous, event-driven)             │
│  - Main loop polls keyboard + tmux events               │
│  - Redraws only on events (no polling)                  │
└────────┬───────────────────┬────────────────────────────┘
         │                   │
    ┌────▼────┐         ┌────▼─────────────────┐
    │Keyboard │         │  ControlModeThread   │
    │ Poll    │         │  (std::thread)       │
    │(100ms)  │         │  - Reads tmux -C     │
    └─────────┘         │  - Parses events     │
                        │  - Sends to channel  │
                        │  - Reconnects on fail│
                        └──────────────────────┘
```

### Key Design Decisions

1. **Synchronous implementation** — Use `std::thread` + `std::sync::mpsc` channel (no async runtime needed)
2. **Control mode thread** — Runs in background, sends events to main thread via channel
3. **Non-blocking channel reads** — Use `try_recv()` to avoid blocking the main loop
4. **Reconnection logic** — Exponential backoff (1s, 2s, 4s, 8s, 16s), exit after 5 failures
5. **Event-driven redraw** — Redraw only on events, no time-based refresh
6. **Session lifecycle** — tmux session persists independently; killing the app does NOT kill the session

## File Structure

```
src/
├── backends/
│   ├── control_mode.rs    (NEW - control mode client)
│   ├── tmux.rs            (MODIFY - add session_name() method)
│   └── mod.rs             (MODIFY - export control_mode)
├── frontends/
│   └── tui/
│       └── app.rs         (MODIFY - integrate event-driven updates)
└── main.rs                (MODIFY - add debug flag, init tracing)
```

## Dependencies

```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = "0.3"
```

---

## Phase 1: Control Mode Module (TDD)

### Step 1.1: Event Parsing

**Tests to write first:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_window_add() {
        let event = parse_event("%window-add @3");
        assert_eq!(event, Some(TmuxEvent::WindowAdd { id: 3 }));
    }

    #[test]
    fn test_parse_window_close() {
        let event = parse_event("%window-close @5");
        assert_eq!(event, Some(TmuxEvent::WindowClose { id: 5 }));
    }

    #[test]
    fn test_parse_window_renamed() {
        let event = parse_event("%window-renamed @3 tmux");
        assert_eq!(event, Some(TmuxEvent::WindowRenamed { 
            id: 3, 
            name: "tmux".to_string() 
        }));
    }

    #[test]
    fn test_parse_session_changed() {
        let event = parse_event("%session-changed $1 1");
        assert_eq!(event, Some(TmuxEvent::SessionChanged { 
            session_id: 1, 
            window_id: 1 
        }));
    }

    #[test]
    fn test_parse_exit() {
        let event = parse_event("%exit");
        assert_eq!(event, Some(TmuxEvent::Exit));
    }

    #[test]
    fn test_parse_invalid_line() {
        assert_eq!(parse_event("invalid"), None);
        assert_eq!(parse_event(""), None);
        assert_eq!(parse_event("%unknown"), None);
    }
}
```

**Implementation:**

```rust
pub enum TmuxEvent {
    WindowAdd { id: u32 },
    WindowClose { id: u32 },
    WindowRenamed { id: u32, name: String },
    SessionChanged { session_id: u32, window_id: u32 },
    Exit,
}

pub fn parse_event(line: &str) -> Option<TmuxEvent> {
    // Parse tmux control mode output
    // Return None for unrecognized lines
}
```

### Step 1.2: Session Name Access

**Tests to write first:**

```rust
#[test]
fn test_session_name() {
    let driver = TmuxDriver::new("test-session");
    assert_eq!(driver.session_name(), "test-session");
}
```

**Implementation:**

Add to `Tmux` trait:

```rust
fn session_name(&self) -> &str;
```

Implement in `TmuxDriver`:

```rust
fn session_name(&self) -> &str {
    &self.session
}
```

### Step 1.3: Control Mode Thread

**Tests to write first:**

```rust
#[test]
fn test_control_mode_sends_events() {
    let (tx, rx) = mpsc::channel();
    // Mock: spawn thread that sends a few events then exits
    // Verify events are received
}

#[test]
fn test_control_mode_reconnects_on_failure() {
    let (tx, rx) = mpsc::channel();
    // Mock: fail first 2 times, succeed on 3rd
    // Verify retry delays and eventual success
}

#[test]
fn test_control_mode_exits_after_max_retries() {
    let (tx, rx) = mpsc::channel();
    // Mock: always fail
    // Verify TmuxEvent::Exit sent after 5 attempts
}
```

**Implementation:**

```rust
pub fn control_mode_thread(
    session: String,
    event_tx: mpsc::Sender<TmuxEvent>,
) {
    let mut retry_count = 0;
    const MAX_RETRIES: usize = 5;
    
    loop {
        match spawn_control_mode(&session, &event_tx) {
            Ok(()) => break,
            Err(e) => {
                tracing::debug!("Control mode connection failed: {}", e);
                retry_count += 1;
                
                if retry_count >= MAX_RETRIES {
                    tracing::error!("Max reconnection attempts reached");
                    let _ = event_tx.send(TmuxEvent::Exit);
                    break;
                }
                
                let delay = Duration::from_secs(2u64.pow(retry_count as u32 - 1));
                tracing::debug!("Reconnecting in {:?} (attempt {}/{})", 
                    delay, retry_count, MAX_RETRIES);
                std::thread::sleep(delay);
            }
        }
    }
}

fn spawn_control_mode(
    session: &str,
    event_tx: &mpsc::Sender<TmuxEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Spawn tmux -C attach -t <session>
    // Read stdout line by line
    // Parse and send events
}
```

---

## Phase 2: App Integration (TDD)

### Step 2.1: App Structure

**Tests to write first:**

```rust
#[test]
fn test_app_new_has_no_event_receiver() {
    let app = App::new(Box::new(MockTmux::new()), Box::new(MockTmux::new())).unwrap();
    assert!(app.event_rx.is_none());
}
```

**Implementation:**

Add to `App` struct:

```rust
event_rx: Option<mpsc::Receiver<TmuxEvent>>,
```

Update `App::new()`:

```rust
event_rx: None,
```

### Step 2.2: Event Loop

**Tests to write first:**

```rust
#[test]
fn test_window_add_triggers_refresh() {
    let (mut app, windows, _) = test_app();
    let (tx, rx) = mpsc::channel();
    app.event_rx = Some(rx);
    
    // Simulate WindowAdd event
    let _ = tx.send(TmuxEvent::WindowAdd { id: 99 });
    
    // Add a window to mock
    windows.borrow_mut().push(Window { /* ... */ });
    
    // Process events
    app.process_tmux_events();
    
    // Verify refresh happened (window count increased)
    assert_eq!(app.windows().len(), 5);
}

#[test]
fn test_exit_event_quits_app() {
    let (mut app, _, _) = test_app();
    let (tx, rx) = mpsc::channel();
    app.event_rx = Some(rx);
    
    let _ = tx.send(TmuxEvent::Exit);
    app.process_tmux_events();
    
    assert!(!app.running);
}
```

**Implementation:**

Add method to `App`:

```rust
fn process_tmux_events(&mut self) {
    if let Some(rx) = &self.event_rx {
        while let Ok(event) = rx.try_recv() {
            match event {
                TmuxEvent::WindowAdd { .. } |
                TmuxEvent::WindowClose { .. } |
                TmuxEvent::WindowRenamed { .. } |
                TmuxEvent::SessionChanged { .. } => {
                    let _ = self.refresh_windows();
                }
                TmuxEvent::Exit => {
                    self.running = false;
                }
            }
        }
    }
}
```

Modify `App::run()`:

```rust
pub fn run(&mut self, mut terminal: DefaultTerminal) -> anyhow::Result<()> {
    let theme = Theme::default();
    
    // Spawn control mode thread
    let (event_tx, event_rx) = mpsc::channel();
    let session = self.nested_driver.session_name().to_string();
    std::thread::spawn(move || {
        crate::backends::control_mode::control_mode_thread(session, event_tx);
    });
    self.event_rx = Some(event_rx);
    
    while self.running {
        terminal.draw(|frame| ui::draw(frame, self, &theme))?;
        
        // Poll keyboard events (100ms timeout)
        if event::poll(Duration::from_millis(100))? {
            if let event::Event::Key(key) = event::read()? {
                if key.kind == event::KeyEventKind::Press {
                    self.handle_action(key_to_action(key));
                }
            }
        }
        
        // Process tmux events (non-blocking)
        self.process_tmux_events();
    }
    
    Ok(())
}
```

### Step 2.3: Remove Polling

**Remove:**

- `REFRESH_INTERVAL_SECS` constant
- `last_draw` variable
- `should_redraw` logic
- Time-based refresh in `run()`

**Update tests:**

- Remove tests that verify time-based refresh
- Keep tests that verify event-driven refresh

---

## Phase 3: Main Function Updates (TDD)

### Step 3.1: Debug Flag

**Tests to write first:**

```rust
#[test]
fn test_debug_flag() {
    let cli = Cli::parse_from(["aot", "--debug"]);
    assert_eq!(cli.debug, Some(true));
}

#[test]
fn test_debug_env_var() {
    unsafe { std::env::set_var("AOT_DEBUG", "1") };
    let cli = Cli::parse_from(["aot"]);
    let config = Config::from(&cli);
    assert_eq!(config.debug, Some(true));
    unsafe { std::env::remove_var("AOT_DEBUG") };
}
```

**Implementation:**

Add to `Cli`:

```rust
#[arg(long, env = "AOT_DEBUG", value_parser = parse_bool, default_missing_value = "true", num_args = 0..=1, require_equals = true)]
debug: Option<bool>,
```

Add to `Config`:

```rust
pub debug: Option<bool>,
```

### Step 3.2: Tracing Initialization

**Tests to write first:**

```rust
#[test]
fn test_tracing_initialized_with_debug() {
    // This is hard to test directly, so we'll just verify the flag is passed
    let config = Config { debug: Some(true), /* ... */ };
    assert_eq!(config.debug, Some(true));
}
```

**Implementation:**

In `main()`:

```rust
if config.debug.unwrap_or(false) {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();
}
```

### Step 3.3: Wire Everything

**No new tests needed** — integration is tested by existing app tests.

**Implementation:**

Update `main()` to pass config to app (already done).

---

## Phase 4: Verification

1. **Format check:**

   ```bash
   cargo fmt --check
   ```

2. **Lint check:**

   ```bash
   cargo clippy -- -D warnings
   ```

3. **Run tests:**

   ```bash
   cargo test
   ```

4. **Manual testing:**

   - Start app: `cargo run -- --tui --debug`
   - Create window: verify immediate update
   - Kill window: verify immediate update
   - Rename window: verify immediate update
   - Switch window externally: verify immediate update
   - Kill tmux server: verify app exits after 5 retries

---

## Execution Order

1. **Phase 1.1**: Event parsing (tests → implementation)
2. **Phase 1.2**: Session name access (tests → implementation)
3. **Phase 1.3**: Control mode thread (tests → implementation)
4. **Phase 2.1**: App structure (tests → implementation)
5. **Phase 2.2**: Event loop (tests → implementation)
6. **Phase 2.3**: Remove polling (update tests → remove code)
7. **Phase 3.1**: Debug flag (tests → implementation)
8. **Phase 3.2**: Tracing initialization (tests → implementation)
9. **Phase 3.3**: Wire everything
10. **Phase 4**: Verification

---

## tmux Control Mode Protocol Reference

Based on testing `tmux -C` output:

| Event | Format | Example |
|-------|--------|---------|
| Window added | `%window-add @<id>` | `%window-add @3` |
| Window closed | `%window-close @<id>` | `%window-close @5` |
| Window renamed | `%window-renamed @<id> <name>` | `%window-renamed @3 tmux` |
| Session changed | `%session-changed $<session_id> <window_id>` | `%session-changed $1 1` |
| Exit | `%exit` | `%exit` |

Note: Window IDs use `@` prefix, session IDs use `$` prefix.

---

## Notes

- **Session lifecycle**: The tmux session persists independently of the app. Killing the app does NOT kill the session.
- **Reconnection**: If control mode connection drops, the app attempts to reconnect with exponential backoff.
- **Debug logging**: Use `--debug` flag or `AOT_DEBUG=1` env var to enable debug logging.
- **No async runtime**: Using threads and channels keeps the implementation simple and avoids adding tokio as a dependency.

---

## Cleanup

**When implementation is complete and verified, delete this plan file:**

```bash
rm -f docs/plans/2026-07-24-tmux-control-mode.md
```

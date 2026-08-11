# tmux Control Mode Integration

**Date:** 2026-07-24  
**Status:** Planned  
**Goal:** Replace 5-second polling with event-driven updates using tmux control mode

## Progress

Execute top to bottom (tests → implementation for each). Check off as completed.

- [x] **Phase 1.1** — Event parsing (`TmuxEvent` = `Refresh` | `Exit`; `%output` &
      `%window-pane-changed` map to `Refresh`)
- [x] **Phase 1.2** — `session_name()` on `Tmux` trait (+ `MockTmux`; rename the new test)
- [x] **Phase 1.3** — Control-mode thread (`pump_events` + `run_with_reconnect`, injectable
      connect/backoff; stdin-piped child)
- [x] **Phase 1.3b** — `window-size=largest` on session create (`tmux.rs`)
- [x] **Phase 2.1** — App structure (`event_rx` field)
- [x] **Phase 2.2** — Event loop (drain-then-refresh, coalesced; 1s redraw tick)
- [x] **Phase 2.3** — Remove the 5s data poll (keep the redraw tick)
- [x] **Phase 3.1** — Debug flag (thread through `Cli`, `Config`, `merge`, `From`, `Display`)
- [x] **Phase 3.2** — Logger init to a **file** (not stdout; std-only `src/logger.rs`)
- [x] **Phase 3.3** — Wire everything
- [x] **Phase 4** — Verification (`fmt` → `clippy` → `test` → manual)
- [ ] **Cleanup** — delete this plan file once verified

## Overview

Current implementation polls `tmux list-windows` every 5 seconds to detect state changes. This plan replaces polling with event-driven updates using tmux's control mode (`tmux -C`), which provides real-time notifications for window and session changes.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│              App (synchronous, event-driven)             │
│  - Main loop: keyboard poll + drain tmux events          │
│  - Data refresh (list_windows) is event-driven           │
│  - UI redraw stays on a ~1s cadence (uptime timer)       │
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
5. **Separate refresh from redraw** — The expensive tmux call (`list_windows`) becomes
   event-driven, replacing the 5s poll. But the UI still shows a live uptime counter
   (`format_elapsed` in `ui.rs`), so the terminal must keep redrawing on a small cadence
   (~1s). "Event-driven" applies to *data refresh*, not to *painting the screen*.
6. **Activity flag coverage** — The app's core signal is `notification_pending`
   (tmux `window_activity_flag`). Control mode has **no** `%window-activity` notification,
   so structural events alone (add/close/rename/session-changed) will not keep the
   notification indicator live. Pane output (`%output`) and pane changes
   (`%window-pane-changed`) MUST also trigger a (debounced) refresh — otherwise this
   plan silently regresses the app's most important feature. See Phase 1.1.
7. **Session lifecycle** — tmux session persists independently; killing the app does NOT kill the session
8. **Control-mode client sizing** — A `tmux -C attach` client joins the session as a real
   client. With the default `window-size latest`, its small (80x24) size can shrink the
   user's attached view. The session must use `window-size largest` (see Phase 1.3).

## File Structure

```
src/
├── backends/
│   ├── control_mode.rs    (NEW - control mode client)
│   ├── tmux.rs            (MODIFY - add session_name() to Tmux trait;
│   │                                set window-size=largest on session create)
│   └── mod.rs             (MODIFY - `pub mod control_mode;`)
├── frontends/
│   └── tui/
│       └── app.rs         (MODIFY - integrate event-driven updates)
├── logger.rs              (NEW - std-only file logger)
└── main.rs                (MODIFY - add debug flag, init logger)
```

## Dependencies

No new crates — every added crate grows the binary. Logging uses the std-only
`src/logger.rs` module (global `OnceLock<Mutex<File>>`, `logger::init` /
`logger::debug` / `logger::error`).

---

## Phase 1: Control Mode Module (TDD)

### Step 1.1: Event Parsing

The app only needs to know *whether something changed* so it can re-run `list_windows`;
it does not need to apply per-window deltas itself. Therefore every relevant notification
collapses into one of two outcomes: **refresh the window list** or **exit**. This keeps the
parser small and, crucially, lets us treat activity-bearing notifications (`%output`,
`%window-pane-changed`) as refresh triggers — without them the `notification_pending`
indicator never updates in real time (see Key Design Decision #6).

**Notes on parsing:**

- Window ids are prefixed with `@` and session ids with `$`; strip the sigil before
  parsing the number, and return `None` if the remainder is not a valid `u32`.
- `%window-renamed @3 some name` — names may contain spaces. Use `splitn(3, ' ')` so the
  whole remainder becomes the name (the name is discarded anyway since we only refresh,
  but the line must still parse without error).
- `%output %5 ...`, `%extended-output ...` and `%window-pane-changed @3` are high-frequency;
  they map to `Refresh` and are coalesced downstream (see Step 2.2), not one refresh each.
- Unrecognized lines, control-block framing (`%begin`/`%end`/`%error`), empty lines, and
  the initial handshake all return `None`.

**Tests to write first:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_window_add() {
        assert_eq!(parse_event("%window-add @3"), Some(TmuxEvent::Refresh));
    }

    #[test]
    fn test_parse_window_close() {
        assert_eq!(parse_event("%window-close @5"), Some(TmuxEvent::Refresh));
    }

    #[test]
    fn test_parse_window_renamed_with_spaces() {
        assert_eq!(parse_event("%window-renamed @3 my window"), Some(TmuxEvent::Refresh));
    }

    #[test]
    fn test_parse_session_changed() {
        assert_eq!(parse_event("%session-changed $1 1"), Some(TmuxEvent::Refresh));
    }

    #[test]
    fn test_parse_output_triggers_refresh() {
        assert_eq!(parse_event("%output %5 hello"), Some(TmuxEvent::Refresh));
    }

    #[test]
    fn test_parse_window_pane_changed_triggers_refresh() {
        assert_eq!(parse_event("%window-pane-changed @3 %6"), Some(TmuxEvent::Refresh));
    }

    #[test]
    fn test_parse_exit() {
        assert_eq!(parse_event("%exit"), Some(TmuxEvent::Exit));
    }

    #[test]
    fn test_parse_invalid_line() {
        assert_eq!(parse_event("invalid"), None);
        assert_eq!(parse_event(""), None);
        assert_eq!(parse_event("%unknown"), None);
        assert_eq!(parse_event("%begin 123 0"), None);
    }

    #[test]
    fn test_parse_malformed_id_is_none() {
        assert_eq!(parse_event("%window-add @notanumber"), None);
    }
}
```

**Implementation:**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxEvent {
    /// Some observable state changed — re-run list_windows.
    Refresh,
    /// The control-mode connection ended or the server exited.
    Exit,
}

pub fn parse_event(line: &str) -> Option<TmuxEvent> {
    let mut parts = line.split(' ');
    match parts.next()? {
        "%exit" => Some(TmuxEvent::Exit),
        // Structural changes: validate the id so malformed lines are ignored.
        "%window-add" | "%window-close" | "%window-renamed" => {
            let id = parts.next()?.strip_prefix('@')?;
            id.parse::<u32>().ok().map(|_| TmuxEvent::Refresh)
        }
        // Activity / focus changes: keep notification_pending live.
        "%session-changed" | "%window-pane-changed" | "%output" | "%extended-output" => {
            Some(TmuxEvent::Refresh)
        }
        _ => None,
    }
}
```

> **Design note:** collapsing all structural + activity notifications into a single
> `Refresh` variant is deliberate — the app already re-derives full window state from
> `list_windows()`, so carrying `id`/`name` payloads would be dead data. If a future
> feature needs per-window deltas, reintroduce typed variants then.

### Step 1.2: Session Name Access

The control-mode thread needs the session name, and `App` only holds `Box<dyn Tmux>`, so
`session_name()` must live on the `Tmux` trait (not just the concrete `TmuxDriver`).

> **Gap:** `tmux.rs` already has a test named `test_session_name` (it asserts the
> `SESSION_NAME` constant). Name the new test `test_driver_session_name` to avoid a
> compile-time duplicate-name collision.

> **Gap:** adding a method to the `Tmux` trait breaks every implementor. Besides
> `TmuxDriver`, the `MockTmux` in `src/frontends/tui/app.rs` tests implements `Tmux` and
> MUST also gain `fn session_name(&self) -> &str { "agents-on-tmux" }`. Place the new trait
> method per the AGENTS.md ordering convention (with the other trait methods).

**Tests to write first:**

```rust
#[test]
fn test_driver_session_name() {
    let driver = TmuxDriver::new("test-session");
    assert_eq!(driver.session_name(), "test-session");
}
```

**Implementation:**

Add to `Tmux` trait:

```rust
/// Returns the session name this driver targets.
fn session_name(&self) -> &str;
```

Implement in `TmuxDriver`:

```rust
fn session_name(&self) -> &str {
    &self.session
}
```

### Step 1.3: Control Mode Thread

**Testability refactor (ambiguity fix):** the original thread tests were pseudocode
("Mock: fail first 2 times…") because `spawn_control_mode` shells out to real tmux and the
backoff `sleep`s for seconds — neither is unit-testable. Split the concern so the
retry/backoff logic takes an **injectable connect closure** returning a `BufRead`, and an
**injectable backoff** so tests can pass zero delays. The real tmux spawn becomes just the
production `connect`.

```rust
use std::io::BufRead;
use std::sync::mpsc;
use std::time::Duration;

const MAX_RETRIES: usize = 5;

/// Reads control-mode lines until the stream ends. Returns `true` if it saw `%exit`
/// (server/client gone → do not reconnect), `false` on plain EOF (dropped → reconnect).
fn pump_events(reader: impl BufRead, event_tx: &mpsc::Sender<TmuxEvent>) -> bool {
    for line in reader.lines() {
        let Ok(line) = line else { return false };
        if let Some(event) = parse_event(&line) {
            let is_exit = event == TmuxEvent::Exit;
            if event_tx.send(event).is_err() {
                return true; // receiver dropped: app is gone, stop.
            }
            if is_exit {
                return true;
            }
        }
    }
    false
}

/// Retry/backoff loop, decoupled from tmux and from real time for testing.
fn run_with_reconnect<C, R>(
    mut connect: C,
    event_tx: &mpsc::Sender<TmuxEvent>,
    backoff: impl Fn(u32) -> Duration,
) where
    C: FnMut() -> std::io::Result<R>,
    R: BufRead,
{
    let mut retries = 0u32;
    loop {
        match connect() {
            Ok(reader) => {
                let clean_exit = pump_events(reader, event_tx);
                if clean_exit {
                    let _ = event_tx.send(TmuxEvent::Exit);
                    return;
                }
                retries = 0; // connection worked; a later drop restarts backoff.
            }
            Err(e) => {
                tracing::debug!("control mode connect failed: {e}");
                retries += 1;
                if retries as usize >= MAX_RETRIES {
                    tracing::error!("max reconnection attempts reached");
                    let _ = event_tx.send(TmuxEvent::Exit);
                    return;
                }
                std::thread::sleep(backoff(retries));
            }
        }
    }
}
```

**Tests to write first** (all fast, no tmux, no real sleeps — pass `|_| Duration::ZERO`):

```rust
#[test]
fn test_pump_forwards_events_and_stops_on_exit() {
    let (tx, rx) = mpsc::channel();
    let input = std::io::Cursor::new("%window-add @3\n%output %1 hi\n%exit\n");
    assert!(pump_events(input, &tx)); // saw %exit
    assert_eq!(rx.recv().unwrap(), TmuxEvent::Refresh);
    assert_eq!(rx.recv().unwrap(), TmuxEvent::Refresh);
    assert_eq!(rx.recv().unwrap(), TmuxEvent::Exit);
}

#[test]
fn test_reconnects_then_exits_after_max_retries() {
    let (tx, rx) = mpsc::channel();
    let mut attempts = 0;
    run_with_reconnect(
        || { attempts += 1; Err(std::io::Error::other("boom")) as std::io::Result<std::io::Cursor<&[u8]>> },
        &tx,
        |_| Duration::ZERO,
    );
    assert_eq!(attempts, MAX_RETRIES);
    assert_eq!(rx.recv().unwrap(), TmuxEvent::Exit);
}

#[test]
fn test_clean_exit_stops_immediately() {
    let (tx, rx) = mpsc::channel();
    run_with_reconnect(
        || Ok(std::io::Cursor::new(b"%exit\n".to_vec())),
        &tx,
        |_| Duration::ZERO,
    );
    // one Refresh-free stream that only carried %exit, plus the loop's own Exit
    assert_eq!(rx.recv().unwrap(), TmuxEvent::Exit);
}
```

**Production entry point + real connect:**

```rust
pub fn control_mode_thread(session: String, event_tx: mpsc::Sender<TmuxEvent>) {
    run_with_reconnect(
        || spawn_control_mode(&session),
        &event_tx,
        |n| Duration::from_secs(2u64.pow(n - 1)), // 1s, 2s, 4s, 8s
    );
}

/// Spawns `tmux -C attach-session -t <session>` and returns a reader over its stdout.
/// The returned reader owns the `Child`; keep it alive for the connection's lifetime.
fn spawn_control_mode(session: &str) -> std::io::Result<impl BufRead> {
    // Command::new("tmux").args(["-C", "attach-session", "-t", session])
    //   .stdin(Stdio::piped())   // MUST stay open, see note below
    //   .stdout(Stdio::piped())
    //   .stderr(Stdio::null())
    //   .env_remove("TMUX").env_remove("TMUX_TMPDIR")  // like execute_inherit_stdio
    //   .spawn()?;
    // Wrap child.stdout + the retained child + child.stdin in a small struct that
    // implements BufRead by delegating to a BufReader<ChildStdout>. Dropping it (on
    // reconnect or process exit) closes stdin, so tmux detaches the control client.
    todo!()
}
```

> **Gap — stdin lifecycle / leaked clients:** a `tmux -C attach` client is a *real* client.
> If stdin is `Stdio::null()`, tmux may exit immediately; if the child is spawned and
> forgotten, it stays attached after `aot` exits. Spawn with `stdin(Stdio::piped())` and
> keep the `ChildStdin` owned alongside the reader. When the reader is dropped (reconnect)
> or the process exits, stdin closes and tmux cleanly detaches — no explicit kill needed,
> no leaked clients.

> **Gap — env like the existing attach:** mirror `execute_inherit_stdio` and remove `TMUX`
> / `TMUX_TMPDIR` from the child env so the control client attaches to the right server and
> does not nest.

### Step 1.3b: Prevent control-client resize (MODIFY `tmux.rs`)

A control-mode client attaching at 80x24 will, under the default `window-size latest`,
shrink the user's real attached view of `agents-on-tmux`. In
`create_session_if_not_exists`, when creating the session, also set:

```rust
self.executor
    .execute(&["set-option", "-t", &self.session, "window-size", "largest"])?;
```

Add a test asserting the option is issued (extend the existing session-creation coverage in
`tmux.rs`). This keeps the session sized to the user's real terminal regardless of the
control client.

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

    app.process_tmux_events();
    assert_eq!(app.windows().len(), 5);
}

#[test]
fn test_multiple_events_refresh_once() {
    // Coalescing: many events, but list_windows is re-read a single time.
    let (mut app, _, _) = test_app();
    let (tx, rx) = mpsc::channel();
    app.event_rx = Some(rx);
    for _ in 0..10 {
        let _ = tx.send(TmuxEvent::Refresh);
    }
    app.process_tmux_events();
    assert!(app.running);
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

> To assert coalescing more strictly (exactly one `list_windows` call per drain), extend
> `MockTmux` with a call counter on `list_windows` — optional but aligns with the
> AGENTS.md 100%-coverage goal.

**Implementation:**

Add method to `App`. **Gap — borrow checker + refresh storm:** the original draft held
`&self.event_rx` (immutable borrow) while calling `self.refresh_windows()` (needs `&mut
self`) — that does not compile. It also refreshed once *per* event, which is pathological
given `%output` volume. Fix both: drain the channel into local flags first (the receiver
borrow ends before any `&mut self` call), then refresh at most once.

```rust
fn process_tmux_events(&mut self) {
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
}
```

Modify `App::run()`. **Gap — the uptime timer must keep ticking.** `ui.rs` renders
`format_elapsed(window.started_at)`, so the screen must repaint on a small cadence even when
no events arrive. Keep a ~1s redraw tick for *painting*; data *refresh* is now event-driven
(the 5s `refresh_windows` poll is gone). Also debounce redraws so an `%output` burst does not
repaint on every iteration.

```rust
pub fn run(&mut self, mut terminal: DefaultTerminal) -> anyhow::Result<()> {
    let theme = Theme::default();

    // Spawn control mode thread (only the session name crosses the thread boundary).
    let (event_tx, event_rx) = mpsc::channel();
    let session = self.nested_driver.session_name().to_string();
    std::thread::spawn(move || {
        crate::backends::control_mode::control_mode_thread(session, event_tx);
    });
    self.event_rx = Some(event_rx);

    let redraw_tick = Duration::from_secs(1);
    let mut last_draw = Instant::now() - redraw_tick;
    while self.running {
        if last_draw.elapsed() >= redraw_tick {
            terminal.draw(|frame| ui::draw(frame, self, &theme))?;
            last_draw = Instant::now();
        }

        // Poll keyboard (100ms). A keypress forces an immediate redraw next iteration.
        if event::poll(Duration::from_millis(100))?
            && let event::Event::Key(key) = event::read()?
            && key.kind == event::KeyEventKind::Press
        {
            self.handle_action(key_to_action(key));
            last_draw = Instant::now() - redraw_tick;
        }

        // Drain tmux events (non-blocking). A refresh forces an immediate redraw.
        let before = self.windows().len();
        self.process_tmux_events();
        if self.windows().len() != before {
            last_draw = Instant::now() - redraw_tick;
        }
    }

    Ok(())
}
```

> Keep the `use std::time::{Duration, Instant};` import — it stays in use for the redraw
> tick even though the old `REFRESH_INTERVAL_SECS` refresh is removed.

> **Gap — `MockTmux` in these tests** must implement the new `Tmux::session_name`; add
> `fn session_name(&self) -> &str { "agents-on-tmux" }` (see Step 1.2).

### Step 2.3: Remove Polling

**Remove:**

- `REFRESH_INTERVAL_SECS` constant (the 5s value)
- The time-based **data refresh**: the `if should_redraw { self.refresh_windows()?; }`
  block in `run()`. `refresh_windows` is now called only from `process_tmux_events`.

**Keep (repurposed, do NOT remove):**

- `last_draw` + a `redraw_tick` (~1s) drive **UI painting** so the uptime counter keeps
  ticking. This is a redraw cadence, not a data poll — see Step 2.2's `run()`.
- `std::time::{Duration, Instant}` import stays in use.

**Tests:**

- No existing test asserts the 5s timing directly (they call `refresh_windows()`
  explicitly), so none need deletion. Add the event-driven tests from Step 2.2.

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

Add to `Cli` (`main.rs`):

```rust
/// Enable debug logging to a file
#[arg(long, env = "AOT_DEBUG", value_parser = parse_bool, default_missing_value = "true", num_args = 0..=1, require_equals = true)]
debug: Option<bool>,
```

> **Gap — `debug` must be threaded through every place the icon flags already are.**
> Missing any one of these breaks compilation or silently drops the flag:
>
> 1. `Config` struct (`backends/config.rs`): add `#[serde(default)] pub debug: Option<bool>,`
> 2. `Config::merge` (`config.rs`): add `debug: other.debug.or(self.debug),`
> 3. `impl From<&Cli> for Config` (`main.rs`): add `debug: cli.debug,`
> 4. `impl Display for Cli` (`main.rs`): forward it, e.g.
>    `if let Some(debug) = self.debug { write!(f, " --debug={}", debug)?; }`
>
> Item 4 is essential and easy to miss: the control-mode thread runs inside the **`--tui`
> child process** that `main` launches via `split_window(format!("{} --tui=true{}", exe, cli))`.
> If `Display` does not emit `--debug`, the launching process's flag never reaches the TUI
> and logging stays off where it matters. Update the existing `config.rs` merge tests and
> `main.rs` `Display`/`From` tests to cover the new field.

### Step 3.2: Logger Initialization

**Tests to write first:**

```rust
#[test]
fn test_logger_initialized_with_debug() {
    // This is hard to test directly, so we'll just verify the flag is passed
    let config = Config { debug: Some(true), /* ... */ };
    assert_eq!(config.debug, Some(true));
}
```

(The logger itself is already covered by `src/logger.rs` tests.)

**Implementation:**

> **Gap — never log to stdout/stderr in a TUI.** stdout is the ratatui alternate screen —
> logging there would corrupt the display. `logger::debug`/`logger::error` are silent
> no-ops until `logger::init` points them at a **file**. Initialize before
> `ratatui::init()` and before spawning the control-mode thread, and remove the temporary
> `#![allow(dead_code)]` from `src/logger.rs`.

In `main()`:

```rust
if config.debug.unwrap_or(false) {
    let path = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("aot");
    let _ = std::fs::create_dir_all(&path);
    let _ = logger::init(&path.join("aot.log"));
}
```

> `dirs` is already a dependency. `logger::init` is a global one-shot (`OnceLock`);
> repeated calls are no-ops.

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
   - **Trigger activity in a background agent window** (e.g. a build finishes / bell):
     verify the `notification_pending` indicator lights up promptly and clears on focus —
     this is the `%output`/`%window-pane-changed` path and the whole reason for the change.
   - Confirm the user's attached view of `agents-on-tmux` is **not** resized to 80x24 when
     the TUI (control client) is running (validates `window-size largest`).
   - Verify the uptime counter keeps ticking while idle (no keypresses/events).
   - Kill tmux server: verify app exits after 5 retries.
   - Verify debug log is written to `${XDG_CACHE_HOME:-~/.cache}/aot/aot.log` and the TUI
     display is not corrupted by log output.

---

## tmux Control Mode Protocol Reference

Notifications and how this app maps them (all "state changed" notifications collapse to
`TmuxEvent::Refresh`; see Step 1.1):

| Event | Format | Example | Maps to |
|-------|--------|---------|---------|
| Window added | `%window-add @<id>` | `%window-add @3` | Refresh |
| Window closed | `%window-close @<id>` | `%window-close @5` | Refresh |
| Window renamed | `%window-renamed @<id> <name>` | `%window-renamed @3 my tmux` | Refresh |
| Session changed | `%session-changed $<sid> <widx>` | `%session-changed $1 1` | Refresh |
| Active window changed | `%session-window-changed $<sid> @<id>` | `%session-window-changed $1 @5` | Refresh |
| Pane changed (focus) | `%window-pane-changed @<id> %<pane>` | `%window-pane-changed @3 %6` | Refresh |
| Pane output (activity) | `%output %<pane> <data>` | `%output %5 done` | Refresh |
| Exit | `%exit` | `%exit` | Exit |

Notes:

- Window IDs use `@`, session IDs use `$`, pane IDs use `%`.
- **`%output`/`%window-pane-changed` are the events that keep `notification_pending`
  (the tmux activity flag) live** — structural events alone do not. This is why the parser
  treats them as refresh triggers, coalesced in `process_tmux_events` (Step 2.2).
- There is **no** `%window-activity` control-mode notification; do not rely on one.
- Names in `%window-renamed` may contain spaces — parse the id, treat the rest as the name.

---

## Notes

- **Session lifecycle**: The tmux session persists independently of the app. Killing the app does NOT kill the session.
- **Reconnection**: If control mode connection drops, the app attempts to reconnect with exponential backoff.
- **Debug logging**: Use `--debug` flag or `AOT_DEBUG=1` env var to enable debug logging to
  a file (`${XDG_CACHE_HOME:-~/.cache}/aot/aot.log`) via the std-only `src/logger.rs`
  module (no logging crates). Never log to stdout/stderr — it would corrupt the ratatui
  alternate screen.
- **No async runtime**: Using threads and channels keeps the implementation simple and avoids adding tokio as a dependency.

---

## Cleanup

**When implementation is complete and verified, delete this plan file:**

```bash
rm -f docs/plans/2026-07-24-tmux-control-mode.md
```

---

## Appendix: Notification Alternatives (Reference)

Options explored for a live "window had background activity" signal, for future
reference — none implemented.

- **WORKS — `monitor-activity`** — tmux option; sets `window_activity_flag`,
  confirmed via `tmux list-windows -F '...#{window_activity_flag}...'`. No
  corresponding control-mode notification (no `%window-activity`), so this
  requires bringing back **list-windows polling** — the flag is only readable by
  re-running `list_windows`, not pushed.
- **WORKS (same mechanism) — `monitor-bell`** — same idea for bell
  (`printf '\a'`), via `#{window_bell_flag}`; visual bell shows on inactive
  windows. Same as `monitor-activity`: no control-mode notification, needs the
  same polling to read the flag.
- **DOES NOT WORK — Hook + `display-message` + monitor** — bind
  `alert-activity`/`alert-bell` to `display-message`, hoping it surfaces as
  `%message`. Tested: never fires — likely because `display-message` needs a
  real client to show to, and aot's nested session only has a control-mode
  client attached (no visual terminal target).
- **UNTESTED — `refresh-client -B` (subscribe)** — control client issues
  `refresh-client -B name:what:format` to subscribe to a format string (e.g.
  `#{window_activity_flag}`); tmux pushes `%subscription-changed` when it
  changes. Purpose-built for this. Exact `what` targeting syntax (per-pane vs
  per-window, `%*` fan-out) unconfirmed.
- **UNTESTED — Hook + direct IPC to aot** — bind `alert-activity`/`alert-bell`
  to a `run-shell` command that writes directly to a socket/pipe aot listens on
  (window id + kind), bypassing control mode entirely. Sidesteps all of the
  above limitations but adds a new IPC backend and hook lifecycle management
  (install on session create, matching the `window-size largest` pattern).

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const MAX_RETRIES: usize = 5;

/// Events emitted by the tmux control-mode client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxEvent {
    /// Some observable state changed — re-run list_windows.
    Refresh,
    /// The control-mode connection ended or the server exited.
    Exit,
}

/// Owns the control-mode child process. Reading delegates to the child's stdout;
/// dropping closes stdin so tmux detaches the control client, then reaps the child.
struct ControlModeReader {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Read for ControlModeReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.stdout.read(buf)
    }
}

impl BufRead for ControlModeReader {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.stdout.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.stdout.consume(amt)
    }
}

impl Drop for ControlModeReader {
    fn drop(&mut self) {
        // Closing stdin makes tmux detach the control client and exit.
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

/// Parses a single control-mode notification line into an event, if relevant.
pub fn parse_event(line: &str) -> Option<TmuxEvent> {
    let mut parts = line.split(' ');
    match parts.next()? {
        command @ "%exit" => {
            super::logger::debug(&format!("control_mode: {command}"));
            Some(TmuxEvent::Exit)
        }
        // Structural changes: validate the id so malformed lines are ignored.
        command @ "%window-add" | command @ "%window-close" | command @ "%window-renamed" => {
            super::logger::debug(&format!("control_mode: {command}"));
            let id = parts.next()?.strip_prefix('@')?;
            id.parse::<u32>().ok().map(|_| TmuxEvent::Refresh)
        }
        // Activity / focus changes: keep the active window live.
        "%session-changed" | "%session-window-changed" | "%window-pane-changed" => {
            Some(TmuxEvent::Refresh)
        }
        command => {
            super::logger::debug(&format!("control_mode: {command}"));
            None
        }
    }
}

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
                super::logger::debug(&format!("control_mode: connect failed: {e}"));
                retries += 1;
                if retries as usize >= MAX_RETRIES {
                    super::logger::error("control_mode: max reconnection attempts reached");
                    let _ = event_tx.send(TmuxEvent::Exit);
                    return;
                }
                std::thread::sleep(backoff(retries));
            }
        }
    }
}

/// Runs the control-mode client for the given session, sending events on the channel.
/// Reconnects with exponential backoff; sends `Exit` and returns when the connection
/// ends cleanly or retries are exhausted.
pub fn control_mode_thread(session: String, event_tx: mpsc::Sender<TmuxEvent>) {
    run_with_reconnect(
        || spawn_control_mode(&session),
        &event_tx,
        |n| Duration::from_secs(2u64.pow(n - 1)), // 1s, 2s, 4s, 8s
    );
}

/// Spawns `tmux -C attach-session -t <session>` and returns a reader over its stdout.
fn spawn_control_mode(session: &str) -> std::io::Result<ControlModeReader> {
    let mut child = Command::new("tmux")
        .args(["-C", "attach-session", "-t", session])
        .stdin(Stdio::piped()) // must stay open; closing it detaches the client
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_remove("TMUX")
        .env_remove("TMUX_TMPDIR")
        .spawn()?;

    let stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture tmux stdout"))?;

    Ok(ControlModeReader {
        child,
        stdin,
        stdout: BufReader::new(stdout),
    })
}

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
        assert_eq!(
            parse_event("%window-renamed @3 my window"),
            Some(TmuxEvent::Refresh)
        );
    }

    #[test]
    fn test_parse_session_changed() {
        assert_eq!(
            parse_event("%session-changed $1 1"),
            Some(TmuxEvent::Refresh)
        );
    }

    #[test]
    fn test_parse_session_window_changed_triggers_refresh() {
        assert_eq!(
            parse_event("%session-window-changed $1 @5"),
            Some(TmuxEvent::Refresh)
        );
    }

    #[test]
    fn test_parse_output_is_ignored() {
        assert_eq!(parse_event("%output %5 hello"), None);
    }

    #[test]
    fn test_parse_extended_output_is_ignored() {
        assert_eq!(parse_event("%extended-output %5 0 : hello"), None);
    }

    #[test]
    fn test_parse_window_pane_changed_triggers_refresh() {
        assert_eq!(
            parse_event("%window-pane-changed @3 %6"),
            Some(TmuxEvent::Refresh)
        );
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
        assert_eq!(parse_event("%window-add notanumber"), None);
        assert_eq!(parse_event("%window-add"), None);
    }

    #[test]
    fn test_pump_forwards_events_and_stops_on_exit() {
        let (tx, rx) = mpsc::channel();
        let input = std::io::Cursor::new("%window-add @3\n%output %1 hi\n%exit\n");
        assert!(pump_events(input, &tx)); // saw %exit
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Refresh);
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Exit);
    }

    #[test]
    fn test_pump_returns_false_on_eof() {
        let (tx, rx) = mpsc::channel();
        let input = std::io::Cursor::new("%window-add @3\n");
        assert!(!pump_events(input, &tx)); // plain EOF, no %exit
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Refresh);
    }

    #[test]
    fn test_pump_stops_when_receiver_dropped() {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let input = std::io::Cursor::new("%window-add @3\n%window-add @4\n");
        assert!(pump_events(input, &tx)); // receiver gone: stop for good
    }

    #[test]
    fn test_reconnects_then_exits_after_max_retries() {
        let (tx, rx) = mpsc::channel();
        let mut attempts = 0;
        run_with_reconnect(
            || -> std::io::Result<std::io::Cursor<&[u8]>> {
                attempts += 1;
                Err(std::io::Error::other("boom"))
            },
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
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Exit);
    }

    #[test]
    fn test_reconnect_resets_retries_after_success() {
        let (tx, rx) = mpsc::channel();
        let mut attempts = 0;
        // Streams that end without %exit trigger reconnects; after enough failed
        // connects following a success, retries still count from zero again.
        run_with_reconnect(
            || -> std::io::Result<std::io::Cursor<Vec<u8>>> {
                attempts += 1;
                match attempts {
                    1..=4 => Err(std::io::Error::other("boom")),
                    5 => Ok(std::io::Cursor::new(b"%window-add @1\n".to_vec())),
                    _ => Err(std::io::Error::other("boom")),
                }
            },
            &tx,
            |_| Duration::ZERO,
        );
        // 4 failures (retries 1-4), one success (resets), then 5 more failures.
        assert_eq!(attempts, 10);
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Refresh);
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Exit);
    }
}

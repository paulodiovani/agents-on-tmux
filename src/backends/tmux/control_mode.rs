use std::io::{BufRead, BufReader, Read};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::backends::logger;

const MAX_RETRIES: usize = 5;

/// Outcome of pumping events from a control-mode connection.
#[derive(Debug, PartialEq, Eq)]
enum PumpOutcome {
    /// Stop reconnecting (exit received or receiver dropped).
    Exit,
    /// Connection lost, should reconnect. `forwarded` tells whether the
    /// connection delivered any event before it ended.
    Reconnect { forwarded: bool },
}

/// Events emitted by the tmux control-mode client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxEvent {
    /// The control-mode connection ended or the server exited.
    Exit,
    /// The active pane changed to the given pane-id.
    PaneChanged(String),
    /// Some observable state changed — re-run list_windows.
    Refresh,
}

/// Which tmux server a control-mode connection targets.
/// Determines which events are parsed and forwarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventProducer {
    /// The parent tmux server — only pane focus events.
    Parent,
    /// The nested tmux server — all events except pane focus.
    Nested,
}

/// Owns the control-mode child process. Reading delegates to the child's stdout;
/// dropping closes stdin so tmux detaches the control client, then reaps the child.
struct ControlModeReader {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    stderr_thread: Option<JoinHandle<()>>,
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
        match self.child.wait() {
            Ok(status) => {
                if !status.success() {
                    logger::debug!("control_mode: tmux exited with {status}");
                }
            }
            Err(error) => {
                logger::error!("control_mode: failed to wait for tmux: {error}");
            }
        }
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
    }
}

/// Parses a single control-mode notification line into an event, if relevant.
fn parse_event(line: &str) -> Option<TmuxEvent> {
    let mut parts = line.split(' ');
    let command = parts.next()?;

    match command {
        "%exit" => {
            logger::debug!("control_mode: {command}");
            Some(TmuxEvent::Exit)
        }
        "%session-changed" | "%session-window-changed" => {
            logger::debug!("control_mode: {command}");
            Some(TmuxEvent::Refresh)
        }
        "%window-add" | "%window-close" | "%window-renamed" => {
            logger::debug!("control_mode: {command}");
            let raw = parts.next()?;
            let id = raw.strip_prefix('@')?;
            match id.parse::<u32>() {
                Ok(_) => Some(TmuxEvent::Refresh),
                Err(_) => {
                    logger::debug!("control_mode: malformed window id '{id}'");
                    None
                }
            }
        }
        "%window-pane-changed" => {
            logger::debug!("control_mode: {command}");
            let id = parts.nth(1).filter(|id| id.starts_with("%"));
            match id {
                Some(id) => Some(TmuxEvent::PaneChanged(id.to_string())),
                None => {
                    logger::debug!("control_mode: {} missing pane id", command);
                    None
                }
            }
        }
        _ => None,
    }
}

/// Reads control-mode lines until the stream ends. Returns `PumpOutcome::Exit` if it saw `%exit`
/// (server/client gone → do not reconnect), `PumpOutcome::Reconnect` on plain EOF (dropped → reconnect).
/// Filters events based on the producer:
/// - `Parent`: only forwards `PaneChanged` events (and `Exit` to signal disconnection)
/// - `Nested`: forwards all events except `PaneChanged`
fn pump_events(
    mut reader: impl BufRead,
    producer: EventProducer,
    event_tx: &mpsc::Sender<TmuxEvent>,
) -> PumpOutcome {
    let mut buf = Vec::new();
    let mut forwarded = false;
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                logger::error!("control_mode: read error: {error}");
                return PumpOutcome::Reconnect { forwarded };
            }
        }
        let line = String::from_utf8_lossy(&buf).trim_end().to_string();

        match (producer, parse_event(&line)) {
            (_, None) => continue,

            (_, Some(TmuxEvent::Exit)) => {
                send_exit(event_tx);
                return PumpOutcome::Exit;
            }

            (EventProducer::Parent, Some(event @ TmuxEvent::PaneChanged(_))) => {
                if let Err(error) = event_tx.send(event) {
                    logger::error!("control_mode: failed to send event: {error}");
                    return PumpOutcome::Exit;
                }
                forwarded = true;
            }

            (EventProducer::Nested, Some(event)) if !matches!(event, TmuxEvent::PaneChanged(_)) => {
                if let Err(error) = event_tx.send(event) {
                    logger::error!("control_mode: failed to send event: {error}");
                    return PumpOutcome::Exit;
                }
                forwarded = true;
            }

            _ => continue,
        }
    }
    PumpOutcome::Reconnect { forwarded }
}

/// Retry/backoff loop, decoupled from tmux and from real time for testing.
/// A connection that ends after delivering events resets the retry count; one
/// that ends without delivering any counts as a failed attempt.
fn run_with_reconnect<C, R>(
    mut connect: C,
    producer: EventProducer,
    event_tx: &mpsc::Sender<TmuxEvent>,
    backoff: impl Fn(u32) -> Duration,
) where
    C: FnMut() -> std::io::Result<R>,
    R: BufRead,
{
    let mut retries = 0u32;
    loop {
        let failure = match connect() {
            Ok(reader) => match pump_events(reader, producer, event_tx) {
                PumpOutcome::Exit => {
                    send_exit(event_tx);
                    return;
                }
                PumpOutcome::Reconnect { forwarded: true } => {
                    logger::debug!("control_mode: connection ended, reconnecting");
                    retries = 0;
                    None
                }
                PumpOutcome::Reconnect { forwarded: false } => {
                    Some("connection ended without events".to_string())
                }
            },
            Err(error) => Some(error.to_string()),
        };
        if let Some(error) = failure {
            logger::debug!("control_mode: connect failed: {error}");
            retries += 1;
            if retries as usize >= MAX_RETRIES {
                logger::error!("control_mode: max reconnection attempts reached: {error}");
                send_exit(event_tx);
                return;
            }
        }
        std::thread::sleep(backoff(retries.max(1)));
    }
}

fn send_exit(event_tx: &mpsc::Sender<TmuxEvent>) {
    if let Err(error) = event_tx.send(TmuxEvent::Exit) {
        logger::error!("control_mode: failed to send exit: {error}");
    }
}

/// Runs a control-mode client, sending its events on the channel. `command`
/// builds the client process for each connection attempt. Reconnects with
/// exponential backoff; sends `Exit` and returns when the connection ends
/// cleanly or retries are exhausted.
pub fn control_mode_thread(
    command: impl Fn() -> Command,
    producer: EventProducer,
    event_tx: mpsc::Sender<TmuxEvent>,
) {
    run_with_reconnect(|| spawn_reader(command()), producer, &event_tx, backoff);
}

/// Delay before reconnect attempt `attempt` (1-based), doubling from one second.
fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(2u64.saturating_pow(attempt.saturating_sub(1)))
}

/// Spawns the control-mode client and returns a reader over its stdout.
fn spawn_reader(mut command: Command) -> std::io::Result<ControlModeReader> {
    let mut child = command
        // Closing stdin detaches the client.
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture tmux stdout"))?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture tmux stderr"))?;

    let stderr_thread = std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            if !line.is_empty() {
                logger::error!("control_mode: tmux: {line}");
            }
        }
    });

    Ok(ControlModeReader {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        stderr_thread: Some(stderr_thread),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::logger::tests::{init_for_test, temp_log_path};
    use std::cell::RefCell;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn test_control_mode_thread_forwards_events_until_exit() {
        let (tx, rx) = mpsc::channel();

        control_mode_thread(
            || shell("printf '%s\\n' '%window-add @1' '%exit'"),
            EventProducer::Nested,
            tx,
        );

        let events: Vec<TmuxEvent> = rx.try_iter().collect();
        assert_eq!(events.first(), Some(&TmuxEvent::Refresh));
        assert_eq!(events.get(1), Some(&TmuxEvent::Exit));
    }

    #[test]
    fn test_spawned_reader_streams_stdout() {
        let reader = spawn_reader(shell("printf '%s\\n' '%window-add @1' '%exit'")).unwrap();
        let (tx, rx) = mpsc::channel();

        let outcome = pump_events(reader, EventProducer::Nested, &tx);

        assert_eq!(outcome, PumpOutcome::Exit);
        assert_eq!(
            rx.try_iter().collect::<Vec<_>>(),
            [TmuxEvent::Refresh, TmuxEvent::Exit]
        );
    }

    #[test]
    fn test_dropping_reader_closes_stdin_and_reaps_child() {
        let path = temp_log_path();
        let _guard = init_for_test(&path);
        let reader = spawn_reader(shell("cat >/dev/null; exit 3")).unwrap();

        drop(reader);

        let log = std::fs::read_to_string(&path).unwrap();
        assert!(log.contains("control_mode: tmux exited with exit status: 3"));
    }

    #[test]
    fn test_spawned_reader_logs_stderr_lines() {
        let path = temp_log_path();
        let _guard = init_for_test(&path);
        let reader = spawn_reader(shell("echo 'no sessions' >&2; echo >&2")).unwrap();

        drop(reader);

        let log = std::fs::read_to_string(&path).unwrap();
        assert!(log.contains("ERROR control_mode: tmux: no sessions"));
        assert_eq!(log.matches("control_mode: tmux:").count(), 1);
    }

    #[test]
    fn test_spawned_reader_reads_raw_bytes() {
        let mut reader = spawn_reader(shell("printf ok")).unwrap();
        let mut output = String::new();

        reader.read_to_string(&mut output).unwrap();

        assert_eq!(output, "ok");
    }

    #[test]
    fn test_backoff_doubles_from_one_second() {
        let delays: Vec<u64> = (1..=4).map(|attempt| backoff(attempt).as_secs()).collect();
        assert_eq!(delays, [1, 2, 4, 8]);
    }

    #[test]
    fn test_pump_read_error_reconnects() {
        struct FailingReader;
        impl Read for FailingReader {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("broken pipe"))
            }
        }
        let (tx, _rx) = mpsc::channel();

        let outcome = pump_events(BufReader::new(FailingReader), EventProducer::Nested, &tx);

        assert_eq!(outcome, PumpOutcome::Reconnect { forwarded: false });
    }

    #[test]
    fn test_pump_parent_stops_when_receiver_dropped() {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let input = std::io::Cursor::new("%window-pane-changed @1 %2\n");
        assert_eq!(
            pump_events(input, EventProducer::Parent, &tx),
            PumpOutcome::Exit
        );
    }

    #[test]
    fn test_exit_with_receiver_dropped_is_logged() {
        let path = temp_log_path();
        let _guard = init_for_test(&path);
        let (tx, rx) = mpsc::channel();
        drop(rx);

        pump_events(std::io::Cursor::new("%exit\n"), EventProducer::Nested, &tx);

        let log = std::fs::read_to_string(&path).unwrap();
        assert!(log.contains("control_mode: failed to send exit"));
    }

    #[test]
    fn test_spawn_failure_is_an_error() {
        assert!(spawn_reader(Command::new("/nonexistent/aot-test-program")).is_err());
    }

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
    fn test_parse_window_pane_changed() {
        assert_eq!(
            parse_event("%window-pane-changed @3 %6"),
            Some(TmuxEvent::PaneChanged("%6".to_string()))
        );
    }

    #[test]
    fn test_parse_window_pane_changed_malformed_is_none() {
        assert_eq!(parse_event("%window-pane-changed @3"), None);
        assert_eq!(parse_event("%window-pane-changed @3 6"), None);
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
    fn test_pump_parent_forwards_only_pane_changed() {
        let (tx, rx) = mpsc::channel();
        let input = std::io::Cursor::new("%window-add @3\n%window-pane-changed @1 %5\n%exit\n");
        assert_eq!(
            pump_events(input, EventProducer::Parent, &tx),
            PumpOutcome::Exit
        );
        // Parent should only forward PaneChanged, not window-add
        assert_eq!(rx.recv().unwrap(), TmuxEvent::PaneChanged("%5".to_string()));
        // Exit is also forwarded by Parent
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Exit);
    }

    #[test]
    fn test_pump_nested_forwards_all_except_pane_changed() {
        let (tx, rx) = mpsc::channel();
        let input = std::io::Cursor::new("%window-add @3\n%window-pane-changed @1 %5\n%exit\n");
        assert_eq!(
            pump_events(input, EventProducer::Nested, &tx),
            PumpOutcome::Exit
        );
        // Nested should forward window-add and exit, but not pane-changed
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Refresh); // window-add
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Exit);
    }

    #[test]
    fn test_pump_forwards_events_and_stops_on_exit() {
        let (tx, rx) = mpsc::channel();
        let input = std::io::Cursor::new("%window-add @3\n%output %1 hi\n%exit\n");
        assert_eq!(
            pump_events(input, EventProducer::Nested, &tx),
            PumpOutcome::Exit
        ); // saw %exit
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Refresh);
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Exit);
    }

    #[test]
    fn test_pump_returns_reconnect_on_eof() {
        let (tx, rx) = mpsc::channel();
        let input = std::io::Cursor::new("%window-add @3\n");
        assert_eq!(
            pump_events(input, EventProducer::Nested, &tx),
            PumpOutcome::Reconnect { forwarded: true }
        ); // plain EOF, no %exit
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Refresh);
    }

    #[test]
    fn test_pump_stops_when_receiver_dropped() {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let input = std::io::Cursor::new("%window-add @3\n%window-add @4\n");
        assert_eq!(
            pump_events(input, EventProducer::Nested, &tx),
            PumpOutcome::Exit
        ); // receiver gone: stop for good
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
            EventProducer::Nested,
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
            EventProducer::Nested,
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
            EventProducer::Nested,
            &tx,
            |_| Duration::ZERO,
        );
        // 4 failures (retries 1-4), one success (resets), then 5 more failures.
        assert_eq!(attempts, 10);
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Refresh);
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Exit);
    }

    #[test]
    fn test_pump_filtered_events_are_not_forwarded() {
        let (tx, _rx) = mpsc::channel();
        let input = std::io::Cursor::new("%window-add @3\n");
        assert_eq!(
            pump_events(input, EventProducer::Parent, &tx),
            PumpOutcome::Reconnect { forwarded: false }
        );
    }

    #[test]
    fn test_eof_without_events_counts_as_failure() {
        let (tx, rx) = mpsc::channel();
        let mut attempts = 0;
        run_with_reconnect(
            || -> std::io::Result<std::io::Cursor<&[u8]>> {
                attempts += 1;
                Ok(std::io::Cursor::new(b""))
            },
            EventProducer::Nested,
            &tx,
            |_| Duration::ZERO,
        );
        assert_eq!(attempts, MAX_RETRIES);
        assert_eq!(rx.recv().unwrap(), TmuxEvent::Exit);
    }

    #[test]
    fn test_backs_off_after_every_reconnect() {
        let (tx, _rx) = mpsc::channel();
        let mut attempts = 0;
        let delays = RefCell::new(Vec::new());
        run_with_reconnect(
            || -> std::io::Result<std::io::Cursor<Vec<u8>>> {
                attempts += 1;
                match attempts {
                    1 | 2 => Ok(std::io::Cursor::new(b"%window-add @1\n".to_vec())),
                    3 => Err(std::io::Error::other("boom")),
                    _ => Ok(std::io::Cursor::new(b"%exit\n".to_vec())),
                }
            },
            EventProducer::Nested,
            &tx,
            |n| {
                delays.borrow_mut().push(n);
                Duration::ZERO
            },
        );
        assert_eq!(*delays.borrow(), [1, 1, 1]);
    }

    #[test]
    fn test_pump_handles_invalid_utf8() {
        let (tx, rx) = mpsc::channel();
        let data = b"%window-add @1\n\xff\xfe invalid utf-8\n%session-changed\n";
        pump_events(
            std::io::Cursor::new(data.to_vec()),
            EventProducer::Nested,
            &tx,
        );
        let events: Vec<_> = rx.try_iter().collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], TmuxEvent::Refresh);
        assert_eq!(events[1], TmuxEvent::Refresh);
    }
}

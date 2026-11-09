use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ShimError {
    #[error("AOT MCP not running (no server at {0}); start aot first")]
    NotRunning(PathBuf),
    #[error("AOT closed the connection (did AOT exit?)")]
    Disconnected,
    #[error("AOT MCP connection failed: {0}")]
    Io(#[from] io::Error),
}

/// Bridges an agent's MCP stdio to the AOT MCP socket, identifying the agent by its tmux pane.
pub fn run(
    path: &Path,
    pane: Option<&str>,
    input: impl Read + Send + 'static,
    output: &mut impl Write,
) -> Result<(), ShimError> {
    let mut socket = UnixStream::connect(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
            ShimError::NotRunning(path.to_path_buf())
        }
        _ => ShimError::Io(error),
    })?;
    let handshake = json!({ "pane": pane }).to_string();
    socket.write_all(handshake.as_bytes())?;
    socket.write_all(b"\n")?;

    let mut upstream = socket.try_clone()?;
    let client_closed = Arc::new(AtomicBool::new(false));
    let input_done = Arc::clone(&client_closed);
    thread::spawn(move || {
        let _ = pump(input, &mut upstream);
        input_done.store(true, Ordering::SeqCst);
        let _ = upstream.shutdown(Shutdown::Write);
    });
    pump(socket, output)?;
    if client_closed.load(Ordering::SeqCst) {
        Ok(())
    } else {
        Err(ShimError::Disconnected)
    }
}

fn pump(mut from: impl Read, to: &mut impl Write) -> io::Result<()> {
    let mut buffer = [0u8; 8192];
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        to.write_all(&buffer[..read])?;
        to.flush()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Cursor};
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("closed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct InterruptedOnce {
        interrupted: bool,
        data: Cursor<&'static [u8]>,
    }

    impl Read for InterruptedOnce {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            self.data.read(buffer)
        }
    }

    struct BrokenReader;

    impl Read for BrokenReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("broken"))
        }
    }

    fn temp_socket() -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("aot-shim-test-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("mcp.sock")
    }

    fn echo_server(path: &Path) -> thread::JoinHandle<String> {
        let listener = UnixListener::bind(path).unwrap();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut lines = BufReader::new(stream).lines();
            let handshake = lines.next().unwrap().unwrap();
            for line in lines {
                writeln!(writer, "re:{}", line.unwrap()).unwrap();
            }
            handshake
        })
    }

    #[test]
    fn test_run_sends_handshake_and_bridges_both_ways() {
        let path = temp_socket();
        let server = echo_server(&path);
        let mut output = Vec::new();

        run(
            &path,
            Some("%4"),
            Cursor::new(b"a\nb\n".to_vec()),
            &mut output,
        )
        .unwrap();

        assert_eq!(server.join().unwrap(), r#"{"pane":"%4"}"#);
        assert_eq!(String::from_utf8(output).unwrap(), "re:a\nre:b\n");
    }

    #[test]
    fn test_run_without_pane_sends_null() {
        let path = temp_socket();
        let server = echo_server(&path);

        run(&path, None, Cursor::new(Vec::new()), &mut Vec::new()).unwrap();

        assert_eq!(server.join().unwrap(), r#"{"pane":null}"#);
    }

    #[test]
    fn test_run_reports_not_running_when_socket_missing() {
        let path = temp_socket();

        let error = run(&path, None, Cursor::new(Vec::new()), &mut Vec::new()).unwrap_err();

        assert!(matches!(error, ShimError::NotRunning(ref p) if *p == path));
        assert!(error.to_string().contains("AOT MCP not running"));
    }

    #[test]
    fn test_run_reports_not_running_when_socket_stale() {
        let path = temp_socket();
        drop(UnixListener::bind(&path).unwrap());

        let error = run(&path, None, Cursor::new(Vec::new()), &mut Vec::new()).unwrap_err();

        assert!(matches!(error, ShimError::NotRunning(_)));
    }

    #[test]
    fn test_run_reports_other_connect_errors() {
        let path = temp_socket();
        std::fs::write(path.with_extension("file"), "").unwrap();
        let path = path.with_extension("file").join("mcp.sock");

        let error = run(&path, None, Cursor::new(Vec::new()), &mut Vec::new()).unwrap_err();

        assert!(matches!(error, ShimError::Io(_)));
    }

    #[test]
    fn test_run_reports_aot_closing_the_connection() {
        let path = temp_socket();
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut lines = BufReader::new(stream).lines();
            lines.next().unwrap().unwrap()
        });
        let (input, _keep_open) = UnixStream::pair().unwrap();

        let error = run(&path, None, input, &mut Vec::new()).unwrap_err();

        server.join().unwrap();
        assert!(matches!(error, ShimError::Disconnected));
        assert_eq!(
            error.to_string(),
            "AOT closed the connection (did AOT exit?)"
        );
    }

    #[test]
    fn test_pump_retries_interrupted_reads() {
        let mut output = Vec::new();
        let input = InterruptedOnce {
            interrupted: false,
            data: Cursor::new(b"ok"),
        };
        pump(input, &mut output).unwrap();
        assert_eq!(output, b"ok");
    }

    #[test]
    fn test_pump_propagates_read_errors() {
        assert!(pump(BrokenReader, &mut Vec::new()).is_err());
    }

    #[test]
    fn test_pump_propagates_write_errors() {
        assert!(pump(Cursor::new(b"x".to_vec()), &mut FailingWriter).is_err());
    }
}

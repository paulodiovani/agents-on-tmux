use std::fs::{DirBuilder, Permissions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use serde::Deserialize;
use thiserror::Error;

use super::protocol::Protocol;
use super::tools::CallContext;
use crate::backends::logger;

const DIR_MODE: u32 = 0o700;
const SOCKET_DIR: &str = "aot";
const SOCKET_FILE: &str = "mcp.sock";
const SOCKET_MODE: u32 = 0o600;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("failed to set up MCP socket {path}: {source}")]
    Socket { path: PathBuf, source: io::Error },
}

#[derive(Deserialize)]
struct Handshake {
    pane: Option<String>,
}

/// Running MCP socket server owned by this AOT instance; removes its socket when dropped.
pub struct McpServer {
    path: PathBuf,
}

impl McpServer {
    /// Serves MCP on `path` in the background, unless a live server already owns it
    /// (then returns `None` and leaves that socket untouched).
    pub fn start(
        path: &Path,
        protocol: Arc<dyn Protocol + Send + Sync>,
    ) -> Result<Option<Self>, ServerError> {
        if UnixStream::connect(path).is_ok() {
            return Ok(None);
        }

        let socket_error = |source| ServerError::Socket {
            path: path.to_path_buf(),
            source,
        };
        match std::fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                return Err(socket_error(error));
            }
            _ => {}
        }
        if let Some(dir) = path.parent() {
            DirBuilder::new()
                .recursive(true)
                .mode(DIR_MODE)
                .create(dir)
                .map_err(socket_error)?;
        }
        let listener = UnixListener::bind(path).map_err(socket_error)?;
        let server = Self {
            path: path.to_path_buf(),
        };
        std::fs::set_permissions(path, Permissions::from_mode(SOCKET_MODE))
            .map_err(socket_error)?;

        thread::spawn(move || serve(listener, protocol));
        Ok(Some(server))
    }
}

impl Drop for McpServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Location of the AOT MCP socket, shared by the server and the `aot mcp` shim.
pub fn socket_path() -> PathBuf {
    socket_path_in(dirs::runtime_dir(), dirs::cache_dir())
}

fn socket_path_in(runtime_dir: Option<PathBuf>, cache_dir: Option<PathBuf>) -> PathBuf {
    runtime_dir
        .or(cache_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join(SOCKET_DIR)
        .join(SOCKET_FILE)
}

fn serve(listener: UnixListener, protocol: Arc<dyn Protocol + Send + Sync>) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let protocol = Arc::clone(&protocol);
                thread::spawn(move || {
                    if let Err(error) = handle_connection(stream, protocol.as_ref()) {
                        logger::error(&format!("mcp: connection closed: {error}"));
                    }
                });
            }
            Err(error) => logger::error(&format!("mcp: accept failed: {error}")),
        }
    }
}

fn handle_connection(stream: UnixStream, protocol: &dyn Protocol) -> io::Result<()> {
    let mut writer = stream.try_clone()?;
    let mut lines = BufReader::new(stream).lines();
    let Some(first) = lines.next() else {
        return Ok(());
    };
    let handshake: Handshake = serde_json::from_str(&first?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let context = CallContext {
        pane: handshake.pane,
    };
    logger::debug(&format!(
        "mcp: agent connected from pane {:?}",
        context.pane
    ));

    for line in lines {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = protocol.handle(&line, &context) {
            writer.write_all(reply.as_bytes())?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::Shutdown;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct EchoProtocol;

    impl Protocol for EchoProtocol {
        fn handle(&self, line: &str, context: &CallContext) -> Option<String> {
            (line != "quiet").then(|| format!("{:?}:{line}", context.pane))
        }
    }

    fn temp_socket(name: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir()
            .join(format!("aot-mcp-test-{}-{unique}", std::process::id()))
            .join(format!("{name}.sock"))
    }

    fn exchange(stream: &mut UnixStream, input: &str) -> String {
        stream.write_all(input.as_bytes()).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut output = String::new();
        stream.read_to_string(&mut output).unwrap();
        output
    }

    #[test]
    fn test_socket_path_prefers_runtime_dir() {
        let path = socket_path_in(Some("/run/user/1".into()), Some("/home/u/.cache".into()));
        assert_eq!(path, PathBuf::from("/run/user/1/aot/mcp.sock"));
    }

    #[test]
    fn test_socket_path_falls_back_to_cache_dir() {
        let path = socket_path_in(None, Some("/home/u/.cache".into()));
        assert_eq!(path, PathBuf::from("/home/u/.cache/aot/mcp.sock"));
    }

    #[test]
    fn test_socket_path_falls_back_to_temp_dir() {
        let path = socket_path_in(None, None);
        assert_eq!(path, std::env::temp_dir().join("aot").join("mcp.sock"));
    }

    #[test]
    fn test_socket_path_uses_environment_dirs() {
        assert!(socket_path().ends_with("aot/mcp.sock"));
    }

    #[test]
    fn test_handle_connection_replies_with_handshake_context() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_connection(server, &EchoProtocol));

        let output = exchange(&mut client, "{\"pane\":\"%7\"}\nhello\n\nquiet\nbye\n");

        handle.join().unwrap().unwrap();
        assert_eq!(output, "Some(\"%7\"):hello\nSome(\"%7\"):bye\n");
    }

    #[test]
    fn test_handle_connection_accepts_missing_pane() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_connection(server, &EchoProtocol));

        let output = exchange(&mut client, "{\"pane\":null}\nhi\n");

        handle.join().unwrap().unwrap();
        assert_eq!(output, "None:hi\n");
    }

    #[test]
    fn test_handle_connection_rejects_bad_handshake() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_connection(server, &EchoProtocol));

        let output = exchange(&mut client, "hello\n");

        let error = handle.join().unwrap().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(output, "");
    }

    #[test]
    fn test_handle_connection_without_handshake() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_connection(server, &EchoProtocol));

        assert_eq!(exchange(&mut client, ""), "");
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn test_start_binds_serves_and_removes_socket_on_drop() {
        let path = temp_socket("owner");
        let server = McpServer::start(&path, Arc::new(EchoProtocol))
            .unwrap()
            .expect("first instance owns the socket");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, SOCKET_MODE);
        let mut client = UnixStream::connect(&path).unwrap();
        assert_eq!(
            exchange(&mut client, "{\"pane\":\"%1\"}\nping\n"),
            "Some(\"%1\"):ping\n"
        );

        drop(server);
        assert!(!path.exists());
    }

    #[test]
    fn test_start_skips_when_socket_is_live() {
        let path = temp_socket("live");
        let owner = McpServer::start(&path, Arc::new(EchoProtocol))
            .unwrap()
            .unwrap();

        let second = McpServer::start(&path, Arc::new(EchoProtocol)).unwrap();

        assert!(second.is_none());
        assert!(path.exists());
        drop(owner);
    }

    #[test]
    fn test_start_replaces_stale_socket() {
        let path = temp_socket("stale");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        drop(UnixListener::bind(&path).unwrap());
        assert!(path.exists());

        let server = McpServer::start(&path, Arc::new(EchoProtocol))
            .unwrap()
            .expect("stale socket is replaced");

        let mut client = UnixStream::connect(&path).unwrap();
        assert_eq!(exchange(&mut client, "{\"pane\":null}\nx\n"), "None:x\n");
        drop(server);
    }

    #[test]
    fn test_start_fails_when_directory_cannot_be_created() {
        let blocker = temp_socket("blocker");
        std::fs::create_dir_all(blocker.parent().unwrap()).unwrap();
        std::fs::write(&blocker, "").unwrap();
        let path = blocker.join("mcp.sock");

        let error = McpServer::start(&path, Arc::new(EchoProtocol)).err();

        assert!(matches!(error, Some(ServerError::Socket { .. })));
    }
}

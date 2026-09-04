# Separate tmux server for agents-on-tmux

**Date:** 2026-09-04  
**Status:** In Progress  
**Issues:** [#20](https://github.com/paulodiovani/agents-on-tmux/issues/20), [#31](https://github.com/paulodiovani/agents-on-tmux/issues/31)

## Problem

The nested `agents-on-tmux` session runs on the same tmux server as the parent session, causing:

1. **Freeze on process kill (#20):** If the `aot` main process is killed while a user interacts with the nested session, the entire tmux server freezes and becomes unresponsive until manually restarted.

2. **Session detection failure (#31):** When launching `aot` from a tmux popup, `detect_parent_session()` incorrectly returns `agents-on-tmux` (the client's session) instead of the session containing the current pane.

## Solution

Use `tmux -L agents-on-tmux` to create an **isolated tmux server** for the nested session, similar to how `overmind` works. This provides:

- **Process isolation:** The nested session runs on its own server, so killing `aot` doesn't affect the parent session.
- **Better control:** Each server has its own socket, making session management more explicit.
- **Stability:** Nested sessions are discouraged by tmux; separate servers avoid the pitfalls.

## Design

### Constants

```rust
pub const SESSION_NAME: &str = "agents-on-tmux";
pub const SOCKET_NAME: &str = "agents-on-tmux";
```

### Command execution

All tmux commands for the nested server prepend `-L agents-on-tmux`:

```bash
# Before (default server)
tmux new-session -d -s agents-on-tmux

# After (isolated server)
tmux -L agents-on-tmux new-session -d -s agents-on-tmux
```

### Driver architecture

- `ShellCommandExecutor` gets `socket: Option<String>`
  - `None` → default tmux server (parent session)
  - `Some("agents-on-tmux")` → isolated server (nested session)
- `TmuxDriver` stores socket and passes it to executor
- `Tmux` trait gets `socket_name() -> Option<&str>`

### Control mode

`control_mode_thread` and `spawn_control_mode` accept `socket: Option<String>` and prepend `-L <socket>` when spawning `tmux -C attach-session`.

### Main initialization

```rust
// Parent driver: default server
let parent_driver = TmuxDriver::new(&parent_session);

// Nested driver: isolated server
let nested_driver = TmuxDriver::new_with_socket(SESSION_NAME, SOCKET_NAME);
```

### Socket-based detection

The guard that prevents running `aot` inside its own session now uses socket detection instead of session name detection:

- `detect_parent_socket()` parses the `TMUX` environment variable to extract the socket name
- Compares against `SOCKET_NAME` instead of `SESSION_NAME`
- Prevents running `aot` when already connected to the `agents-on-tmux` server, regardless of session name
- `detect_parent_session()` is still used for creating the parent driver (session name is still useful for targeting)

```rust
// Before: session-based check
if parent_session == SESSION_NAME {
    return Err(TmuxError::InsideOwnSession(parent_session).into());
}

// After: socket-based check
if parent_socket == SOCKET_NAME {
    return Err(TmuxError::InsideOwnServer(parent_socket).into());
}
```

## Tasks

**IMPORTANT:** After completing each task, the agent MUST:
1. Run `cargo fmt --check && cargo clippy -- -D warnings && cargo test` to verify the changes
2. Update the task tracker below (mark the task as complete with `[x]`)
3. Stop and wait for human review before proceeding to the next task
4. List the changes made in that task for the human to review

Do not continue to the next task until explicitly instructed to do so.

- [x] **Task 0:** Create plan file ✅
- [x] **Task 1:** Socket-aware executor & driver (`tmux.rs`) ✅
  - Add `socket: Option<String>` to `ShellCommandExecutor`
  - Add `socket: String` to `TmuxDriver`
  - Prepend `-L <socket>` in `execute`/`execute_inherit_stdio` when socket is set
  - Add `socket_name() -> Option<&str>` to `Tmux` trait
  - Update constructors: `new()`, `new_with_socket()`, `with_executor()`
  - Update `Default` impl
  - Update `MockCommandExecutor` to support socket
  - Update all tests
  
- [x] **Task 2:** Control mode socket support (`control_mode.rs`) ✅
  - Update `control_mode_thread(session: String, ...)` → `control_mode_thread(session: String, socket: Option<String>, ...)`
  - Update `spawn_control_mode(session: &str, ...)` → `spawn_control_mode(session: &str, socket: Option<&str>, ...)`
  - Prepend `-L <socket>` in `Command::new("tmux")` when socket is set
  - Update tests

- [x] **Task 3:** App & mock socket plumbing (`app.rs`, `ui.rs`) ✅
  - In `App::run()`, read socket from `nested_driver.socket_name()`
  - Pass socket to `control_mode_thread()`
  - Update `MockTmux` in `app.rs` tests to implement `socket_name()`
  - Update `MockTmux` in `ui.rs` tests to implement `socket_name()`

- [x] **Task 4:** Main initialization with socket (`main.rs`, `tmux.rs`) ✅
  - Import `SOCKET_NAME` constant
  - Create nested driver with socket: `TmuxDriver::new_with_socket(SESSION_NAME, SOCKET_NAME)`
  - Parent driver remains unchanged: `TmuxDriver::new(&parent_session)`
  - Add `detect_parent_socket()` function to parse TMUX env var
  - Move socket detection into `create_session_if_not_exists()` to prevent nested execution
  - Keep parent session log in main.rs, log created session/socket in `create_session_if_not_exists()`
  - Update error enum: `InsideOwnSession` → `InsideOwnServer`
  - Add tests for `detect_parent_socket()`

- [ ] **Task 5:** Update docs
  - **`AGENTS.md`:** Document socket architecture, explain `-L` usage
  - **`README.md`:** 
    - Update architecture diagram to show separate servers
    - Update "How does it work" section to mention isolated server
  - **`CHANGELOG.md`:** Add entry under `[Unreleased]`:
    ```
    ### Changed
    - Use separate tmux server (`-L agents-on-tmux`) for the nested session, providing process isolation and preventing server freezes (#20, #31)
    ```

- [ ] **Task 6:** Remove plan file
  - Delete `docs/plans/2026-09-04-separate-tmux-server.md`

## References

- [tmux man page - socket options](https://man7.org/linux/man-pages/man1/tmux.1.html)
- [overmind](https://github.com/DarthSim/overmind) - inspiration for separate server approach
- Issue #20: Killing main thread freezes tmux server
- Issue #31: Session detection fails when running from tmux popup

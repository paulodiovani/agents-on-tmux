# AGENTS.md

## Commands

- `cargo build` — build the `aot` binary
- `cargo run` — run `aot`
- `cargo run -- --tui` — run with the TUI frontend
- `cargo test` — run all tests
- `cargo test <name>` — run a single test or filter by name
- `cargo clippy -- -D warnings` — lint
- `cargo fmt --check` — format check
- Verify order: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`

## Testing

- Always write tests for new code; no untested logic should land.
- Mock the `Tmux` trait with the mockall-generated `MockTmux` (`#[cfg_attr(test, mockall::automock)]`, exported from `backends::tmux` in test builds); set expectations per test.
- Aim for 100% code coverage.
- Never run the code (`cargo run`, the `aot` binary, or real tmux commands) during
  implementation — it would interact with existing tmux sessions. Only run tests with
  mocked tmux. The user will do the final (manual) testing.

## Git operations

- Don't make write git operations (commit, push, rebase, etc.) unless explicitly asked by the user.
- Commit messages should be a single line; use bullets for additional changes only when needed.

## Architecture

Binary crate (`aot`, package name `agents-on-tmux`).

Two top-level modules under `src/`:

- `backends/` — tmux communication interface
  - `Tmux` trait defines the contract (session management, window CRUD, split_window, resize_pane, list_keys, show_options, prompt_rename_window, capture_pane, window_for_pane, send_keys, paste_text, set_window_name)
  - `NewWindow` struct holds `create_window` options (name, dir, command as program + arguments, detached)
  - `TmuxDriver<E: CommandExecutor>` implements `Tmux` with dependency injection
  - `CommandExecutor` trait abstracts tmux command execution (real `ShellCommandExecutor` + mock for tests)
  - `Window` struct represents tmux window state (id, name, running_command, started_at, notification_pending, is_active, current_dir)
  - `KeyBinding` struct represents a tmux key binding (key, command)
  - `TmuxError` enum for error handling
  - `agents.rs` identifies agents by name and command
  - `logger.rs` is a std-only global file logger (`init`/`debug`/`error`), it never writes to stdout/stderr

- `frontends/` — interfaces to AOT features
  - `tui/` terminal UI (app, event, icons, path, theme, ui)
    - `App` manages TUI state and user actions
    - `key_to_action` maps keyboard input to `Action` enum
    - `Theme` defines visual styles
    - `ui::draw` renders the interface
  - `mcp/` MCP server for agents running inside AOT
    - `server.rs` unix socket server (`$XDG_RUNTIME_DIR/aot/mcp.sock`, first instance owns it) running in a background thread of the main process
    - `shim.rs` the `aot mcp` stdio bridge agents spawn; sends the agent's `TMUX_PANE` in a handshake, then copies bytes both ways
    - `protocol.rs` hand-rolled JSON-RPC/MCP handler (`McpHandler`, `Protocol` trait), dual-era: legacy `initialize` and modern per-request `_meta`
    - `tools/` one file per tool implementing the `Tool` trait, registered in `registry.rs`; shared argument and target helpers in `tool.rs`
    - Agent-visible text (descriptions, instructions, errors) never mentions tmux or other implementation details, and tool descriptions don't point to other tools
    - Windows are addressed by their tmux window index (`Window.id`), as a plain integer

Presentation belongs to the frontend: backends never carry icons, colors, or any other display data
Colors are named ANSI slots (0-15) or `Color::Reset` only — never `Color::Rgb` or `Color::Indexed`, so the UI follows the user's terminal theme

### Separate tmux server architecture

The nested `agents-on-tmux` session runs on its own isolated tmux server using the `-L` socket option, providing process isolation and preventing the freeze issues that occur with nested sessions on the same server.

- `SESSION_NAME` constant: `"agents-on-tmux"` (the session name)
- `SOCKET_NAME` constant: `"agents-on-tmux"` (the socket/server name)
- `ShellCommandExecutor` stores `socket: Option<String>` and prepends `-L <socket>` to all tmux commands when set
- `TmuxDriver::new(session)` creates a driver for the default tmux server (used for parent session)
- `TmuxDriver::new_with_socket(session, socket)` creates a driver for a specific tmux server (used for nested session)
- `detect_parent_socket()` parses the `TMUX` environment variable to extract the current socket name
- `create_session_if_not_exists()` checks if we're already running on the same socket to prevent nested execution, returning `TmuxError::InsideOwnServer` if detected

This architecture is similar to how `overmind` works and prevents issues #20 (server freeze) and #31 (session detection failure).

Entry point: `main.rs` parses CLI args with clap (`aot mcp` runs only the MCP bridge and returns), detects the parent tmux session, creates a parent `TmuxDriver` (default server) and a nested `TmuxDriver` with socket (isolated server), ensures the nested session exists, and starts the MCP server. Without `--tui`, it splits a pane in the parent session to launch the TUI and attaches to the nested session. With `--tui`, it runs the TUI directly.

## Rust conventions (non-default — follow strictly)

- `mod.rs` contains **only** `(re)export` statements, no logic
- Custom errors live in the module that uses them; no `errors.rs` file
- All public structs must implement a trait; inter-module communication follows trait contracts
- Private by default; only expose what external modules actually use
- Module item order (top to bottom): traits → constants → enums → structs. Within each category: private before public
- Prefer std over new crates in general: every added crate grows the binary.

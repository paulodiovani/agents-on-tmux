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
  - `Tmux` trait defines the contract (session management, window CRUD, split_window, resize_pane, list_keys, show_options, command_prompt)
  - `TmuxDriver<E: CommandExecutor>` implements `Tmux` with dependency injection
  - `CommandExecutor` trait abstracts tmux command execution (real `ShellCommandExecutor` + mock for tests)
  - `Window` struct represents tmux window state (id, name, running_command, started_at, notification_pending, is_active, current_dir)
  - `KeyBinding` struct represents a tmux key binding (key, command)
  - `TmuxError` enum for error handling
  - `agents.rs` identifies agents by name and command
  - `logger.rs` is a std-only global file logger (`init`/`debug`/`error`), it never writes to stdout/stderr

- `frontends/` — terminal UI (`tui/` with app, event, icons, path, theme, ui)
  - `App` manages TUI state and user actions
  - `key_to_action` maps keyboard input to `Action` enum
  - `Theme` defines visual styles
  - `ui::draw` renders the interface

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

Entry point: `main.rs` parses CLI args with clap, detects the parent tmux session, creates a parent `TmuxDriver` (default server) and a nested `TmuxDriver` with socket (isolated server), ensures the nested session exists. Without `--tui`, it splits a pane in the parent session to launch the TUI and attaches to the nested session. With `--tui`, it runs the TUI directly.

## Rust conventions (non-default — follow strictly)

- `mod.rs` contains **only** `(re)export` statements, no logic
- Custom errors live in the module that uses them; no `errors.rs` file
- All public structs must implement a trait; inter-module communication follows trait contracts
- Private by default; only expose what external modules actually use
- Module item order (top to bottom): traits → constants → enums → structs. Within each category: private before public
- Prefer std over new crates in general: every added crate grows the binary.

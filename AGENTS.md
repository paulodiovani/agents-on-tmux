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

## Architecture

Binary crate (`aot`, package name `agents-on-tmux`).

Two top-level modules under `src/`:

- `backends/` — tmux communication interface
  - `Tmux` trait defines the contract (session management, window CRUD, split_window)
  - `TmuxDriver<E: CommandExecutor>` implements `Tmux` with dependency injection
  - `CommandExecutor` trait abstracts tmux command execution (real `ShellCommandExecutor` + mock for tests)
  - `Window` struct represents tmux window state (id, name, running_command, started_at, notification_pending, is_active, current_dir)
  - `TmuxError` enum for error handling
  - Backend is fully wired and tested

- `frontends/` — terminal UI (`tui/` with app, event, theme, ui)
  - `App` manages TUI state and user actions
  - `key_to_action` maps keyboard input to `Action` enum
  - `Theme` defines visual styles
  - `ui::draw` renders the interface

The `agents` module is planned but not yet implemented.

Entry point: `main.rs` parses CLI args with clap, detects the parent tmux session, creates a nested `TmuxDriver` for the `agents-on-tmux` session, ensures it exists. Without `--tui`, it splits a pane in the parent session to launch the TUI and attaches to the nested session. With `--tui`, it runs the TUI directly.

## Rust conventions (non-default — follow strictly)

- `mod.rs` contains **only** `(re)export` statements, no logic
- Custom errors live in the module that uses them; no `errors.rs` file
- All public structs must implement a trait; inter-module communication follows trait contracts
- Private by default; only expose what external modules actually use
- Module item order (top to bottom): traits → constants → enums → structs. Within each category: private before public

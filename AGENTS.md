# AGENTS.md

## Commands

- `cargo build` — build the `aot` binary
- `cargo run` — run `aot`
- `cargo test` — run all tests
- `cargo test <name>` — run a single test or filter by name
- `cargo clippy -- -D warnings` — lint
- `cargo fmt --check` — format check
- Verify order: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`

## Testing

- Always write tests for new code; no untested logic should land.
- Aim for 100% code coverage.

## Architecture

Binary crate (`aot`). Three primary modules planned:

- `tui` — terminal control panel (runs in its own tmux pane/window/popup)
- `tmux` — tmux communication interface
- `agents` — AI agent adapters (start, listen, remote-control)

## Rust conventions (non-default — follow strictly)

- `lib.rs` / `mod.rs` contain **only** `(re)export` statements, no logic
- Custom errors live in the module that uses them; no `errors.rs` file
- All public structs must implement a trait; inter-module communication follows trait contracts
- Private by default; only expose what external modules actually use
- Module item order (top to bottom): traits → constants → enums → structs. Within each category: private before public

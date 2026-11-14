# Code quality improvement plan

Date: 2026-11-14

Result of a review of the codebase for best practices, Rust conventions, tests,
architecture, security, and logging. Line numbers refer to the tree at commit
`5e25007`.

## Baseline

- `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test` (449 tests) pass.
- Line coverage is 92% (`cargo llvm-cov`).
- Nothing forbids panics: there is no `[lints]` table, no `clippy.toml` and no
  crate-level `#![deny]`. The only gate is default clippy with `-D warnings`.

Clippy restriction lints on non-test code:

| Lint | Hits |
|---|---|
| `unwrap_used` | 1 (`src/frontends/tui/app.rs:674`) |
| `expect_used`, `panic`, `todo`, `unimplemented`, `unreachable` | 0 |
| `string_slice` | 1 (`src/backends/tmux/commands.rs:396`) |
| `indexing_slicing` | ~37 (`ui.rs`, `path.rs`, `app.rs`, `commands.rs:317-343`, `protocol.rs`) |
| `arithmetic_side_effects` | ~60, mostly TUI layout math |

The `string_slice` hit is a real panic: `truncate` does `&s[..80]` on a byte
offset, so a malformed window line with a multibyte character at byte 80
crashes the TUI. The indexing hits that were checked are guarded by length
checks, but only by convention.

## Findings

### Security

- **Agent privilege escalation.** Any agent can run arbitrary commands outside
  its own sandbox through `send_command`, `new_window` and `new_agent`.
  `new_agent` passes `args` as-is, so it can start a peer with
  permission-bypass flags. This is by design, but there is no opt-out or
  allowlist.
- **Non-shell targets.** `send_command` only checks that the target is not an
  agent, so keys can be typed into vim, ssh or a REPL.
- **Socket fallback.** `socket_path` falls back to `/tmp/aot/mcp.sock` when
  there is no runtime or cache dir. `DirBuilder.mode(0o700)` does not fix a
  pre-existing directory, so another local user could pre-create it and
  impersonate the server. The debug log has the same fallback.
- **Server hardening** (`src/frontends/mcp/server.rs`):
  - no peer-uid check on connections;
  - the pane id in the handshake is self-reported;
  - `BufReader::lines()` has no length cap, and there is one thread per
    connection with no limit;
  - the connect-then-`remove_file` startup can race with a second instance.
- **Prompt content.** `send_prompt` pastes text unfiltered. Control characters
  could end the bracketed paste early (tmux's handling was not verified), and
  a prompt can embed a fake `[from agent N]` line.
- **Split command.** `src/main.rs:220` builds a shell string from the unquoted
  executable path and `Display for Cli`, which breaks on paths with spaces.
- **Supply chain.** CI has no `cargo audit` or `cargo deny`, and actions are
  pinned by tag rather than SHA.

### Correctness and robustness

- **Possible reconnect spin.** In `src/backends/tmux/control_mode.rs:210`, an
  EOF without `%exit` resets `retries` and reconnects with no delay. If the
  session is gone and tmux exits without `%exit`, this would respawn tmux in a
  tight loop. tmux's behaviour here was not confirmed.
- **Process table scans.** `get_pane_start_time` (`app.rs:34`) refreshes all
  processes for a single pid lookup.
- **Stale socket.** `OnSignal::Exit` calls `process::exit`, skipping
  `McpServer::drop`. The next start recovers.
- **Misleading error.** `TmuxError::WindowNotFound` is also used for parse
  failures, and `new_agent` reads it as "agent exited".
- **Config.** There is no `deny_unknown_fields`, so typos are silently ignored.
  `tmux_env` and `tui_pane` can be set from the config file, though they are
  meant to be env-only.

### Architecture and conventions (against AGENTS.md)

- `detect_parent_session` and `spawn_control_mode` call `Command::new("tmux")`
  directly, bypassing `CommandExecutor`.
- `Window.started_at` is a backend field filled in by the frontend through
  `sysinfo`.
- `app.rs` and `ui.rs` tests use hand-written `MockTmux` structs instead of
  the mockall one.
- Item order is broken in `commands.rs`, where free functions sit between
  structs and impls.
- Several public structs implement no trait: `Config`, `McpServer`, `Window`,
  `KeyBinding`, `CallContext`, `ToolOutput`, `Delivery`.
- What-comments exist in `main.rs:59`, `commands.rs:159-163,414`,
  `control_mode.rs` and `ui.rs:27-30`; impl doc comments duplicate the trait
  docs.
- Smaller items:
  - `is_agent` returns an `Option` clone;
  - `agents.rs` and `signals.rs` use `LazyLock` where a plain static would do;
  - logger calls mix `logger::` and `crate::backends::logger::` paths;
  - `Default for TmuxDriver` looks unused;
  - the three `needless_lifetimes` allows exist only for automock.
- `app.rs` (850 production lines) mixes the event loop, state, tmux hints and
  process lookup.

### Tests and CI

- Coverage gaps against the 100% goal: `main.rs` 51%, `signals.rs` 72%,
  `control_mode.rs` 73%, `app.rs` 86%, `commands.rs` 90%.
- CI does not measure coverage, and clippy does not lint test code (no
  `--all-targets`).
- There is no `[profile.release]` and no `rust-version`.

### Logging

- **MCP is nearly silent.** Protocol, shim and all 11 tools have one debug
  line between them. There is no record of which agent called which tool, with
  what target, or whether it failed.
- **tmux commands are not traced.** `ShellCommandExecutor::execute` logs
  neither argv, exit code nor duration.
- **The shim cannot log.** `aot mcp` returns before `logger::init`.
- **Lines carry little context.** The format is `<epoch-seconds> LEVEL msg`,
  with no pid. The split launcher, the TUI process and any second instance all
  append to the same `aot.log` and cannot be told apart.
- **No level filter.** `--debug` is all-or-nothing, and `info`/`error` are
  also dropped when it is off, so a crash without `--debug` leaves nothing.
- **Messages are always formatted.** Every `logger::debug(&format!(...))`
  allocates even when logging is disabled.
- **The file grows forever.** It is opened in append mode with no rotation or
  size cap.
- **Config is not logged.** The resolved config and launch mode never appear
  in the log.

## Plan

### Tracking

| Step | Status |
|---|---|
| 1. Panic gate | ✅ Done (2026-11-14, PR #72) |
| 2. Robustness fixes | ⬜ Not started |
| 3. MCP hardening | ⛔ Blocked (needs confirmation) |
| 4. Architecture | ⬜ Not started |
| 5. Tests and CI | ⬜ Not started |
| 6. Logging | ⬜ Not started |

Statuses: `⬜ Not started`, `🔄 In progress`, `⛔ Blocked (<reason>)`, `✅ Done (<date>, <commit or PR>)`.

How to update:

- When starting a step, set its status to `🔄 In progress` in the table above and
  in the step's own `Status:` line.
- When a step is complete (all its bullets landed and
  `cargo fmt --check && cargo clippy -- -D warnings && cargo test` passes),
  set both to `✅ Done` with the date and the commit or PR, in the same commit or
  PR that finishes the step.
- If a bullet is dropped or deferred, strike it through in the step and note
  why, rather than deleting it.
- Keep the table and the per-step `Status:` lines in sync.

### 1. Panic gate (small)

Status: ✅ Done (2026-11-14, PR #72)

- Add `[lints.clippy]` to `Cargo.toml` denying `unwrap_used`, `expect_used`,
  `panic`, `todo`, `unimplemented`, `unreachable`, `string_slice` and
  `indexing_slicing`.
- Add a `clippy.toml` with the `allow-*-in-tests = true` options so tests keep
  their unwraps.
- Fix `truncate` to cut on a char boundary and replace the `app.rs:674` unwrap
  with `get_or_insert_with`.
- Convert the indexing sites to slice patterns, `.get()` or
  `first()`/`last()`.
- Leave `arithmetic_side_effects` off as too noisy; use `saturating_*` in
  layout math where underflow is possible.

Outcome: the remaining subtractions in production code are guarded by explicit
checks. The one unguarded case was `Instant::now() - redraw_tick` in the TUI
run loop, now behind `redraw_due` with `checked_sub`.

### 2. Robustness fixes (small)

Status: ⬜ Not started

- Add backoff after an EOF reconnect in control mode.
- Add `TmuxError::UnexpectedOutput` for parse failures.
- Add `deny_unknown_fields` and mark the env-only config fields
  `#[serde(skip)]`.
- Build the split command from shell-quoted argv, reusing `shell_quote`.

### 3. MCP hardening (medium, needs confirmation)

Status: ⛔ Blocked (needs confirmation)

This step changes agent-visible behaviour and must be confirmed before work
starts.

- Refuse to start the server if the socket directory is not owned by the user
  with mode 0700, and drop the `/tmp` fallback.
- Cap line length and concurrent connections.
- Strip control characters other than `\n` and `\t` from prompts.
- Make `send_command` require a known shell in `running_command`.
- Add a config switch to disable mutating tools or allowlist `new_agent` args.

### 4. Architecture (medium)

Status: ⬜ Not started

- Route `detect_parent_session` and control-mode spawning through
  `CommandExecutor` or a spawner trait.
- Move the process start-time lookup into `backends/` behind a trait, and
  consider replacing `sysinfo` with a `/proc/<pid>` read to shrink the binary.
- Split `app.rs` into state, run loop and hints.
- Extract `main()` into a testable `run(config, deps)`.
- Reorder items and remove the what-comments.

### 5. Tests and CI (small to medium)

Status: ⬜ Not started

- Replace the hand-written mocks with mockall's `MockTmux`.
- Fix the `type_complexity` warning in the `app.rs` tests (`test_app`), the
  only thing `clippy --all-targets -D warnings` currently reports.
- Cover `main`, `signals` and `control_mode`.
- Add `cargo llvm-cov --fail-under-lines`, `clippy --all-targets`,
  `cargo audit` or `cargo deny`, and SHA-pinned actions.
- Add `[profile.release]` (`lto`, `strip`, `codegen-units = 1`) and
  `rust-version`.

### 6. Logging (small to medium, std-only)

Status: ⬜ Not started

- Trace every tmux command in the executor at debug level: argv, exit code,
  elapsed ms, and stderr on failure. Truncate `set-buffer` and `send-keys`
  payloads so prompt text does not land in the log.
- Log each MCP `tools/call` in `McpHandler::call_tool`: caller pane, tool
  name, target index, ok or error, and duration. Log connection open and close
  in `server.rs`. Argument values stay out, only their sizes.
- Add pid and a role tag (`main`, `tui`, `mcp`, `shim`) to each line, and
  switch the timestamp to millisecond precision.
- Make the calls lazy with an `enabled()` check or a small `log_debug!` macro,
  so nothing is formatted when logging is off.
- Initialise the logger in the shim when `AOT_DEBUG` is set.
- Truncate or rotate once at startup when the file exceeds a size cap (for
  example keep one `aot.log.old`).
- Log the resolved config and launch mode once at startup.

## Open decisions

- **Step 3 scope.** Shell-only `send_command` and a tool or args allowlist
  change what agents can do.
- **Errors without `--debug`.** Writing errors by default would make crashes
  diagnosable, but aot would always create a file in the cache dir. The
  default proposal is to keep logging opt-in.

## Suggested order

Steps 1 and 2 first (safe, small), then 6 and 5, then 4. Step 3 waits for the
decision above.

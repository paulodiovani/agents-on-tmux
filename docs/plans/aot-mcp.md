# AOT MCP: let agents drive other windows

Branch: `feature/aot-mcp`

## Goal

Agents running inside AOT's nested tmux session can list, create, read, and send input to other windows/agents through an MCP server exposed by AOT. AOT never configures agents; the user adds the MCP entry to each agent. While AOT runs, the server is available.

## Decisions

| Topic | Decision |
|---|---|
| Transport | Unix socket server inside AOT + `aot mcp` stdio shim spawned by each agent |
| Server process | Background thread of the main `aot` process, started before attach/TUI in every launch mode, removed on exit |
| Window reference | The tmux window **index** (`#{window_index}`, stored as `Window.id` in the backend, shown in the TUI and status bar), passed as a plain integer (e.g. `3`), never with `@` (in tmux `@N` is the stable window id, which AOT does not use). Titles are output only. Indexes may change (e.g. window moved); agents deal with it (re-list, or the user tells them). The sender's index is resolved from its pane on every call, never cached |
| list format | JSON object wrapping the array (e.g. `{"windows": [...]}`, since legacy clients require `structuredContent` to be an object), sent as `structuredContent` + same JSON as text content |
| read format | Plain text (raw captured pane) |
| Sender identity | The shim runs inside the agent's pane, so it inherits `$TMUX_PANE`; it sends it in a handshake, and the server maps pane → window index. No user config, no stored table |
| MCP protocol era | Dual-era: legacy `initialize` handshake (negotiates 2024-11-05 … 2025-11-25) and modern 2026-07-28 (per-request `_meta` protocol version + client capabilities, `server/discover`). Stateless either way |
| Dependencies | Add `serde_json` only (serde already present). JSON-RPC/MCP is hand-rolled on std; no `rmcp`/HTTP/async runtime, to keep the binary small |
| Architecture rules | The MCP server is a new frontend: `frontends/mcp/` (server, shim, tools), a sibling of `frontends/tui/`, since it is another interface to AOT features. `mod.rs` re-exports only; traits first; public structs implement a trait; tmux access goes only through the backend `Tmux` trait (mockable). Presentation/protocol concerns (JSON shapes, tool descriptions) stay in the frontend; the backend gains only generic tmux operations. `main.rs` wires the frontend like it does the TUI |

### Wire design

- Agent config: `{"command": "aot", "args": ["mcp"]}` (stdio, newline-delimited JSON-RPC per MCP spec).
- `aot mcp` (shim): connects to the socket, sends one handshake line `{"pane":"<$TMUX_PANE>"}`, then copies stdin↔socket byte for byte. It contains no MCP logic. If AOT is not running it exits with a clear stderr message.
- Server: per-connection thread; handles `initialize`, `server/discover`, `ping`, `tools/list`, `tools/call`; notifications get no reply. Modern requests with an unsupported version get -32022, missing required `_meta` fields get -32602. Tool errors return `isError: true` with a message, not JSON-RPC errors.
- Socket path: `$XDG_RUNTIME_DIR/aot/mcp.sock`, fallback `<cache_dir>/aot/mcp.sock`; mode 0600; unlinked on shutdown by its owner.
- Single owner: all AOT instances share the same nested tmux server, so one MCP server is enough (it is stateless; tmux holds the state). On start, AOT tries to connect to the socket first:
  - connection succeeds → another instance owns it; skip starting a server and log it (a live socket is never replaced);
  - connection refused / file missing → remove the stale file and bind.
  - Only the owner unlinks the socket on shutdown.
- No takeover in v1: if the owner exits while other instances run, the MCP is unavailable until AOT restarts, and the shim reports "AOT MCP not running". Revisit (background rebind retry) only if multi-instance use turns out to matter.
- When AOT exits (or restarts) while an agent is connected, the shim exits with code 1 and "AOT closed the connection (did AOT exit?)"; it exits 0 only when the client closes stdin. Agents reconnect their MCP manually; transparent reconnect was considered and rejected.
- Verify against the current MCP spec (lifecycle + tools + stdio) at implementation time.

### Tmux backend additions (`Tmux` trait + `TmuxDriver`)

- `create_window(&NewWindow { name, dir, command: Vec<String>, detached })` (`new-window -P -F '#{window_index}' [-d] [-n name] [-c dir] [program args...]`; with several entries tmux runs the program directly, no shell parsing), returning the window found by the printed index; the TUI passes only the name, keeping its behavior.
- `capture_pane(index, history)` → `capture-pane -p -J -t <session>:<index> -S -<history>` (visible screen plus `history` scrollback lines); `read_window` drops trailing blank lines and keeps the last `lines`.
- `send_keys(index, text, enter)` → `send-keys -t <session>:<index> -l -- <text>` then `send-keys ... Enter` (literal mode and `--` so text is never interpreted as key names or flags; empty text sends only Enter).
- `set_window_name(index, name)` → `rename-window -t <session>:<index> -- <name>` (direct, unlike the TUI's interactive `prompt_rename_window`, renamed from `rename_window`).
- `paste_text(index, text)` → `set-buffer -b aot-paste-<pid>-<n> -- <text>` then `paste-buffer -b <name> -p -d -t <session>:<index>` (bracketed paste, unique buffer deleted after use).
- `window_for_pane(pane_id)` → `display-message -p -t %N '#{window_index}'`.
- `Window` already carries `running_command` and `current_dir`; agent vs. plain window decided with `agents::is_agent`.

### Tools

All take/return window indexes (field/arg `index`, integer). Names are the final proposal; descriptions are written for the controlling LLM and say "use indexes, not titles; resolve titles via list tools".

| Tool | Args | Result |
|---|---|---|
| `list_windows` | – | `{windows: [...]}` of non-agent windows: `{index, title, command, cwd, active}` |
| `list_agents` | – | `{agents: [...]}` of agent windows: `{index, title, agent, command, cwd, active}` |
| `read_window` | `index`, `lines` (default 50, max 2000) | plain text |
| `read_agent` | same | alias of `read_window` |
| `new_window` | `dir?` (default AOT's PWD; relative paths resolve from it; must exist) | `{index, title, cwd}`; created detached so the user's view does not switch |
| `new_agent` | `command?` (enum of known agent commands; default: caller's agent if the caller is an agent, else error asking for it), `args?` (string array passed to the agent as-is), `dir?` (default AOT's PWD) | `{index, title, agent, cwd}`; created detached; after ~500 ms the window (same index and pane pid) must still exist, else a tool error (agent not installed or failed on startup) |
| `rename_window` | `index`, `title` (single line) | `{index, title}`; renames any window, including the caller's own |
| `rename_agent` | same | alias of `rename_window` |
| `send_command` | `index`, `command` (array: program then arguments; each element shell-quoted only when needed and joined, so no shell syntax is interpreted) | confirmation; types the command and Enter |
| `send_prompt` | `index`, `prompt` | gives another agent a prompt, delivered as `[from agent N] <prompt>` then Enter; no response is implied; sender (N = caller's window index) resolved from the connection's pane on each call and must be an agent window |
| `respond_prompt` | `index` (N of the `[from agent N]` tag being responded to), `prompt` | responds, when needed, to a prompt previously received from agent N; same signed delivery as `send_prompt`, different purpose |

Guardrails: unknown/non-nested indexes → tool error; `send_*` refuse to target the caller's own window (when the caller's pane is known); `send_command` only targets non-agent windows and `send_prompt`/`respond_prompt` only agent windows, with errors pointing to the right tool; `lines` clamped.

Settled (before task 8): prompts are delivered with bracketed paste (`set-buffer` + `paste-buffer -p -d`) so newlines stay in one prompt; a fixed ~200 ms delay separates the pasted text from Enter; both `send_prompt` and `respond_prompt` sign prompts with `[from agent N]` and add no footer (revised after task 10); a prompt does not necessarily need a response.

## Workflow

1. This plan → pause.
2. Each task below is a separate commit, then pause for manual testing. Verify before each commit: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`. No running `aot` or real tmux during implementation (mocked tests only).
3. Tests required for all new code; aim for 100% coverage.

## Tasks tracker

- [x] 0. Plan reviewed
- [x] 1. MCP core: `serde_json` dep, JSON-RPC/MCP handler with empty tool registry (`initialize`, `ping`, `tools/list`), tool trait + registry, unix socket server thread started from `main` (first-instance-owns-socket, tests for live/stale socket), `aot mcp` shim subcommand, handshake + pane capture
- [x] 2. `list_windows`
- [x] 3. `list_agents`
- [x] 4. `read_window` (+ `capture_pane`)
- [x] 5. `read_agent` (alias)
- [x] 6. `new_window` (+ `create_window` cwd/command)
- [x] 7. `new_agent` (+ `window_for_pane`, moved up from task 10 for the caller's default command)
- [x] 8. `send_command` (+ `send_keys`)
- [x] 9. `send_prompt` (+ `paste_text`)
- [x] 10. `respond_prompt` (sender resolution via `window_for_pane`)
- [x] 11. `rename_window` (+ `set_window_name`)
- [x] 12. `rename_agent` (alias)
- [ ] 13. Docs: user manual page with per-agent MCP config snippets, README mention, `AGENTS.md` architecture section (add `frontends/mcp/`)

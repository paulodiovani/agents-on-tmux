# AOT MCP: let agents drive other windows

Branch: `feature/aot-mcp`

## Goal

Agents running inside AOT's nested tmux session can list, create, read, and send input to other windows/agents through an MCP server exposed by AOT. AOT never configures agents; the user adds the MCP entry to each agent. While AOT runs, the server is available.

## Decisions

| Topic | Decision |
|---|---|
| Transport | Unix socket server inside AOT + `aot mcp` stdio shim spawned by each agent |
| Server process | Background thread of the main `aot` process, started before attach/TUI in every launch mode, removed on exit |
| Window reference | AOT's window id (`Window.id`, the tmux window index shown in the TUI), written `@N` (bare `N` also accepted). Titles are output only. Ids may change (e.g. window moved); agents deal with it (re-list, or the user tells them). The sender's id is resolved from its pane on every call, never cached |
| list format | JSON object wrapping the array (e.g. `{"windows": [...]}`, since legacy clients require `structuredContent` to be an object), sent as `structuredContent` + same JSON as text content |
| read format | Plain text (raw captured pane) |
| Sender identity | The shim runs inside the agent's pane, so it inherits `$TMUX_PANE`; it sends it in a handshake, and the server maps pane → window id. No user config, no stored table |
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
- Verify against the current MCP spec (lifecycle + tools + stdio) at implementation time.

### Tmux backend additions (`Tmux` trait + `TmuxDriver`)

- `create_window` gains cwd and optional command (`new-window -d -P -F ... -c <dir> [command]`); existing TUI caller keeps current behavior.
- `capture_pane(id, lines)` → `capture-pane -p -J -t @N -S -<lines>`.
- `send_keys(id, text, enter)` → `send-keys -t @N -l <text>` then `Enter` (literal mode so text is never interpreted as key names).
- `window_for_pane(pane_id)` → `display-message -p -t %N '#{window_id}'`.
- `Window` already carries `running_command` and `current_dir`; agent vs. plain window decided with `agents::is_agent`.

### Tools

All take/return window ids. Names are the final proposal; descriptions are written for the controlling LLM and say "use ids, not titles; resolve titles via list tools".

| Tool | Args | Result |
|---|---|---|
| `list_windows` | – | `{windows: [...]}` of non-agent windows: `{id, title, command, cwd, active}` |
| `list_agents` | – | `{agents: [...]}` of agent windows: `{id, title, agent, command, cwd, active}` |
| `read_window` | `id`, `lines` (default 50, max 2000) | plain text |
| `read_agent` | same | alias of `read_window` |
| `new_window` | `dir?` (default AOT's PWD) | `{id, title, cwd}` |
| `new_agent` | `command?`, `dir?` (defaults: caller's agent command if the caller is an agent, else error asking for it; AOT's PWD) | `{id, title, agent, cwd}` |
| `send_command` | `id`, `command` | confirmation; types the command and Enter |
| `send_prompt` | `id`, `prompt` | confirmation; prompt delivered + Enter |
| `respond_prompt` | `id`, `prompt` | like `send_prompt`, prefixed with sender (`[from window @N] ...`) and a hint on how to reply; sender resolved from the connection's pane |

Guardrails: unknown/non-nested ids → tool error; `send_*` refuse to target the caller's own window; `lines` clamped.

Open points to settle during implementation (ask before deciding): multi-line prompts (plain `send-keys -l` vs. bracketed paste via `load-buffer`/`paste-buffer -p`), whether `send_prompt` also gets the sender prefix, and per-agent submit delay.

## Workflow

1. This plan → pause.
2. Each task below is a separate commit, then pause for manual testing. Verify before each commit: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`. No running `aot` or real tmux during implementation (mocked tests only).
3. Tests required for all new code; aim for 100% coverage.

## Tasks tracker

- [x] 0. Plan reviewed
- [x] 1. MCP core: `serde_json` dep, JSON-RPC/MCP handler with empty tool registry (`initialize`, `ping`, `tools/list`), tool trait + registry, unix socket server thread started from `main` (first-instance-owns-socket, tests for live/stale socket), `aot mcp` shim subcommand, handshake + pane capture
- [x] 2. `list_windows`
- [ ] 3. `list_agents`
- [ ] 4. `read_window` (+ `capture_pane`)
- [ ] 5. `read_agent` (alias)
- [ ] 6. `new_window` (+ `create_window` cwd/command)
- [ ] 7. `new_agent`
- [ ] 8. `send_command` (+ `send_keys`)
- [ ] 9. `send_prompt`
- [ ] 10. `respond_prompt` (+ `window_for_pane` sender resolution)
- [ ] 11. Docs: user manual page with per-agent MCP config snippets, README mention, `AGENTS.md` architecture section (add `frontends/mcp/`)

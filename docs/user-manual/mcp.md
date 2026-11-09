## AOT MCP Server

AOT exposes an [MCP](https://modelcontextprotocol.io/) server that lets agents running inside AOT orchestrate other windows and agents: list them, start new ones, read their output, run commands, and send prompts to each other.

The server runs inside the main `aot` process while AOT is running. Agents connect to it through the `aot mcp` command, a small stdio bridge that each agent starts as a regular MCP server.

AOT never changes agent configurations; add the MCP server to each agent you want to use it.

### Requirements

- AOT is running, and the agent runs in a window of the AOT session.
- The `aot` binary is in the agent's `PATH` (or use its absolute path as the command).

### Configuration

Every agent uses the same stdio server: command `aot`, arguments `mcp`. Only the configuration format changes.

#### Claude Code

Project scope, in `.mcp.json` at the project root:

```json
{
  "mcpServers": {
    "aot": {
      "command": "aot",
      "args": ["mcp"]
    }
  }
}
```

Or add it for all projects:

```sh
claude mcp add --scope user aot -- aot mcp
```

#### OpenCode

Project scope, in `opencode.jsonc` at the project root (see [OpenCode MCP servers](https://opencode.ai/docs/mcp-servers/)):

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "aot": {
        "type": "local",
        "command": ["aot", "mcp"]
      }
    }
  }
}
```

Use `opencode mcp add --global` to make it available in every project.

#### Pi

User scope, in `~/.pi/agent/mcp.json` (project scope: `.pi/mcp.json`):

```json
{
  "mcpServers": {
    "aot": {
      "command": "aot",
      "args": ["mcp"]
    }
  }
}
```

Or add it with `pi mcp add aot -- aot mcp`. Run `/reload` in a running session to pick up the change.

#### Other agents

Configure a local (stdio) MCP server named `aot` that runs `aot` with the single argument `mcp`, following the agent's MCP documentation.

### Tools

Windows and agents are addressed by their **index**, the number shown in the TUI, never by title. Indexes can change when windows are moved; agents list again when unsure.

| Tool             | Description                                                                              |
| ---------------- | ---------------------------------------------------------------------------------------- |
| `list_windows`   | List non-agent windows: index, title, command, working directory, active                 |
| `list_agents`    | List agents: index, title, agent, command, working directory, active                     |
| `read_window`    | Read the latest output of a window (default 50 lines, max 2000)                          |
| `read_agent`     | Read the latest output of an agent                                                       |
| `new_window`     | Open a new shell window, optionally in a given directory                                 |
| `new_agent`      | Start a new agent (defaults to the caller's agent), with optional arguments and directory |
| `rename_window`  | Change the title of a window                                                             |
| `rename_agent`   | Change the title of an agent, including the caller itself                                |
| `send_command`   | Run a command (program and arguments) in a non-agent window                              |
| `send_prompt`    | Give a prompt to another agent                                                           |
| `respond_prompt` | Respond to a prompt received from another agent                                          |

Notes:

- New windows and agents open in the background; your current view does not switch.
- `new_agent` reports an error when the agent exits right after starting (for example, when it is not installed).
- `send_command` passes each argument as-is; shell syntax such as pipes or redirection is not interpreted.
- Agents cannot send commands or prompts to their own window.

### Prompts between agents

Prompts sent with `send_prompt` and `respond_prompt` are signed with the sender's index:

```
[from agent 2] Please review the changes in src/main.rs
```

The receiving agent knows who sent the prompt and, when a response is needed, answers with `respond_prompt` using that index. Not every prompt needs a response.

Sending and responding to prompts requires AOT to identify the sending agent, which works when the agent starts `aot mcp` itself, from its own window. OpenCode starts MCP servers from a shared background service instead, so OpenCode agents can receive prompts but cannot send or respond to them.

### Multiple AOT instances

All AOT instances share the same session, so a single MCP server is enough: the first instance owns it and the others skip starting one. If the owner exits while others keep running, the MCP server is unavailable until AOT is started again.

### Troubleshooting

| Message                                       | Meaning                                                                                       |
| --------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `AOT MCP not running (no server at ...)`      | AOT is not running. Start AOT, then reconnect the agent's MCP server.                         |
| `AOT closed the connection (did AOT exit?)`   | AOT exited or was restarted after the agent connected. Reconnect the agent's MCP server.      |
| `You are not running inside AOT, ...`         | The agent was not started in a window of the AOT session, so it cannot be identified.         |

Run AOT with `--debug` to log MCP server activity to the AOT log file.

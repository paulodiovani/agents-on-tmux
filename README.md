# Agents on TMUX

A TMUX-based AI Agents orchestrator.

## Design

`agents-on-tmux`, or just `aot`, works as a thin wrapper over tmux and a TUI control panel. Its features are divided into three primary modules:

- `tui` The terminal interface provides a control panel for running agents, including actions to focus, stop, or start a new agent.
  The TUI run on its own TMUX pane, window, or popup, it does not highjack or wrap your terminal to run agents inside its interface.
- `tmux` The TMUX communication interface, allowing to start and control a dedicated TMUX session.
- `agents` The `agents` interface, supporting popular terminal-based AI Agents and interact with them.

## Requirements

- [TMUX](https://tmux.app/)

## How does it work

1. Start the application with `aot`.
1. The application checks if a parent TMUX session is running or stops if not.
2. Start a new dedicated TMUX session named `agents-on-tmux`, this is the session that will host the agents.
3. The TUI control panel is started by default on a left panel. Can also be started with `aot --tui`.
4. User can interact with the dedicated session using the TUI control or standard TMUX mappings.

## Screenshots

TBD

## Supported Agents

| AI Agent      | Start | Listen | Remote Control |
| :------------ | :---: | :----: | :------------: |
| Generic / Any |  ✔️   |   -    |       -        |

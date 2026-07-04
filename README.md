# Agents on TMUX

A TMUX-based AI Agents orchestrator.

## Design

`agents-on-tmux`, or just `aot`, works as a thin wrapper and TUI over tmux. Its features are divided into three primary modules:

- `tui` The terminal interface provides a control panel for running agents, including actions to focus, stop, or start a new agent.
  The TUI run on its own TMUX pane, window, or popup, it does not highjack your terminal or run agents inside its interface.
- `tmux` The TMUX communication interface, allowing to start and control a dedicated session.
- `agents` The `agents` interface, supporting popular terminal-based AI Agents and interact with them.

## Screenshots

TBD

## Supported Agents

| AI Agent      | Start | Listen | Remote Control |
| :------------ | :---: | :----: | :------------: |
| Generic / Any |  ✔️   |   -    |       -        |

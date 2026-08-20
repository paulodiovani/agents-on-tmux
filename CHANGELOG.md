# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `--tui-width` CLI flag and `tui_width` config option to set the TUI side panel width in columns (default: 35)
- `,` (rename) keybinding in the TUI, renaming the selected window through a tmux command prompt pre-filled with its current name; also advertised in the unfocused footer hints (`C-b C-b , rename`)
- The TUI footer now switches hints by pane focus: its own keybindings while the TUI pane is focused, and the user's tmux bindings for the nested session (e.g. `C-b C-b c new`, read from `list-keys`) when it is not. Requires `focus-events on` (see [recommended tmux config](docs/recommended-tmux-config.md)); without it the footer keeps showing the TUI keybindings

### Changed
- Redesigned the TUI with compact two-line rows, a new tab bar, full-width background selection, and right-aligned paths
- The TUI new-window key is now `c` (was `n`), matching tmux's own binding

### Fixed
- The side panel now retains its width when the terminal or window is resized (only when started in normal mode, with both the TUI pane and the nested session)

## [0.3.0] - 2026-08-09

### Added
- `--debug` flag (also `AOT_DEBUG` env var) to enable file logging to `~/.cache/aot/aot.log`

### Changed
- Replaced 5-second polling with event-driven updates via tmux control mode (`tmux -C`), so the TUI reacts immediately to window changes, renames, and selection

### Known Issues
- The window activity indicator (`!`) no longer updates live. tmux control mode has no push notification for `window_activity_flag`, so it now only refreshes when something else triggers a redraw (navigating, adding/closing/renaming a window). Requires `monitor-activity on` (see [recommended tmux config](docs/recommended-tmux-config.md)); minor regression, accepted for now.

## [0.2.0] - 2026-07-12

### Added
- Agent Icons support using Nerd font or Font Awesome
- User config file at `~/.config/aot/aot.conf`, also supporting env vars and CLI options
- Include Copilot agent

### Changed
- Updated README and added installation instructions

## [0.1.4] - 2026-07-09

### Added
- `--version`/`-V` flag to display version information

### Fixed
- Creating a new window now properly focuses it and returns to the TUI pane

## [0.1.3] - 2026-07-09

### Added
- Switch to last-pane when selecting a window from the TUI, returning focus to the main pane

### Changed
- When switching tabs, selection now moves to the window in the same directory

## [0.1.2] - 2026-07-07

### Fixed
- Elapsed time now shows days and hours, as long as minutes and seconds

## [0.1.1] - 2026-07-06

### Changed
- Refactored module structure to use module-based imports instead of flat re-exports
- Sorted struct fields, enum variants, and match arms alphabetically for consistency
- Sorted tmux list-windows format string fields alphabetically

## [0.1.0] - 2026-07-06

### Added
- Terminal UI with left pane control panel and right pane for agent/window management
- Tabbed interface separating Agents and Windows into distinct tabs
- Window management: list, select, kill with double-press confirmation
- Window display with ID, name, running command, start time, and current directory
- Auto-select new windows when added through the TUI
- Scrollable TUI for handling many windows
- Live sync with external tmux session changes
- Session auto-start/attach on application startup
- CLI argument parsing with `--tui` flag
- Nested session detection to prevent startup loops

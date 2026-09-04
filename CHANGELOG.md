# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-09-04

### Added
- `--tui-width` CLI flag and `tui_width` config option to set the side panel width (default: 35)
- `,` keybinding to rename the selected window via tmux command prompt
- Context-aware footer: shows TUI keys when focused, tmux bindings when the nested session is focused

### Changed
- Display the window id before the name (e.g. `3 my-window`); replaced by `*` for the active window
- Redesigned the TUI with compact two-line rows, a tab bar, full-width selection, and right-aligned paths
- New-window key changed from `n` to `c`, matching tmux's default binding

### Fixed
- Side panel retains its width on terminal resize
- Selection no longer jumps to the active window on refresh when the TUI is focused

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

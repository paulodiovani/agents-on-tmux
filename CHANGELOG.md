# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
- Basic agent support with detection and management of supported AI coding agents
- Tabbed interface separating Agents and Windows into distinct tabs
- Double-press confirmation for destructive actions (quit and kill window)

## [0.1.0-beta.1] - 2026-07-06

### Fixed
- Prevent startup infinite loop by detecting and rejecting nested `aot` runs inside its own dedicated session
- Switch error handling from `Box<dyn Error>` to `anyhow` for cleaner error propagation

## [0.1.0-alpha.7] - 2026-07-06

### Added
- Auto-select new windows when added through the TUI
- Window/agent counter display
- Current directory display for each window
- Scrollable TUI for handling many windows
- Live sync with external tmux session changes

### Changed
- Bump crossterm from 0.28.1 to 0.29.0

## [0.1.0-alpha.6] - 2026-07-05

### Fixed
- Parent session detection now checks `TMUX` environment variable to correctly determine if running inside tmux

## [0.1.0-alpha.5] - 2026-07-05

### Changed
- Cleaned up `is_running` function and removed unused `send_keys` method

### Added
- Screenshot to README

## [0.1.0-alpha.4] - 2026-07-05

### Added
- GitHub Actions workflows for lint and test
- Dependabot configuration
- Pull request template

## [0.1.0-alpha.3] - 2026-07-04

### Changed
- Startup now launches both TUI and tmux session together
- Uses current app name and path for session management

## [0.1.0-alpha.2] - 2026-07-04

### Added
- `CommandExecutor` trait for testable tmux command execution
- Session auto-start/attach on application startup
- TUI auto-refresh every 5 seconds

### Changed
- Removed `ctrlc` dependency

## [0.1.0-alpha.1] - 2026-07-04

### Added
- Terminal UI with ratatui/crossterm
- Tmux backend with trait-based contract
- CLI argument parsing with clap (`--tui` flag)
- Window management: list, select, kill with confirmation
- Window display with ID, name, running command, and start time
- Full module documentation

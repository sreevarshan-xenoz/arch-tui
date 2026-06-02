# metapak - Developer Context

metapak is a modern, terminal-based user interface for managing packages across multiple operating systems. It provides a unified experience for searching, installing, and removing packages across multiple package managers (pacman, AUR helpers, apt, brew, dnf, apk, scoop) and language ecosystems (npm, pip, cargo).

## 🚀 Project Overview

- **Main Technologies:** Rust, Ratatui (TUI rendering), Tokio (Async runtime), Crossterm (Terminal handling), Reqwest (HTTP client).
- **Core Purpose:** To provide a fast, safe, and visually appealing way to manage system packages directly from the terminal.
- **Key Features:** Universal search (System Repos + Language Ecosystems), batch operations, live search, system diagnostics, btrfs snapshots/rollbacks, hooks execution, and a robustness suite.

## 🛠️ Architecture

The project follows a modular architecture designed for extensibility and safety:

- **`src/main.rs`**: Entry point. Sets up the terminal, initializes the application state, spawns background tasks for async actions, and manages the main event loop.
- **`src/app.rs`**: Contains the `App` struct, which holds the entire application state (results, selection, loading state, history, etc.).
- **`src/backends/`**: Implements the `PackageProvider` and `UpdateProvider` traits for different package managers (`pacman`, `apt`, `brew`, `apk`, `scoop`, `npm`, `pip`, etc.) under the `providers/` subdirectory. Also contains snapshot providers (`btrfs`, `timeshift`).
- **`src/diagnostics.rs`**: Gathers system health and hardware information using the `sysinfo` crate for cross-platform compatibility.
- **`src/ui.rs`**: Modularized UI components using Ratatui.
- **`src/action.rs` & `src/models.rs`**: Define the internal message passing (Actions) and data structures (Packages).

## 🏗️ Building and Running

### Prerequisites
- Rust toolchain (`cargo`, `rustc`)
- System dependencies: `pacman`, `inotify` (on Linux)

### Commands
- **Run:** `cargo run`
- **Build:** `cargo build --release`
- **Test:** `cargo test`
- **Lint:** `cargo clippy`
- **Format:** `cargo fmt`

## 📝 Development Conventions

- **Async First:** Most I/O and backend operations are asynchronous using `tokio`. Use `tokio::spawn` for non-blocking tasks.
- **Trait-Based Backends:** New package managers should implement the `UniversalPackageManager` trait in `src/backends/`.
- **Action-Driven State:** The TUI state is updated via `Action` objects sent through an async channel. This keeps UI rendering decoupled from backend logic.
- **Robustness Suite:**
    - **Simulation:** Use `SimulationEngine` in `src/simulation.rs` to dry-run operations.
    - **Watchdog:** `HealthWatchdog` in `src/watchdog.rs` monitors application health.
    - **Snapshots:** Use `SnapshotProvider` in `src/backends/snapshots/` for filesystem safety.
- **Error Handling:** Use `thiserror` for library-level errors and `anyhow` for application-level context. Custom error types are in `src/errors.rs`.
- **Theming:** Centralized theme configuration in `src/theme.rs`.

## ⚙️ Configuration

- **Default Config:** `config/default.toml`
- **User Config:** `~/.config/metapak/config.toml`
- Configuration is loaded using the `config` crate and managed in `src/config.rs`. Supports `[ui]`, `[search]`, `[robustness]`, `[hooks]`, `[i18n]`, `[notifications]`, and `[telemetry]` sections.

## 🧪 Testing

- **Unit Tests:** Located within source files (e.g., `src/config.rs`).
- **Integration Tests:** Located in `tests/`, covering snapshots, robustness, and simulation.
- **Simulation:** A dedicated simulation engine allows testing UI flows without modifying the actual system.
- **CI Pipeline:** Runs cross-platform matrix on Ubuntu, macOS, and Windows for formatting, linting, and testing.

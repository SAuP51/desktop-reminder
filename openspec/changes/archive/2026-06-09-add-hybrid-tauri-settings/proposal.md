# Add Hybrid Tauri Settings Architecture

## Summary

Build a balanced V2 desktop architecture for the reminder application:

- `reminder-agent.exe` remains the lightweight native Rust process that runs continuously.
- `reminder-ui.exe` is a Tauri v2 settings application launched only when needed.
- The agent owns scheduling, SQLite mutations, overlay dispatch, native tray, and local IPC.
- The UI provides graphical configuration, preview, test, and history workflows through IPC.

## Motivation

The project goal is a low-resource Windows desktop reminder app. A full-time webview process would make configuration easier but works against the idle-resource goal. A pure CLI/native approach is lightweight but weak for day-to-day configuration.

The chosen hybrid model keeps the steady-state runtime native and small, while allowing a richer Tauri settings experience on demand.

## Scope

In scope:

- Replace fixed polling-style scheduler waiting with a WaitableTimer backend on Windows.
- Add native agent tray entry points.
- Add shared IPC between agent, CLI, and settings UI.
- Add Tauri v2 settings app in the Rust workspace.
- Keep Task Scheduler startup pointed at the native agent only.
- Reuse existing core, storage, and overlay crates.
- Document validation prerequisites and current local build-tool limitations.

Out of scope:

- Auto-update.
- Cloud sync.
- Account system.
- Enterprise service mode.
- Using Tauri as the scrolling overlay renderer.
- Starting the Tauri UI automatically at login.

## Outcome

The implementation now contains:

- Native agent scheduler loop with reload/pause/test/open-settings commands.
- Windows WaitableTimer backend with non-Windows fallback timer thread.
- Windows tray module with Open Settings, Test Reminder, Pause 30m, Resume, Next Reminder, and Exit.
- Shared IPC crate with Windows named pipe transport and non-Windows TCP fallback.
- IPC-first CLI behavior.
- Tauri settings app with reminder list, editor, schedule preview, display settings, test overlay, basic history, and start-agent affordance.
- Validation script updates that diagnose missing Windows native build prerequisites.

## Validation Notes

Verified in the current workspace:

- `rustc 1.96.0` and `cargo 1.96.0` are available.
- `cargo fmt --all -- --check` passes.
- `apps/reminder-ui` production build passes.
- Full `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` are blocked before crate code by missing MSVC linker and Windows SDK import libraries (`link.exe`, `kernel32.lib` in `LIB`).

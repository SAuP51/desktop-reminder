# Tasks

## 1. Native Agent

- [x] Introduce a scheduler timer abstraction.
- [x] Add Windows WaitableTimer backend.
- [x] Keep a non-Windows fallback timer for development.
- [x] Add reload signaling into the scheduler loop.
- [x] Rebuild the queue from current time on resume/pause expiry.
- [x] Add native Windows tray module.
- [x] Add tray commands: Open Settings, Test Reminder, Pause 30m, Resume, Next Reminder, Exit.
- [x] Add single-instance behavior based on IPC status detection.

## 2. IPC

- [x] Add shared `reminder-ipc` crate.
- [x] Define JSON request/response protocol.
- [x] Implement Windows named pipe transport.
- [x] Keep non-Windows TCP fallback transport.
- [x] Route create/update/delete/enable/disable through the agent.
- [x] Add preview, test, pause/resume, history, reload, shutdown, and open-settings commands.

## 3. CLI

- [x] Update CLI to prefer IPC when the agent is running.
- [x] Keep direct SQLite fallback for offline/diagnostic paths.
- [x] Add CLI commands for status, history, test overlay, open settings, pause/resume, reload, and shutdown.

## 4. Tauri Settings App

- [x] Scaffold Tauri v2 settings app as `reminder-ui`.
- [x] Use React and TypeScript frontend.
- [x] Add Tauri backend commands that call agent IPC.
- [x] Add single-instance focus behavior for the settings app.
- [x] Add reminder list and editor workflows.
- [x] Add once, daily, and interval rule editing.
- [x] Add schedule preview.
- [x] Add display settings and test overlay request.
- [x] Add basic history view.
- [x] Add agent unavailable state and start-agent action.

## 5. Lifecycle

- [x] Keep Task Scheduler autostart pointed at `reminder-agent.exe`.
- [x] Keep agent alive when the Tauri settings window closes.
- [x] Launch settings from the native tray or direct app launch.

## 6. Validation And Documentation

- [x] Run Rust formatting.
- [x] Attempt full Rust test/clippy validation.
- [x] Document MSVC Build Tools and Windows SDK as required for full Rust validation.
- [x] Update validation script to diagnose missing Windows native build tools.
- [x] Run frontend production build.
- [x] Update README and development notes.
- [x] Sync OpenSpec main spec and archive this change.

## Residual Follow-Up

These are tracked as follow-up work outside this archived change:

- Install MSVC Build Tools and Windows SDK on the machine used for final verification.
- Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` from Developer PowerShell.
- Complete and manually verify the real Win32 scrolling overlay renderer.

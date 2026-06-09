# Reminder V1

Rust Windows reminder agent prototype for low-resource scheduled screen-overlay reminders, with a hybrid V2 settings shell.

## Current scope

- Cross-platform testable scheduling core.
- Local SQLite storage with reminder history.
- CLI for adding/listing/enabling/disabling/deleting reminders.
- Agent loop that loads reminders, waits for the next due item, and dispatches display events.
- Shared local IPC protocol used by the agent, CLI, and settings UI.
- Native Windows tray entry points for opening settings, testing overlay, pausing, resuming, showing next reminder, and exiting.
- Tauri v2 settings app that opens on demand and talks to the native agent.
- Overlay abstraction with a no-op implementation for non-Windows builds and a Windows implementation slot for the Win32 layered-window renderer.

## Design constraints

- The always-running process is `reminder-agent`, not a traditional Windows Service.
- Tauri is used for settings only; the scheduler and overlay remain native Rust.
- Scheduling uses a min-heap model and a single wait point for the nearest due reminder.
- On Windows, the agent timer backend uses a WaitableTimer instead of a polling sleep loop.
- Rules are stored as structured JSON to allow V2-compatible expansion.
- The rule engine is deterministic and unit-testable without Windows APIs.

## Run locally

Windows Rust builds require both Rust/Cargo and the MSVC native build toolchain:

- Visual Studio Build Tools with "Desktop development with C++".
- A Windows SDK that provides import libraries such as `kernel32.lib`.
- A shell with `link.exe` on `PATH` and the Windows SDK libraries on `LIB` (Developer PowerShell handles this automatically).

```bash
cargo test --workspace
cargo run -p reminder-agent
cargo run -p reminder-cli -- status
cargo run -p reminder-cli -- add-interval --title "Drink water" --message "Drink water" --window 09:00-18:00 --interval-minutes 30
cargo run -p reminder-cli -- next
cargo run -p reminder-cli -- open-settings
```

Run the settings UI from `apps/reminder-ui`:

```bash
npm install
npm run tauri dev
```

The settings UI expects `reminder-agent` to be running. If it is not running, the UI shows an agent unavailable state.

## Hybrid architecture

```text
reminder-agent.exe
  native Rust, always running
  scheduler + SQLite + overlay + tray + local IPC

reminder-ui.exe
  Tauri v2, launched on demand
  settings UI + rule editor + display settings + schedule preview + history

reminder.exe
  CLI, prefers agent IPC and falls back to direct SQLite when needed
```

IPC uses newline-delimited JSON request/response messages. Windows builds use a per-user named pipe name derived from `reminder-agent-v1`; non-Windows builds retain a localhost TCP fallback at `127.0.0.1:38741` for development.

## Windows scheduled startup

After building `reminder-agent.exe`, register user-logon startup:

```powershell
./scripts/install_task.ps1 -AgentPath "C:\path\to\reminder-agent.exe"
```

Remove it:

```powershell
./scripts/uninstall_task.ps1
```

## Validation

```powershell
./scripts/validate.ps1
```

If Cargo is installed outside `PATH`, pass it explicitly:

```powershell
./scripts/validate.ps1 -CargoPath "D:\develop\rust\cargo\bin\cargo.exe"
```

This runs Rust checks and, when Node dependencies exist, the Tauri frontend build. On Windows, Rust validation is skipped with a clear warning if MSVC Build Tools or the Windows SDK import libraries are not available.

In this workspace, `npm run build` for `apps/reminder-ui` passes. Full Rust validation requires Rust/Cargo plus MSVC Build Tools and a Windows SDK.

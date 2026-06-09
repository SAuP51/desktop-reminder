# Design: Hybrid Tauri Settings Architecture

## Architecture

```text
reminder-agent.exe
  native Rust, always running
  scheduler + SQLite + Win32 overlay abstraction + tray + Named Pipe IPC

reminder-ui.exe
  Tauri v2, launched on demand
  settings UI + rule editor + preview + history

reminder.exe
  CLI, prefers agent IPC and falls back to SQLite only for offline/diagnostic paths
```

## Agent Responsibilities

The agent is the only always-running process. It:

- Loads enabled reminders from SQLite.
- Builds and maintains the scheduler queue.
- Waits for the nearest due reminder through the timer abstraction.
- Dispatches reminder display requests to the native overlay backend.
- Records basic reminder history.
- Serves local IPC commands.
- Owns reloads after reminder mutations.
- Hosts the native tray entry points.

On Windows, the timer backend uses WaitableTimer so idle CPU can stay near zero. Non-Windows builds use a simple timer thread for development.

## IPC

IPC uses newline-delimited JSON request/response messages.

Windows transport:

- Per-user named pipe derived from `reminder-agent-v1`.
- The user suffix is sanitized from the current user name.

Non-Windows development transport:

- TCP fallback at `127.0.0.1:38741`.

Supported commands:

- `GetStatus`
- `ListReminders`
- `GetReminder`
- `CreateReminder`
- `UpdateReminder`
- `DeleteReminder`
- `SetReminderEnabled`
- `PreviewSchedule`
- `ShowTestReminder`
- `OpenSettings`
- `PauseForDuration`
- `Resume`
- `GetHistory`
- `ReloadRules`
- `ShutdownAgent`

## Settings UI

The settings app is Tauri v2 with React and TypeScript. It opens only from direct launch, the tray, or explicit CLI/IPC request. Closing the settings window exits the Tauri process and leaves the agent running.

The UI talks to the Tauri backend, and the Tauri backend talks to the agent through shared IPC. The UI does not write SQLite directly.

Current workflows:

- Agent status.
- Reminder list.
- Create/edit/delete.
- Enable/disable.
- Once, daily, and interval rule editing.
- Next-fire preview.
- Display policy editing.
- Test overlay request.
- Pause/resume.
- History.
- Start-agent affordance when unavailable.

## Tray

The tray belongs to the native agent. It exposes:

- Open Settings.
- Test Reminder.
- Pause 30m.
- Resume.
- Next Reminder.
- Exit.

This keeps the webview out of the idle path.

## Storage

SQLite remains the local source of truth. Existing reminder schema remains compatible with structured JSON schedule/display fields. History is stored in `reminder_history`.

## Autostart

Autostart remains Windows Task Scheduler and points only to `reminder-agent.exe`. The Tauri autostart plugin is not used because the settings app should not start at login.

## Deferred Work

- Complete Win32 scrolling layered-window renderer.
- Full Windows manual checks after MSVC Build Tools and Windows SDK are installed.
- Packaging/bundling verification for final desktop distribution.

# Hybrid Desktop Reminder Specification

## Purpose

Define the hybrid desktop architecture for a low-resource Windows reminder application: a native Rust agent remains always running for scheduling, storage, tray, IPC, and overlay dispatch, while a Tauri v2 settings UI opens only when the user needs graphical configuration.

## Requirements

### Requirement: Native agent remains the always-running process

The desktop reminder system SHALL keep the always-running scheduler, storage owner, overlay dispatcher, native tray, and local IPC server in `reminder-agent`.

#### Scenario: Agent starts without the settings UI

- **GIVEN** the user logs into Windows
- **WHEN** Task Scheduler starts the reminder application
- **THEN** only `reminder-agent.exe` is started automatically
- **AND** the Tauri settings UI is not started until requested

#### Scenario: Agent avoids polling while idle

- **GIVEN** reminders have been loaded into the scheduler queue
- **WHEN** the agent is waiting for the next reminder
- **THEN** the Windows backend uses a WaitableTimer for the nearest wake-up
- **AND** the loop does not rely on a fixed `thread::sleep` polling interval

#### Scenario: Agent avoids duplicate instances

- **GIVEN** an agent process is already serving local IPC
- **WHEN** another agent process starts without an explicit multi-instance override
- **THEN** the second process exits after detecting the existing agent

### Requirement: Agent owns reminder mutations and reloads

The system SHALL route normal reminder mutations through the running agent so the scheduler queue, SQLite state, and overlay dispatch remain consistent.

#### Scenario: UI creates or edits a reminder

- **GIVEN** the Tauri settings UI is open
- **WHEN** the user creates, edits, deletes, enables, or disables a reminder
- **THEN** the UI sends an IPC request to the agent
- **AND** the agent validates the schedule, writes SQLite, and reloads the scheduler queue

#### Scenario: CLI uses IPC first

- **GIVEN** the CLI is invoked without an explicit database override
- **WHEN** the agent is running
- **THEN** the CLI sends the command through IPC
- **AND** direct SQLite writes are reserved for offline or diagnostic fallback paths

### Requirement: Local IPC supports UI and CLI control

The system SHALL expose a local JSON request/response IPC protocol shared by the agent, CLI, and Tauri backend.

#### Scenario: Windows uses a user-scoped named pipe

- **GIVEN** the application is built on Windows
- **WHEN** a client connects to the agent IPC endpoint
- **THEN** the transport uses a per-user named pipe derived from `reminder-agent-v1`
- **AND** messages are newline-delimited JSON request/response envelopes

#### Scenario: Development fallback works outside Windows

- **GIVEN** the application is built on a non-Windows development target
- **WHEN** a client connects to the agent IPC endpoint
- **THEN** the transport uses `127.0.0.1:38741`

#### Scenario: Supported commands cover settings workflows

- **GIVEN** the settings UI or CLI needs to control the agent
- **WHEN** it sends IPC requests
- **THEN** the protocol supports status, list/get/create/update/delete reminders, enable/disable, schedule preview, test overlay, pause/resume, history, reload, open settings, and shutdown

### Requirement: Tauri settings UI opens on demand

The system SHALL provide a Tauri v2 settings application for graphical configuration while keeping it out of the always-running path.

#### Scenario: Tray opens settings

- **GIVEN** the native agent tray icon is available
- **WHEN** the user selects Open Settings
- **THEN** the agent launches `reminder-ui.exe`
- **AND** closing the settings window exits the Tauri process without stopping the agent

#### Scenario: UI exposes core configuration workflows

- **GIVEN** the settings UI can reach the agent
- **WHEN** the user manages reminders
- **THEN** the UI supports list, create, edit, delete, enable, disable, next-fire preview, display settings, test overlay, pause/resume, and basic history

#### Scenario: UI handles unavailable agent state

- **GIVEN** the settings UI starts while the agent is unavailable
- **WHEN** the UI attempts to load status or reminders
- **THEN** it shows a clear unavailable state
- **AND** offers a start-agent action

### Requirement: Native overlay remains outside Tauri

The system SHALL keep transparent topmost reminder display behind the native overlay abstraction rather than implementing the scrolling overlay in the Tauri webview.

#### Scenario: Agent dispatches overlay requests

- **GIVEN** a reminder is due
- **WHEN** the agent dispatches it
- **THEN** it sends a `DisplayRequest` to the platform overlay backend
- **AND** records reminder history after display dispatch

### Requirement: Validation reflects platform prerequisites

The project SHALL make local validation behavior explicit for Rust, Tauri frontend, and Windows native build prerequisites.

#### Scenario: Windows build tools are missing

- **GIVEN** Rust/Cargo are installed
- **AND** MSVC Build Tools or Windows SDK import libraries are missing
- **WHEN** the validation script runs
- **THEN** it reports the missing native build prerequisites clearly
- **AND** continues frontend validation when possible

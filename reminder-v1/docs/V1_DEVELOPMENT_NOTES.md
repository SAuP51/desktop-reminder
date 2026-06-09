# V1 Development Notes

## Implemented in this iteration

1. Workspace structure.
2. Stable data model for reminders, schedule rules, display policy, exclusions, and missed policy.
3. Deterministic scheduling engine:
   - once
   - daily fixed times
   - selected weekdays
   - date range
   - excluded dates
   - time window + interval
   - cross-midnight interval windows
4. Min-heap scheduler queue.
5. SQLite storage layer with migrations and JSON rule storage.
6. CLI for add/list/next/enable/disable/delete.
7. Agent loop that dispatches due reminders through an overlay abstraction.
8. Overlay abstraction and Windows Win32 skeleton.
9. Shared IPC protocol for agent/CLI/settings integration:
   - Windows uses a per-user Named Pipe transport.
   - Non-Windows builds keep a localhost TCP fallback for development.
10. Low-idle agent controls:
   - Windows WaitableTimer backend for scheduler wakeups.
   - Queue reload signaling after mutations.
   - Pause/resume rebuilds the queue from the current time.
11. Native Windows tray module with Open Settings, Test Reminder, Pause 30m, Resume, Next Reminder, and Exit.
12. Tauri settings app with reminder list, create/edit/delete, enable/disable, once/daily/interval rules, schedule preview, display settings, test overlay, history, and agent-start affordance.

## Intentionally deferred

- Real Win32 scrolling layered-window renderer.
- Full Windows compile/manual verification in this environment, because MSVC Build Tools and Windows SDK import libraries are not installed.

These are isolated behind modules/traits so they can be implemented without changing the scheduling and storage core.

## Validation

Rust/Cargo are available in this environment, but full Rust validation currently stops before crate code because the MSVC linker environment is missing `link.exe` and Windows SDK import libraries such as `kernel32.lib`. Unit tests were written in:

- `crates/reminder-core/src/scheduler.rs`
- `crates/reminder-storage/src/lib.rs`

Run this on a Rust-enabled environment:

```powershell
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Frontend validation passed with:

```bash
cd apps/reminder-ui
npm run build
```

## Design notes

The scheduler works in local naive time for deterministic rule calculation. The model includes `utc_offset_seconds` on reminders and `next_fire_after_utc` for runtime conversion. V2 should replace this with a named time-zone model if cross-time-zone/DST correctness becomes a requirement.

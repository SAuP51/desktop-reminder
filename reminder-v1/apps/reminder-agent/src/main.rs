#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration as ChronoDuration, Local, Utc};
use clap::Parser;
use directories_next::ProjectDirs;
use reminder_core::{
    DisplayPolicy, Priority, Reminder, ScheduleEngine, ScheduledReminder, SchedulerQueue,
};
use reminder_ipc::{AgentStatus, IpcRequest, IpcResponse, ReminderHistoryEntry};
use reminder_overlay::{DisplayRequest, OverlayBackend, PlatformOverlay};
use reminder_storage::ReminderStore;
use uuid::Uuid;

mod timer;
mod tray;

#[derive(Debug, Parser)]
#[command(name = "reminder-agent")]
#[command(about = "Low-resource reminder background agent", version)]
struct Args {
    #[arg(long)]
    db: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    once: bool,

    #[arg(long, default_value_t = false)]
    no_ipc: bool,

    #[arg(long, default_value_t = false)]
    allow_multiple: bool,
}

#[derive(Debug)]
enum AgentCommand {
    Reload,
    ShowTest(DisplayRequest),
    ShowNextReminder,
    OpenSettings,
    PauseForDuration(u32),
    Resume,
    Shutdown,
}

#[derive(Debug)]
enum AgentEvent {
    Command(AgentCommand),
    TimerElapsed(u64),
}

#[derive(Debug, Default)]
struct AgentControl {
    paused_until_utc: Mutex<Option<DateTime<Utc>>>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let args = Args::parse();

    if !args.allow_multiple
        && !args.no_ipc
        && reminder_ipc::send_request(&IpcRequest::GetStatus).is_ok()
    {
        tracing::info!("reminder-agent is already running");
        return Ok(());
    }

    let db_path = args.db.unwrap_or_else(default_db_path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create data directory {}", parent.display()))?;
    }

    let store = ReminderStore::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    let mut overlay = PlatformOverlay::default();
    let control = Arc::new(AgentControl::default());
    let (event_tx, event_rx) = mpsc::channel();

    if !args.no_ipc {
        start_ipc_server(db_path.clone(), control.clone(), event_tx.clone());
    }

    if !args.once {
        tray::start(event_tx.clone())?;
    }

    run_agent_loop(&store, &mut overlay, event_rx, event_tx, control, args.once)
}

fn start_ipc_server(db_path: PathBuf, control: Arc<AgentControl>, event_tx: Sender<AgentEvent>) {
    thread::spawn(move || {
        let result = reminder_ipc::serve(move |request| {
            handle_ipc_request(&db_path, control.clone(), event_tx.clone(), request)
        });

        if let Err(error) = result {
            tracing::error!(%error, "IPC server stopped");
        }
    });
}

fn handle_ipc_request(
    db_path: &Path,
    control: Arc<AgentControl>,
    event_tx: Sender<AgentEvent>,
    request: IpcRequest,
) -> Result<IpcResponse, String> {
    match request {
        IpcRequest::GetStatus => {
            let store = open_store(db_path)?;
            let reminders = load_enabled_reminders(&store).map_err(|error| error.to_string())?;
            let queue = SchedulerQueue::rebuild(&reminders, Local::now().naive_local())
                .map_err(|error| error.to_string())?;

            let paused_until_utc = *control
                .paused_until_utc
                .lock()
                .map_err(|_| "pause state lock poisoned".to_owned())?;

            Ok(IpcResponse::Status(AgentStatus {
                running: true,
                enabled_reminders: reminders.len(),
                next_fire_at: queue.peek_next().map(|item| item.next_fire_at),
                paused_until_utc,
            }))
        }
        IpcRequest::ListReminders => {
            let store = open_store(db_path)?;
            Ok(IpcResponse::Reminders(
                store.list_reminders().map_err(|error| error.to_string())?,
            ))
        }
        IpcRequest::GetReminder { id } => {
            let store = open_store(db_path)?;
            Ok(IpcResponse::Reminder(
                store.get_reminder(id).map_err(|error| error.to_string())?,
            ))
        }
        IpcRequest::CreateReminder { reminder } => {
            ScheduleEngine::validate(&reminder.schedule).map_err(|error| error.to_string())?;
            let store = open_store(db_path)?;
            store
                .upsert_reminder(&reminder)
                .map_err(|error| error.to_string())?;
            send_agent_command(&event_tx, AgentCommand::Reload)?;
            Ok(IpcResponse::ReminderId(reminder.id))
        }
        IpcRequest::UpdateReminder { reminder } => {
            ScheduleEngine::validate(&reminder.schedule).map_err(|error| error.to_string())?;
            let store = open_store(db_path)?;
            store
                .upsert_reminder(&reminder)
                .map_err(|error| error.to_string())?;
            send_agent_command(&event_tx, AgentCommand::Reload)?;
            Ok(IpcResponse::ReminderId(reminder.id))
        }
        IpcRequest::DeleteReminder { id } => {
            let store = open_store(db_path)?;
            let changed = store
                .delete_reminder(id)
                .map_err(|error| error.to_string())?;
            send_agent_command(&event_tx, AgentCommand::Reload)?;
            Ok(IpcResponse::Changed { changed })
        }
        IpcRequest::SetReminderEnabled { id, enabled } => {
            let store = open_store(db_path)?;
            let changed = store
                .set_enabled(id, enabled)
                .map_err(|error| error.to_string())?;
            send_agent_command(&event_tx, AgentCommand::Reload)?;
            Ok(IpcResponse::Changed { changed })
        }
        IpcRequest::PreviewSchedule { rule, after, limit } => {
            let after = after.unwrap_or_else(|| Local::now().naive_local());
            let preview = ScheduleEngine::preview_after(&rule, after, limit.clamp(1, 50))
                .map_err(|error| error.to_string())?;
            Ok(IpcResponse::Preview(preview))
        }
        IpcRequest::ShowTestReminder {
            title,
            message,
            priority,
            policy,
        } => {
            send_agent_command(
                &event_tx,
                AgentCommand::ShowTest(DisplayRequest {
                    reminder_id: Uuid::new_v4(),
                    title,
                    message,
                    priority,
                    policy,
                }),
            )?;
            Ok(IpcResponse::Ack)
        }
        IpcRequest::OpenSettings => {
            send_agent_command(&event_tx, AgentCommand::OpenSettings)?;
            Ok(IpcResponse::Ack)
        }
        IpcRequest::PauseForDuration { minutes } => {
            send_agent_command(&event_tx, AgentCommand::PauseForDuration(minutes))?;
            Ok(IpcResponse::Ack)
        }
        IpcRequest::Resume => {
            send_agent_command(&event_tx, AgentCommand::Resume)?;
            Ok(IpcResponse::Ack)
        }
        IpcRequest::GetHistory { limit } => {
            let store = open_store(db_path)?;
            let rows = store
                .list_history(limit.clamp(1, 200))
                .map_err(|error| error.to_string())?;
            Ok(IpcResponse::History(
                rows.into_iter()
                    .map(|row| ReminderHistoryEntry {
                        id: row.id,
                        reminder_id: row.reminder_id,
                        fired_at_utc: row.fired_at_utc,
                        displayed_at_utc: row.displayed_at_utc,
                        result: row.result,
                    })
                    .collect(),
            ))
        }
        IpcRequest::ReloadRules => {
            send_agent_command(&event_tx, AgentCommand::Reload)?;
            Ok(IpcResponse::Ack)
        }
        IpcRequest::ShutdownAgent => {
            send_agent_command(&event_tx, AgentCommand::Shutdown)?;
            Ok(IpcResponse::Ack)
        }
    }
}

fn run_agent_loop(
    store: &ReminderStore,
    overlay: &mut dyn OverlayBackend,
    event_rx: Receiver<AgentEvent>,
    event_tx: Sender<AgentEvent>,
    control: Arc<AgentControl>,
    once: bool,
) -> Result<()> {
    let mut reminders = load_enabled_reminders(store)?;
    let mut queue = SchedulerQueue::rebuild(&reminders, Local::now().naive_local())?;
    let mut shutdown = false;
    let timer = timer::TimerService::start(event_tx)?;
    let mut timer_generation = 0_u64;

    loop {
        drain_agent_events(
            &event_rx,
            timer_generation,
            store,
            overlay,
            &mut reminders,
            &mut queue,
            control.as_ref(),
            &mut shutdown,
        )?;
        if shutdown {
            break;
        }

        if pause_has_elapsed(&control)? {
            reminders = load_enabled_reminders(store)?;
            queue = SchedulerQueue::rebuild(reminders.as_slice(), Local::now().naive_local())?;
            tracing::info!("pause window elapsed; reminders resumed");
        }

        if !is_paused(&control)? {
            dispatch_due_reminders(store, overlay, &reminders, &mut queue)?;
        }

        if once {
            break;
        }

        timer_generation = timer_generation.wrapping_add(1);
        timer.arm(next_wait_timeout(&queue, &control)?, timer_generation)?;

        match event_rx.recv() {
            Ok(event) => handle_agent_event(
                event,
                timer_generation,
                store,
                overlay,
                &mut reminders,
                &mut queue,
                control.as_ref(),
                &mut shutdown,
            )?,
            Err(_) => break,
        }
    }

    Ok(())
}

fn dispatch_due_reminders(
    store: &ReminderStore,
    overlay: &mut dyn OverlayBackend,
    reminders: &[Reminder],
    queue: &mut SchedulerQueue,
) -> Result<()> {
    let now = Local::now().naive_local();
    let due = queue.pop_due(now);
    if due.is_empty() {
        return Ok(());
    }

    let by_id = reminders
        .iter()
        .map(|reminder| (reminder.id, reminder.clone()))
        .collect::<HashMap<_, _>>();

    for item in due {
        let Some(reminder) = by_id.get(&item.reminder_id) else {
            tracing::warn!(reminder_id = %item.reminder_id, "due reminder no longer exists");
            continue;
        };

        overlay.show(DisplayRequest {
            reminder_id: reminder.id,
            title: reminder.title.clone(),
            message: reminder.message.clone(),
            priority: reminder.priority,
            policy: reminder.display.clone(),
        })?;

        store.record_history(reminder.id, Utc::now(), Some(Utc::now()), "displayed")?;

        if let Some(next_fire_at) =
            ScheduleEngine::next_fire_after(&reminder.schedule, item.due_at)?
        {
            queue.push(ScheduledReminder {
                reminder_id: reminder.id,
                next_fire_at,
            });
        }
    }

    Ok(())
}

fn drain_agent_events(
    event_rx: &Receiver<AgentEvent>,
    current_generation: u64,
    store: &ReminderStore,
    overlay: &mut dyn OverlayBackend,
    reminders: &mut Vec<Reminder>,
    queue: &mut SchedulerQueue,
    control: &AgentControl,
    shutdown: &mut bool,
) -> Result<()> {
    loop {
        match event_rx.try_recv() {
            Ok(event) => handle_agent_event(
                event,
                current_generation,
                store,
                overlay,
                reminders,
                queue,
                control,
                shutdown,
            )?,
            Err(TryRecvError::Empty) => return Ok(()),
            Err(TryRecvError::Disconnected) => {
                *shutdown = true;
                return Ok(());
            }
        }
    }
}

fn handle_agent_event(
    event: AgentEvent,
    current_generation: u64,
    store: &ReminderStore,
    overlay: &mut dyn OverlayBackend,
    reminders: &mut Vec<Reminder>,
    queue: &mut SchedulerQueue,
    control: &AgentControl,
    shutdown: &mut bool,
) -> Result<()> {
    match event {
        AgentEvent::Command(command) => {
            handle_agent_command(command, store, overlay, reminders, queue, control, shutdown)
        }
        AgentEvent::TimerElapsed(generation) => {
            if generation != current_generation {
                tracing::debug!(generation, current_generation, "ignoring stale timer event");
            }
            Ok(())
        }
    }
}

fn handle_agent_command(
    command: AgentCommand,
    store: &ReminderStore,
    overlay: &mut dyn OverlayBackend,
    reminders: &mut Vec<Reminder>,
    queue: &mut SchedulerQueue,
    control: &AgentControl,
    shutdown: &mut bool,
) -> Result<()> {
    match command {
        AgentCommand::Reload => {
            *reminders = load_enabled_reminders(store)?;
            *queue = SchedulerQueue::rebuild(reminders.as_slice(), Local::now().naive_local())?;
            tracing::info!("scheduler queue reloaded");
        }
        AgentCommand::ShowTest(request) => {
            overlay.show(request)?;
        }
        AgentCommand::ShowNextReminder => {
            show_next_reminder(overlay, reminders, queue)?;
        }
        AgentCommand::OpenSettings => {
            launch_settings()?;
        }
        AgentCommand::PauseForDuration(minutes) => {
            let paused_until = Utc::now() + ChronoDuration::minutes(i64::from(minutes.max(1)));
            *control
                .paused_until_utc
                .lock()
                .map_err(|_| anyhow::anyhow!("pause state lock poisoned"))? = Some(paused_until);
            tracing::info!(%paused_until, "reminders paused");
        }
        AgentCommand::Resume => {
            *control
                .paused_until_utc
                .lock()
                .map_err(|_| anyhow::anyhow!("pause state lock poisoned"))? = None;
            rebuild_queue_from_now(store, reminders, queue)?;
            tracing::info!("reminders resumed");
        }
        AgentCommand::Shutdown => {
            *shutdown = true;
        }
    }

    Ok(())
}

fn rebuild_queue_from_now(
    store: &ReminderStore,
    reminders: &mut Vec<Reminder>,
    queue: &mut SchedulerQueue,
) -> Result<()> {
    *reminders = load_enabled_reminders(store)?;
    *queue = SchedulerQueue::rebuild(reminders.as_slice(), Local::now().naive_local())?;
    Ok(())
}

fn show_next_reminder(
    overlay: &mut dyn OverlayBackend,
    reminders: &[Reminder],
    queue: &SchedulerQueue,
) -> Result<()> {
    let Some(next) = queue.peek_next() else {
        overlay.show(DisplayRequest {
            reminder_id: Uuid::new_v4(),
            title: "No upcoming reminder".to_owned(),
            message: "There are no enabled reminders waiting in the queue.".to_owned(),
            priority: Priority::Normal,
            policy: DisplayPolicy::default(),
        })?;
        return Ok(());
    };

    let title = reminders
        .iter()
        .find(|reminder| reminder.id == next.reminder_id)
        .map(|reminder| reminder.title.clone())
        .unwrap_or_else(|| "Next Reminder".to_owned());

    overlay.show(DisplayRequest {
        reminder_id: next.reminder_id,
        title,
        message: format!(
            "Next fire: {}",
            next.next_fire_at.format("%Y-%m-%d %H:%M:%S")
        ),
        priority: Priority::Normal,
        policy: DisplayPolicy::default(),
    })?;
    Ok(())
}

fn next_wait_timeout(queue: &SchedulerQueue, control: &AgentControl) -> Result<StdDuration> {
    let paused_until = *control
        .paused_until_utc
        .lock()
        .map_err(|_| anyhow::anyhow!("pause state lock poisoned"))?;

    if let Some(paused_until) = paused_until {
        let wait = paused_until.signed_duration_since(Utc::now());
        return Ok(duration_or_minimum(wait.num_milliseconds()));
    }

    let Some(next_time) = queue.peek_next().map(|item| item.next_fire_at) else {
        return Ok(StdDuration::from_secs(24 * 60 * 60));
    };

    let wait = next_time.signed_duration_since(Local::now().naive_local());
    Ok(duration_or_minimum(wait.num_milliseconds()))
}

fn duration_or_minimum(milliseconds: i64) -> StdDuration {
    if milliseconds <= 0 {
        StdDuration::from_millis(1)
    } else {
        StdDuration::from_millis(milliseconds as u64)
    }
}

fn pause_has_elapsed(control: &AgentControl) -> Result<bool> {
    let mut paused_until = control
        .paused_until_utc
        .lock()
        .map_err(|_| anyhow::anyhow!("pause state lock poisoned"))?;

    if paused_until
        .as_ref()
        .is_some_and(|value| *value <= Utc::now())
    {
        *paused_until = None;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn is_paused(control: &AgentControl) -> Result<bool> {
    let paused_until = control
        .paused_until_utc
        .lock()
        .map_err(|_| anyhow::anyhow!("pause state lock poisoned"))?;
    Ok(paused_until
        .as_ref()
        .is_some_and(|value| *value > Utc::now()))
}

fn open_store(path: &Path) -> Result<ReminderStore, String> {
    ReminderStore::open(path).map_err(|error| error.to_string())
}

fn send_agent_command(event_tx: &Sender<AgentEvent>, command: AgentCommand) -> Result<(), String> {
    event_tx
        .send(AgentEvent::Command(command))
        .map_err(|error| error.to_string())
}

fn launch_settings() -> Result<()> {
    let executable_name = if cfg!(windows) {
        "reminder-ui.exe"
    } else {
        "reminder-ui"
    };

    let current_exe = std::env::current_exe().context("failed to resolve agent executable path")?;
    let sibling = current_exe
        .parent()
        .map(|parent| parent.join(executable_name))
        .unwrap_or_else(|| PathBuf::from(executable_name));

    let mut command = if sibling.exists() {
        Command::new(sibling)
    } else {
        Command::new(executable_name)
    };

    command
        .spawn()
        .context("failed to launch reminder settings")?;
    Ok(())
}

fn load_enabled_reminders(store: &ReminderStore) -> Result<Vec<Reminder>> {
    let reminders = store
        .list_reminders()?
        .into_iter()
        .filter(|reminder| reminder.enabled)
        .collect::<Vec<_>>();
    tracing::info!(count = reminders.len(), "loaded enabled reminders");
    Ok(reminders)
}

fn default_db_path() -> PathBuf {
    ProjectDirs::from("com", "soma-team", "Reminder")
        .map(|dirs| dirs.data_dir().join("reminder.sqlite3"))
        .unwrap_or_else(|| PathBuf::from("reminder.sqlite3"))
}

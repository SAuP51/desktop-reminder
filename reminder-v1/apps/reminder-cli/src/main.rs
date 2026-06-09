use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime};
use clap::{Parser, Subcommand};
use directories_next::ProjectDirs;
use reminder_core::{DisplayPolicy, Priority, Reminder, ScheduleEngine, ScheduleRule};
use reminder_ipc::{IpcRequest, IpcResponse};
use reminder_storage::ReminderStore;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "reminder")]
#[command(about = "Manage local reminder rules", version)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Use a specific SQLite DB and skip agent IPC"
    )]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    AddOnce {
        #[arg(long)]
        title: String,
        #[arg(long)]
        message: Option<String>,
        #[arg(long, help = "Local datetime in YYYY-MM-DD HH:MM format")]
        at: String,
    },
    AddDaily {
        #[arg(long)]
        title: String,
        #[arg(long)]
        message: Option<String>,
        #[arg(long, help = "Local time in HH:MM format")]
        time: String,
    },
    AddInterval {
        #[arg(long)]
        title: String,
        #[arg(long)]
        message: Option<String>,
        #[arg(long, help = "Time window in HH:MM-HH:MM format")]
        window: String,
        #[arg(long)]
        interval_minutes: u32,
    },
    List,
    Next,
    Status,
    History {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Enable {
        id: String,
    },
    Disable {
        id: String,
    },
    Delete {
        id: String,
    },
    TestOverlay {
        message: String,
        #[arg(long, default_value = "Reminder")]
        title: String,
    },
    OpenSettings,
    Pause {
        #[arg(long, default_value_t = 30)]
        minutes: u32,
    },
    Resume,
    Reload,
    ShutdownAgent,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let cli = Cli::parse();

    match cli.command {
        Command::AddOnce { title, message, at } => {
            let dt = parse_datetime(&at)?;
            let reminder = Reminder::new(
                title,
                message.unwrap_or_default(),
                ScheduleRule::once(dt.date(), dt.time()),
            );
            create_reminder(cli.db, reminder)?;
        }
        Command::AddDaily {
            title,
            message,
            time,
        } => {
            let reminder = Reminder::new(
                title,
                message.unwrap_or_default(),
                ScheduleRule::daily_at(vec![parse_time(&time)?]),
            );
            create_reminder(cli.db, reminder)?;
        }
        Command::AddInterval {
            title,
            message,
            window,
            interval_minutes,
        } => {
            let (start, end) = parse_window(&window)?;
            let reminder = Reminder::new(
                title,
                message.unwrap_or_default(),
                ScheduleRule::daily_interval(start, end, interval_minutes),
            );
            ScheduleEngine::validate(&reminder.schedule)?;
            create_reminder(cli.db, reminder)?;
        }
        Command::List => {
            let reminders = list_reminders(cli.db)?;
            print_reminders(&reminders);
        }
        Command::Next => {
            let reminders = list_reminders(cli.db)?;
            print_next(reminders);
        }
        Command::Status => match reminder_ipc::send_request(&IpcRequest::GetStatus) {
            Ok(IpcResponse::Status(status)) => {
                println!("running={}", status.running);
                println!("enabled_reminders={}", status.enabled_reminders);
                println!(
                    "next_fire_at={}",
                    status
                        .next_fire_at
                        .map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "none".to_owned())
                );
                println!(
                    "paused_until_utc={}",
                    status
                        .paused_until_utc
                        .map(|value| value.to_rfc3339())
                        .unwrap_or_else(|| "none".to_owned())
                );
            }
            Ok(other) => return Err(anyhow!("unexpected agent response: {other:?}")),
            Err(error) => return Err(anyhow!("agent unavailable: {error}")),
        },
        Command::History { limit } => {
            match reminder_ipc::send_request(&IpcRequest::GetHistory { limit }) {
                Ok(IpcResponse::History(rows)) => {
                    if rows.is_empty() {
                        println!("no history found");
                    }
                    for row in rows {
                        println!(
                            "{} | {} | fired={} | displayed={} | {}",
                            row.id,
                            row.reminder_id,
                            row.fired_at_utc.to_rfc3339(),
                            row.displayed_at_utc
                                .map(|value| value.to_rfc3339())
                                .unwrap_or_else(|| "none".to_owned()),
                            row.result
                        );
                    }
                }
                Ok(other) => return Err(anyhow!("unexpected agent response: {other:?}")),
                Err(error) => return Err(anyhow!("agent unavailable: {error}")),
            }
        }
        Command::Enable { id } => set_enabled(cli.db, parse_id(&id)?, true)?,
        Command::Disable { id } => set_enabled(cli.db, parse_id(&id)?, false)?,
        Command::Delete { id } => delete_reminder(cli.db, parse_id(&id)?)?,
        Command::TestOverlay { message, title } => {
            send_ack(IpcRequest::ShowTestReminder {
                title,
                message,
                priority: Priority::Normal,
                policy: DisplayPolicy::default(),
            })?;
            println!("test overlay requested");
        }
        Command::OpenSettings => {
            send_ack(IpcRequest::OpenSettings)?;
            println!("settings requested");
        }
        Command::Pause { minutes } => {
            send_ack(IpcRequest::PauseForDuration { minutes })?;
            println!("paused for {minutes} minute(s)");
        }
        Command::Resume => {
            send_ack(IpcRequest::Resume)?;
            println!("resumed");
        }
        Command::Reload => {
            send_ack(IpcRequest::ReloadRules)?;
            println!("reload requested");
        }
        Command::ShutdownAgent => {
            send_ack(IpcRequest::ShutdownAgent)?;
            println!("shutdown requested");
        }
    }

    Ok(())
}

fn create_reminder(db: Option<PathBuf>, reminder: Reminder) -> Result<()> {
    if db.is_none() {
        match reminder_ipc::send_request(&IpcRequest::CreateReminder {
            reminder: reminder.clone(),
        }) {
            Ok(IpcResponse::ReminderId(id)) => {
                println!("created reminder {id}");
                return Ok(());
            }
            Ok(other) => return Err(anyhow!("unexpected agent response: {other:?}")),
            Err(error) => {
                tracing::warn!(%error, "agent unavailable; falling back to direct SQLite write");
            }
        }
    }

    let store = open_store(db)?;
    store.upsert_reminder(&reminder)?;
    let _ = reminder_ipc::send_request(&IpcRequest::ReloadRules);
    println!("created reminder {}", reminder.id);
    Ok(())
}

fn list_reminders(db: Option<PathBuf>) -> Result<Vec<Reminder>> {
    if db.is_none() {
        match reminder_ipc::send_request(&IpcRequest::ListReminders) {
            Ok(IpcResponse::Reminders(reminders)) => return Ok(reminders),
            Ok(other) => return Err(anyhow!("unexpected agent response: {other:?}")),
            Err(error) => {
                tracing::warn!(%error, "agent unavailable; falling back to direct SQLite read")
            }
        }
    }

    Ok(open_store(db)?.list_reminders()?)
}

fn set_enabled(db: Option<PathBuf>, id: Uuid, enabled: bool) -> Result<()> {
    if db.is_none() {
        match reminder_ipc::send_request(&IpcRequest::SetReminderEnabled { id, enabled }) {
            Ok(IpcResponse::Changed { changed }) => {
                println!("changed={} id={}", changed, id);
                return Ok(());
            }
            Ok(other) => return Err(anyhow!("unexpected agent response: {other:?}")),
            Err(error) => {
                tracing::warn!(%error, "agent unavailable; falling back to direct SQLite write")
            }
        }
    }

    let changed = open_store(db)?.set_enabled(id, enabled)?;
    let _ = reminder_ipc::send_request(&IpcRequest::ReloadRules);
    println!("changed={} id={}", changed, id);
    Ok(())
}

fn delete_reminder(db: Option<PathBuf>, id: Uuid) -> Result<()> {
    if db.is_none() {
        match reminder_ipc::send_request(&IpcRequest::DeleteReminder { id }) {
            Ok(IpcResponse::Changed { changed }) => {
                println!("deleted={} id={}", changed, id);
                return Ok(());
            }
            Ok(other) => return Err(anyhow!("unexpected agent response: {other:?}")),
            Err(error) => {
                tracing::warn!(%error, "agent unavailable; falling back to direct SQLite write")
            }
        }
    }

    let changed = open_store(db)?.delete_reminder(id)?;
    let _ = reminder_ipc::send_request(&IpcRequest::ReloadRules);
    println!("deleted={} id={}", changed, id);
    Ok(())
}

fn send_ack(request: IpcRequest) -> Result<()> {
    match reminder_ipc::send_request(&request) {
        Ok(IpcResponse::Ack) => Ok(()),
        Ok(other) => Err(anyhow!("unexpected agent response: {other:?}")),
        Err(error) => Err(anyhow!("agent unavailable: {error}")),
    }
}

fn open_store(db: Option<PathBuf>) -> Result<ReminderStore> {
    let db_path = db.unwrap_or_else(default_db_path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create data directory {}", parent.display()))?;
    }

    ReminderStore::open(&db_path).with_context(|| format!("failed to open {}", db_path.display()))
}

fn print_reminders(reminders: &[Reminder]) {
    if reminders.is_empty() {
        println!("no reminders found");
    }
    for reminder in reminders {
        println!(
            "{} | {} | enabled={} | priority={:?}",
            reminder.id, reminder.title, reminder.enabled, reminder.priority
        );
    }
}

fn print_next(reminders: Vec<Reminder>) {
    let now = Local::now().naive_local();
    let mut rows = reminders
        .into_iter()
        .filter(|reminder| reminder.enabled)
        .filter_map(|reminder| {
            ScheduleEngine::next_fire_after(&reminder.schedule, now)
                .ok()
                .flatten()
                .map(|next| (next, reminder))
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|(next, _)| *next);
    for (next, reminder) in rows.into_iter().take(20) {
        println!(
            "{} | {} | {}",
            next.format("%Y-%m-%d %H:%M:%S"),
            reminder.id,
            reminder.title
        );
    }
}

fn default_db_path() -> PathBuf {
    ProjectDirs::from("com", "soma-team", "Reminder")
        .map(|dirs| dirs.data_dir().join("reminder.sqlite3"))
        .unwrap_or_else(|| PathBuf::from("reminder.sqlite3"))
}

fn parse_datetime(value: &str) -> Result<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M")
        .with_context(|| format!("invalid datetime: {value}"))
}

fn parse_time(value: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M").with_context(|| format!("invalid time: {value}"))
}

fn parse_window(value: &str) -> Result<(NaiveTime, NaiveTime)> {
    let (start, end) = value
        .split_once('-')
        .ok_or_else(|| anyhow!("window must use HH:MM-HH:MM format"))?;
    Ok((parse_time(start)?, parse_time(end)?))
}

fn parse_id(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value).with_context(|| format!("invalid reminder id: {value}"))
}

#[allow(dead_code)]
fn parse_date(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").with_context(|| format!("invalid date: {value}"))
}

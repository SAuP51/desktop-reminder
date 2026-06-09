#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::Command;

use chrono::NaiveDateTime;
use reminder_core::{DisplayPolicy, Priority, Reminder, ReminderId, ScheduleRule};
use reminder_ipc::{AgentStatus, IpcRequest, IpcResponse, ReminderHistoryEntry};
use tauri::Manager;

#[tauri::command]
fn get_status() -> Result<AgentStatus, String> {
    match send(IpcRequest::GetStatus)? {
        IpcResponse::Status(status) => Ok(status),
        other => Err(format!("unexpected agent response: {other:?}")),
    }
}

#[tauri::command]
fn list_reminders() -> Result<Vec<Reminder>, String> {
    match send(IpcRequest::ListReminders)? {
        IpcResponse::Reminders(reminders) => Ok(reminders),
        other => Err(format!("unexpected agent response: {other:?}")),
    }
}

#[tauri::command]
fn create_reminder(reminder: Reminder) -> Result<ReminderId, String> {
    match send(IpcRequest::CreateReminder { reminder })? {
        IpcResponse::ReminderId(id) => Ok(id),
        other => Err(format!("unexpected agent response: {other:?}")),
    }
}

#[tauri::command]
fn update_reminder(reminder: Reminder) -> Result<ReminderId, String> {
    match send(IpcRequest::UpdateReminder { reminder })? {
        IpcResponse::ReminderId(id) => Ok(id),
        other => Err(format!("unexpected agent response: {other:?}")),
    }
}

#[tauri::command]
fn delete_reminder(id: ReminderId) -> Result<bool, String> {
    match send(IpcRequest::DeleteReminder { id })? {
        IpcResponse::Changed { changed } => Ok(changed),
        other => Err(format!("unexpected agent response: {other:?}")),
    }
}

#[tauri::command]
fn set_reminder_enabled(id: ReminderId, enabled: bool) -> Result<bool, String> {
    match send(IpcRequest::SetReminderEnabled { id, enabled })? {
        IpcResponse::Changed { changed } => Ok(changed),
        other => Err(format!("unexpected agent response: {other:?}")),
    }
}

#[tauri::command]
fn preview_schedule(
    rule: ScheduleRule,
    after: Option<NaiveDateTime>,
    limit: usize,
) -> Result<Vec<NaiveDateTime>, String> {
    match send(IpcRequest::PreviewSchedule { rule, after, limit })? {
        IpcResponse::Preview(preview) => Ok(preview),
        other => Err(format!("unexpected agent response: {other:?}")),
    }
}

#[tauri::command]
fn show_test_reminder(title: String, message: String, policy: DisplayPolicy) -> Result<(), String> {
    expect_ack(IpcRequest::ShowTestReminder {
        title,
        message,
        priority: Priority::Normal,
        policy,
    })
}

#[tauri::command]
fn start_agent() -> Result<(), String> {
    let executable_name = if cfg!(windows) {
        "reminder-agent.exe"
    } else {
        "reminder-agent"
    };

    let current_exe = std::env::current_exe()
        .map_err(|error| format!("failed to resolve settings executable path: {error}"))?;
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
        .map_err(|error| format!("failed to start reminder agent: {error}"))?;
    Ok(())
}

#[tauri::command]
fn pause_for_duration(minutes: u32) -> Result<(), String> {
    expect_ack(IpcRequest::PauseForDuration { minutes })
}

#[tauri::command]
fn resume() -> Result<(), String> {
    expect_ack(IpcRequest::Resume)
}

#[tauri::command]
fn get_history(limit: usize) -> Result<Vec<ReminderHistoryEntry>, String> {
    match send(IpcRequest::GetHistory { limit })? {
        IpcResponse::History(rows) => Ok(rows),
        other => Err(format!("unexpected agent response: {other:?}")),
    }
}

fn expect_ack(request: IpcRequest) -> Result<(), String> {
    match send(request)? {
        IpcResponse::Ack => Ok(()),
        other => Err(format!("unexpected agent response: {other:?}")),
    }
}

fn send(request: IpcRequest) -> Result<IpcResponse, String> {
    reminder_ipc::send_request(&request).map_err(|error| error.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_reminders,
            create_reminder,
            update_reminder,
            delete_reminder,
            set_reminder_enabled,
            preview_schedule,
            show_test_reminder,
            start_agent,
            pause_for_duration,
            resume,
            get_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running reminder settings");
}

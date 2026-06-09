use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Arc;

#[cfg(not(windows))]
use std::net::{TcpListener, TcpStream};
#[cfg(not(windows))]
use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, Utc};
use reminder_core::{DisplayPolicy, Priority, Reminder, ReminderId, ScheduleRule};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const IPC_ADDR: &str = "127.0.0.1:38741";
pub const IPC_PIPE_BASENAME: &str = "reminder-agent-v1";

#[derive(Debug, Error)]
pub enum IpcError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error("agent returned an error: {0}")]
    Remote(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcRequest {
    GetStatus,
    ListReminders,
    GetReminder {
        id: ReminderId,
    },
    CreateReminder {
        reminder: Reminder,
    },
    UpdateReminder {
        reminder: Reminder,
    },
    DeleteReminder {
        id: ReminderId,
    },
    SetReminderEnabled {
        id: ReminderId,
        enabled: bool,
    },
    PreviewSchedule {
        rule: ScheduleRule,
        after: Option<NaiveDateTime>,
        limit: usize,
    },
    ShowTestReminder {
        title: String,
        message: String,
        priority: Priority,
        policy: DisplayPolicy,
    },
    OpenSettings,
    PauseForDuration {
        minutes: u32,
    },
    Resume,
    GetHistory {
        limit: usize,
    },
    ReloadRules,
    ShutdownAgent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcResponse {
    Status(AgentStatus),
    Reminders(Vec<Reminder>),
    Reminder(Option<Reminder>),
    ReminderId(ReminderId),
    Changed { changed: bool },
    Preview(Vec<NaiveDateTime>),
    History(Vec<ReminderHistoryEntry>),
    Ack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub running: bool,
    pub enabled_reminders: usize,
    pub next_fire_at: Option<NaiveDateTime>,
    pub paused_until_utc: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderHistoryEntry {
    pub id: i64,
    pub reminder_id: ReminderId,
    pub fired_at_utc: DateTime<Utc>,
    pub displayed_at_utc: Option<DateTime<Utc>>,
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResponseEnvelope {
    ok: bool,
    response: Option<IpcResponse>,
    error: Option<String>,
}

impl ResponseEnvelope {
    fn ok(response: IpcResponse) -> Self {
        Self {
            ok: true,
            response: Some(response),
            error: None,
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            response: None,
            error: Some(message.into()),
        }
    }
}

#[cfg(windows)]
pub fn send_request(request: &IpcRequest) -> Result<IpcResponse, IpcError> {
    named_pipe::send_request(request)
}

#[cfg(not(windows))]
pub fn send_request(request: &IpcRequest) -> Result<IpcResponse, IpcError> {
    tcp::send_request(request)
}

#[cfg(windows)]
pub fn serve<F>(handler: F) -> Result<(), IpcError>
where
    F: Fn(IpcRequest) -> Result<IpcResponse, String> + Send + Sync + 'static,
{
    named_pipe::serve(handler)
}

#[cfg(not(windows))]
pub fn serve<F>(handler: F) -> Result<(), IpcError>
where
    F: Fn(IpcRequest) -> Result<IpcResponse, String> + Send + Sync + 'static,
{
    tcp::serve(handler)
}

fn send_request_over<T>(mut stream: T, request: &IpcRequest) -> Result<IpcResponse, IpcError>
where
    T: Read + Write,
{
    let mut payload = serde_json::to_vec(request)?;
    payload.push(b'\n');
    stream.write_all(&payload)?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;

    let envelope = serde_json::from_str::<ResponseEnvelope>(&line)?;
    if envelope.ok {
        envelope
            .response
            .ok_or_else(|| IpcError::Remote("empty success response".to_owned()))
    } else {
        Err(IpcError::Remote(
            envelope
                .error
                .unwrap_or_else(|| "unknown IPC error".to_owned()),
        ))
    }
}

fn handle_client<F, T>(mut stream: T, handler: Arc<F>) -> Result<(), IpcError>
where
    F: Fn(IpcRequest) -> Result<IpcResponse, String> + Send + Sync + 'static,
    T: Read + Write,
{
    let mut line = String::new();
    {
        let mut reader = BufReader::new(&mut stream);
        reader.read_line(&mut line)?;
    }

    let envelope = match serde_json::from_str::<IpcRequest>(&line) {
        Ok(request) => match handler(request) {
            Ok(response) => ResponseEnvelope::ok(response),
            Err(message) => ResponseEnvelope::error(message),
        },
        Err(error) => ResponseEnvelope::error(error.to_string()),
    };

    let mut payload = serde_json::to_vec(&envelope)?;
    payload.push(b'\n');
    stream.write_all(&payload)?;
    stream.flush()?;
    Ok(())
}

#[cfg(not(windows))]
mod tcp {
    use super::*;

    pub(super) fn send_request(request: &IpcRequest) -> Result<IpcResponse, IpcError> {
        let stream = TcpStream::connect(IPC_ADDR)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        send_request_over(stream, request)
    }

    pub(super) fn serve<F>(handler: F) -> Result<(), IpcError>
    where
        F: Fn(IpcRequest) -> Result<IpcResponse, String> + Send + Sync + 'static,
    {
        let listener = TcpListener::bind(IPC_ADDR)?;
        let handler = Arc::new(handler);

        for incoming in listener.incoming() {
            let stream = incoming?;
            let handler = handler.clone();
            std::thread::spawn(move || {
                if let Err(error) = handle_client(stream, handler) {
                    eprintln!("IPC client error: {error}");
                }
            });
        }

        Ok(())
    }
}

#[cfg(windows)]
mod named_pipe {
    use super::*;
    use std::ffi::{c_void, OsStr};
    use std::fs::{File, OpenOptions};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{FromRawHandle, RawHandle};
    use std::ptr;
    use std::thread;
    use std::time::{Duration, Instant};

    const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;
    const PIPE_TYPE_BYTE: u32 = 0x0000_0000;
    const PIPE_READMODE_BYTE: u32 = 0x0000_0000;
    const PIPE_WAIT: u32 = 0x0000_0000;
    const PIPE_UNLIMITED_INSTANCES: u32 = 255;
    const ERROR_PIPE_CONNECTED: u32 = 535;
    const INVALID_HANDLE_VALUE: RawHandle = -1isize as RawHandle;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateNamedPipeW(
            lpName: *const u16,
            dwOpenMode: u32,
            dwPipeMode: u32,
            nMaxInstances: u32,
            nOutBufferSize: u32,
            nInBufferSize: u32,
            nDefaultTimeOut: u32,
            lpSecurityAttributes: *mut c_void,
        ) -> RawHandle;
        fn ConnectNamedPipe(hNamedPipe: RawHandle, lpOverlapped: *mut c_void) -> i32;
        fn CloseHandle(hObject: RawHandle) -> i32;
        fn GetLastError() -> u32;
    }

    pub(super) fn send_request(request: &IpcRequest) -> Result<IpcResponse, IpcError> {
        let pipe = connect_pipe()?;
        send_request_over(pipe, request)
    }

    pub(super) fn serve<F>(handler: F) -> Result<(), IpcError>
    where
        F: Fn(IpcRequest) -> Result<IpcResponse, String> + Send + Sync + 'static,
    {
        let handler = Arc::new(handler);

        loop {
            let pipe = accept_pipe()?;
            let handler = handler.clone();
            thread::spawn(move || {
                if let Err(error) = handle_client(pipe, handler) {
                    eprintln!("IPC client error: {error}");
                }
            });
        }
    }

    fn connect_pipe() -> Result<File, IpcError> {
        let name = pipe_name();
        let deadline = Instant::now() + Duration::from_millis(500);

        loop {
            match OpenOptions::new().read(true).write(true).open(&name) {
                Ok(file) => return Ok(file),
                Err(error) => {
                    if Instant::now() >= deadline {
                        return Err(error.into());
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }

    fn accept_pipe() -> Result<File, IpcError> {
        let name = wide_null(&pipe_name());
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                ptr::null_mut(),
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }

        let connected = unsafe { ConnectNamedPipe(handle, ptr::null_mut()) };
        if connected == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_PIPE_CONNECTED {
                unsafe {
                    CloseHandle(handle);
                }
                return Err(std::io::Error::from_raw_os_error(error as i32).into());
            }
        }

        Ok(unsafe { File::from_raw_handle(handle) })
    }

    fn pipe_name() -> String {
        let user = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "default".to_owned());
        let suffix = user
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
            .collect::<String>();
        let suffix = if suffix.is_empty() {
            "default".to_owned()
        } else {
            suffix
        };

        format!(r"\\.\pipe\{}-{}", IPC_PIPE_BASENAME, suffix)
    }

    fn wide_null(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
}

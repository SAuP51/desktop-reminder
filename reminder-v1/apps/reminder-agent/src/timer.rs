#[cfg(not(windows))]
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::AgentEvent;

#[derive(Debug)]
pub struct TimerService {
    inner: PlatformTimerService,
}

impl TimerService {
    pub fn start(event_tx: Sender<AgentEvent>) -> Result<Self> {
        Ok(Self {
            inner: PlatformTimerService::start(event_tx)?,
        })
    }

    pub fn arm(&self, delay: Duration, generation: u64) -> Result<()> {
        self.inner.arm(delay, generation)
    }
}

#[cfg(not(windows))]
#[derive(Debug)]
struct PlatformTimerService {
    request_tx: Sender<TimerRequest>,
}

#[cfg(not(windows))]
#[derive(Debug)]
enum TimerRequest {
    Arm { delay: Duration, generation: u64 },
    Shutdown,
}

#[cfg(not(windows))]
impl PlatformTimerService {
    fn start(event_tx: Sender<AgentEvent>) -> Result<Self> {
        let (request_tx, request_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut current: Option<(Duration, u64)> = None;

            loop {
                match current.take() {
                    Some((delay, generation)) => match request_rx.recv_timeout(delay) {
                        Ok(TimerRequest::Arm { delay, generation }) => {
                            current = Some((delay, generation));
                        }
                        Ok(TimerRequest::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                            break
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if event_tx.send(AgentEvent::TimerElapsed(generation)).is_err() {
                                break;
                            }
                        }
                    },
                    None => match request_rx.recv() {
                        Ok(TimerRequest::Arm { delay, generation }) => {
                            current = Some((delay, generation));
                        }
                        Ok(TimerRequest::Shutdown) | Err(_) => break,
                    },
                }
            }
        });

        Ok(Self { request_tx })
    }

    fn arm(&self, delay: Duration, generation: u64) -> Result<()> {
        self.request_tx
            .send(TimerRequest::Arm { delay, generation })
            .context("failed to arm scheduler timer")
    }
}

#[cfg(not(windows))]
impl Drop for PlatformTimerService {
    fn drop(&mut self) {
        let _ = self.request_tx.send(TimerRequest::Shutdown);
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct PlatformTimerService {
    state: std::sync::Arc<std::sync::Mutex<TimerState>>,
    wake_event: std::sync::Arc<WinHandle>,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct TimerState {
    delay: Duration,
    generation: u64,
    shutdown: bool,
}

#[cfg(windows)]
#[derive(Debug)]
struct WinHandle(Handle);

#[cfg(windows)]
unsafe impl Send for WinHandle {}
#[cfg(windows)]
unsafe impl Sync for WinHandle {}

#[cfg(windows)]
impl Drop for WinHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
impl PlatformTimerService {
    fn start(event_tx: Sender<AgentEvent>) -> Result<Self> {
        let state = std::sync::Arc::new(std::sync::Mutex::new(TimerState {
            delay: Duration::from_secs(24 * 60 * 60),
            generation: 0,
            shutdown: false,
        }));
        let wake_event = std::sync::Arc::new(create_event()?);
        let thread_state = state.clone();
        let thread_wake = wake_event.clone();

        std::thread::Builder::new()
            .name("reminder-waitable-timer".to_owned())
            .spawn(move || run_waitable_timer(thread_state, thread_wake, event_tx))
            .context("failed to start scheduler timer thread")?;

        Ok(Self { state, wake_event })
    }

    fn arm(&self, delay: Duration, generation: u64) -> Result<()> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("timer state lock poisoned"))?;
            state.delay = delay;
            state.generation = generation;
        }

        set_event(self.wake_event.0).context("failed to wake scheduler timer")
    }
}

#[cfg(windows)]
impl Drop for PlatformTimerService {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.shutdown = true;
        }
        let _ = set_event(self.wake_event.0);
    }
}

#[cfg(windows)]
fn run_waitable_timer(
    state: std::sync::Arc<std::sync::Mutex<TimerState>>,
    wake_event: std::sync::Arc<WinHandle>,
    event_tx: Sender<AgentEvent>,
) {
    let timer = match create_waitable_timer() {
        Ok(timer) => timer,
        Err(error) => {
            tracing::error!(%error, "failed to create Windows waitable timer");
            return;
        }
    };

    loop {
        let snapshot = match state.lock() {
            Ok(state) => *state,
            Err(_) => {
                tracing::error!("timer state lock poisoned");
                break;
            }
        };

        if snapshot.shutdown {
            break;
        }

        if let Err(error) = set_waitable_timer(timer.0, snapshot.delay) {
            tracing::error!(%error, "failed to arm Windows waitable timer");
            break;
        }

        let handles = [timer.0, wake_event.0];
        let result = unsafe {
            WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), FALSE, INFINITE)
        };

        match result {
            WAIT_OBJECT_0 => {
                if event_tx
                    .send(AgentEvent::TimerElapsed(snapshot.generation))
                    .is_err()
                {
                    break;
                }
            }
            value if value == WAIT_OBJECT_0 + 1 => unsafe {
                CancelWaitableTimer(timer.0);
            },
            WAIT_FAILED => {
                tracing::error!(
                    error = %std::io::Error::last_os_error(),
                    "WaitForMultipleObjects failed"
                );
                break;
            }
            other => {
                tracing::warn!(other, "unexpected waitable timer result");
            }
        }
    }
}

#[cfg(windows)]
fn create_event() -> Result<WinHandle> {
    let handle = unsafe { CreateEventW(std::ptr::null_mut(), FALSE, FALSE, std::ptr::null()) };
    handle_from_raw(handle, "CreateEventW failed")
}

#[cfg(windows)]
fn create_waitable_timer() -> Result<WinHandle> {
    let handle = unsafe { CreateWaitableTimerW(std::ptr::null_mut(), FALSE, std::ptr::null()) };
    handle_from_raw(handle, "CreateWaitableTimerW failed")
}

#[cfg(windows)]
fn handle_from_raw(handle: Handle, message: &'static str) -> Result<WinHandle> {
    if handle == 0 {
        Err(std::io::Error::last_os_error()).context(message)
    } else {
        Ok(WinHandle(handle))
    }
}

#[cfg(windows)]
fn set_event(handle: Handle) -> Result<()> {
    let ok = unsafe { SetEvent(handle) };
    if ok == 0 {
        Err(std::io::Error::last_os_error()).context("SetEvent failed")
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn set_waitable_timer(handle: Handle, delay: Duration) -> Result<()> {
    let ticks_100ns = (delay.as_nanos() / 100).clamp(1, i64::MAX as u128) as i64;
    let due_time = -ticks_100ns;
    let ok = unsafe {
        SetWaitableTimer(
            handle,
            &due_time,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            FALSE,
        )
    };

    if ok == 0 {
        Err(std::io::Error::last_os_error()).context("SetWaitableTimer failed")
    } else {
        Ok(())
    }
}

#[cfg(windows)]
type Handle = isize;

#[cfg(windows)]
const FALSE: i32 = 0;
#[cfg(windows)]
const INFINITE: u32 = 0xffff_ffff;
#[cfg(windows)]
const WAIT_OBJECT_0: u32 = 0;
#[cfg(windows)]
const WAIT_FAILED: u32 = 0xffff_ffff;

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CancelWaitableTimer(handle: Handle) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn CreateEventW(
        security_attributes: *mut std::ffi::c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> Handle;
    fn CreateWaitableTimerW(
        timer_attributes: *mut std::ffi::c_void,
        manual_reset: i32,
        timer_name: *const u16,
    ) -> Handle;
    fn SetEvent(handle: Handle) -> i32;
    fn SetWaitableTimer(
        handle: Handle,
        due_time: *const i64,
        period: i32,
        completion_routine: *mut std::ffi::c_void,
        arg_to_completion_routine: *mut std::ffi::c_void,
        resume: i32,
    ) -> i32;
    fn WaitForMultipleObjects(
        count: u32,
        handles: *const Handle,
        wait_all: i32,
        milliseconds: u32,
    ) -> u32;
}

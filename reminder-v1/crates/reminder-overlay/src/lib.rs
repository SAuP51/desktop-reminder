use reminder_core::{DisplayPolicy, Priority, ReminderId};
use thiserror::Error;

#[cfg(windows)]
mod win32;

#[derive(Debug, Error)]
pub enum OverlayError {
    #[error("overlay backend error: {0}")]
    Backend(String),
}

#[derive(Debug, Clone)]
pub struct DisplayRequest {
    pub reminder_id: ReminderId,
    pub title: String,
    pub message: String,
    pub priority: Priority,
    pub policy: DisplayPolicy,
}

pub trait OverlayBackend {
    fn show(&mut self, request: DisplayRequest) -> Result<(), OverlayError>;
}

#[derive(Debug, Default)]
pub struct NoopOverlay;

impl OverlayBackend for NoopOverlay {
    fn show(&mut self, request: DisplayRequest) -> Result<(), OverlayError> {
        tracing::info!(
            reminder_id = %request.reminder_id,
            title = %request.title,
            message = %request.message,
            "display reminder via noop overlay"
        );
        Ok(())
    }
}

#[cfg(windows)]
pub type PlatformOverlay = win32::Win32Overlay;

#[cfg(not(windows))]
pub type PlatformOverlay = NoopOverlay;

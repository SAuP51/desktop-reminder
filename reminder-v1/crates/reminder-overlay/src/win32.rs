use reminder_core::DisplayPosition;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, LoadCursorW, PostQuitMessage,
    RegisterClassW, ShowWindow, TranslateMessage, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW,
    CW_USEDEFAULT, HMENU, IDC_ARROW, MSG, SW_HIDE, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CREATE, WM_DESTROY, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::{DisplayRequest, OverlayBackend, OverlayError};

const CLASS_NAME: &str = "ReminderOverlayWindow";

#[derive(Debug, Default)]
pub struct Win32Overlay {
    hwnd: Option<HWND>,
}

impl Win32Overlay {
    pub fn new() -> Result<Self, OverlayError> {
        Ok(Self { hwnd: None })
    }

    fn ensure_window(&mut self, request: &DisplayRequest) -> Result<HWND, OverlayError> {
        if let Some(hwnd) = self.hwnd {
            return Ok(hwnd);
        }

        unsafe {
            let instance =
                GetModuleHandleW(None).map_err(|error| OverlayError::Backend(error.to_string()))?;
            let class_name = wide_null(CLASS_NAME);
            let cursor = LoadCursorW(None, IDC_ARROW)
                .map_err(|error| OverlayError::Backend(error.to_string()))?;

            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                hCursor: cursor,
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };

            RegisterClassW(&wc);

            let mut ex_style: WINDOW_EX_STYLE =
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
            if request.policy.click_through {
                ex_style |= WS_EX_TRANSPARENT;
            }

            let hwnd = CreateWindowExW(
                ex_style,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(wide_null("Reminder Overlay").as_ptr()),
                WINDOW_STYLE(WS_POPUP.0),
                CW_USEDEFAULT,
                match request.policy.position {
                    DisplayPosition::Top => 32,
                    DisplayPosition::Middle => CW_USEDEFAULT,
                    DisplayPosition::Bottom => CW_USEDEFAULT,
                },
                900,
                request.policy.font_size.saturating_add(28) as i32,
                HWND(0),
                HMENU(0),
                instance,
                None,
            );

            if hwnd.0 == 0 {
                return Err(OverlayError::Backend("CreateWindowExW failed".to_owned()));
            }

            ShowWindow(hwnd, SW_HIDE);
            self.hwnd = Some(hwnd);
            Ok(hwnd)
        }
    }
}

impl OverlayBackend for Win32Overlay {
    fn show(&mut self, request: DisplayRequest) -> Result<(), OverlayError> {
        let hwnd = self.ensure_window(&request)?;
        tracing::info!(
            reminder_id = %request.reminder_id,
            title = %request.title,
            "Win32 overlay request accepted; scrolling renderer to be implemented next"
        );
        unsafe {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        Ok(())
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let _create_struct = lparam.0 as *const CREATESTRUCTW;
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[allow(dead_code)]
pub fn run_message_loop() {
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(0), 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

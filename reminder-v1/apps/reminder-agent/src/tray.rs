use std::sync::mpsc::Sender;

use anyhow::Result;
use reminder_core::{DisplayPolicy, Priority};
use reminder_overlay::DisplayRequest;
use uuid::Uuid;

use crate::{AgentCommand, AgentEvent};

#[cfg(not(windows))]
pub fn start(_event_tx: Sender<AgentEvent>) -> Result<()> {
    tracing::info!("native tray is only available on Windows");
    Ok(())
}

#[cfg(windows)]
pub fn start(event_tx: Sender<AgentEvent>) -> Result<()> {
    windows_tray::start(event_tx)
}

#[cfg(windows)]
mod windows_tray {
    use super::*;
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::ptr;
    use std::sync::{Mutex, OnceLock};

    type HINSTANCE = isize;
    type HWND = isize;
    type HICON = isize;
    type HCURSOR = isize;
    type HBRUSH = isize;
    type HMENU = isize;
    type LRESULT = isize;
    type WPARAM = usize;
    type LPARAM = isize;

    const CLASS_NAME: &str = "ReminderAgentTrayWindow";
    const WM_DESTROY: u32 = 0x0002;
    const WM_COMMAND: u32 = 0x0111;
    const WM_LBUTTONDBLCLK: u32 = 0x0203;
    const WM_LBUTTONUP: u32 = 0x0202;
    const WM_RBUTTONUP: u32 = 0x0205;
    const WM_USER: u32 = 0x0400;
    const TRAY_CALLBACK_MESSAGE: u32 = WM_USER + 1;

    const NIM_ADD: u32 = 0x0000_0000;
    const NIM_DELETE: u32 = 0x0000_0002;
    const NIM_SETVERSION: u32 = 0x0000_0004;
    const NIF_MESSAGE: u32 = 0x0000_0001;
    const NIF_ICON: u32 = 0x0000_0002;
    const NIF_TIP: u32 = 0x0000_0004;
    const NOTIFYICON_VERSION_4: u32 = 4;

    const IDI_APPLICATION: usize = 32512;
    const MF_STRING: u32 = 0x0000_0000;
    const MF_SEPARATOR: u32 = 0x0000_0800;
    const TPM_RIGHTBUTTON: u32 = 0x0000_0002;

    const MENU_OPEN_SETTINGS: usize = 1001;
    const MENU_TEST_REMINDER: usize = 1002;
    const MENU_PAUSE_30M: usize = 1003;
    const MENU_RESUME: usize = 1004;
    const MENU_NEXT_REMINDER: usize = 1005;
    const MENU_EXIT: usize = 1006;

    static EVENT_TX: OnceLock<Mutex<Sender<AgentEvent>>> = OnceLock::new();

    #[repr(C)]
    struct WndClassW {
        style: u32,
        lpfn_wnd_proc: Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>,
        cb_cls_extra: i32,
        cb_wnd_extra: i32,
        h_instance: HINSTANCE,
        h_icon: HICON,
        h_cursor: HCURSOR,
        hbr_background: HBRUSH,
        lpsz_menu_name: *const u16,
        lpsz_class_name: *const u16,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct Msg {
        hwnd: HWND,
        message: u32,
        w_param: WPARAM,
        l_param: LPARAM,
        time: u32,
        pt: Point,
        l_private: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    #[repr(C)]
    struct NotifyIconDataW {
        cb_size: u32,
        hwnd: HWND,
        uid: u32,
        uflags: u32,
        ucallback_message: u32,
        hicon: HICON,
        sztip: [u16; 128],
        dw_state: u32,
        dw_state_mask: u32,
        szinfo: [u16; 256],
        uversion_or_timeout: u32,
        szinfo_title: [u16; 64],
        dw_info_flags: u32,
        guid_item: Guid,
        hballoon_icon: HICON,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(lp_module_name: *const u16) -> HINSTANCE;
    }

    #[link(name = "shell32")]
    extern "system" {
        fn Shell_NotifyIconW(dw_message: u32, lpdata: *mut NotifyIconDataW) -> i32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn RegisterClassW(lp_wnd_class: *const WndClassW) -> u16;
        fn CreateWindowExW(
            dw_ex_style: u32,
            lp_class_name: *const u16,
            lp_window_name: *const u16,
            dw_style: u32,
            x: i32,
            y: i32,
            n_width: i32,
            n_height: i32,
            hwnd_parent: HWND,
            hmenu: HMENU,
            hinstance: HINSTANCE,
            lpparam: *mut c_void,
        ) -> HWND;
        fn DefWindowProcW(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
        fn DestroyWindow(hwnd: HWND) -> i32;
        fn PostQuitMessage(n_exit_code: i32);
        fn GetMessageW(
            lp_msg: *mut Msg,
            hwnd: HWND,
            msg_filter_min: u32,
            msg_filter_max: u32,
        ) -> i32;
        fn TranslateMessage(lp_msg: *const Msg) -> i32;
        fn DispatchMessageW(lp_msg: *const Msg) -> LRESULT;
        fn LoadIconW(hinstance: HINSTANCE, lp_icon_name: *const u16) -> HICON;
        fn CreatePopupMenu() -> HMENU;
        fn AppendMenuW(
            hmenu: HMENU,
            uflags: u32,
            uid_new_item: usize,
            lp_new_item: *const u16,
        ) -> i32;
        fn DestroyMenu(hmenu: HMENU) -> i32;
        fn GetCursorPos(lp_point: *mut Point) -> i32;
        fn SetForegroundWindow(hwnd: HWND) -> i32;
        fn TrackPopupMenu(
            hmenu: HMENU,
            uflags: u32,
            x: i32,
            y: i32,
            n_reserved: i32,
            hwnd: HWND,
            prc_rect: *const c_void,
        ) -> i32;
    }

    pub(super) fn start(event_tx: Sender<AgentEvent>) -> Result<()> {
        let _ = EVENT_TX.set(Mutex::new(event_tx));

        std::thread::Builder::new()
            .name("reminder-native-tray".to_owned())
            .spawn(|| {
                if let Err(error) = run_tray_window() {
                    tracing::error!(%error, "native tray stopped");
                }
            })?;

        Ok(())
    }

    fn run_tray_window() -> Result<()> {
        unsafe {
            let instance = GetModuleHandleW(ptr::null());
            let class_name = wide_null(CLASS_NAME);
            let wc = WndClassW {
                style: 0,
                lpfn_wnd_proc: Some(window_proc),
                cb_cls_extra: 0,
                cb_wnd_extra: 0,
                h_instance: instance,
                h_icon: 0,
                h_cursor: 0,
                hbr_background: 0,
                lpsz_menu_name: ptr::null(),
                lpsz_class_name: class_name.as_ptr(),
            };

            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                wide_null("Reminder Agent").as_ptr(),
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                instance,
                ptr::null_mut(),
            );

            if hwnd == 0 {
                return Err(std::io::Error::last_os_error().into());
            }

            add_tray_icon(hwnd)?;
            let mut msg: Msg = zeroed();
            while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        Ok(())
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            TRAY_CALLBACK_MESSAGE => match lparam as u32 {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                    send_command(AgentCommand::OpenSettings);
                    0
                }
                WM_RBUTTONUP => {
                    show_menu(hwnd);
                    0
                }
                _ => 0,
            },
            WM_COMMAND => {
                match wparam & 0xffff {
                    MENU_OPEN_SETTINGS => send_command(AgentCommand::OpenSettings),
                    MENU_TEST_REMINDER => send_command(AgentCommand::ShowTest(DisplayRequest {
                        reminder_id: Uuid::new_v4(),
                        title: "Test Reminder".to_owned(),
                        message: "Tray test reminder".to_owned(),
                        priority: Priority::Normal,
                        policy: DisplayPolicy::default(),
                    })),
                    MENU_PAUSE_30M => send_command(AgentCommand::PauseForDuration(30)),
                    MENU_RESUME => send_command(AgentCommand::Resume),
                    MENU_NEXT_REMINDER => send_command(AgentCommand::ShowNextReminder),
                    MENU_EXIT => {
                        send_command(AgentCommand::Shutdown);
                        DestroyWindow(hwnd);
                    }
                    _ => {}
                }
                0
            }
            WM_DESTROY => {
                delete_tray_icon(hwnd);
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    fn send_command(command: AgentCommand) {
        if let Some(tx) = EVENT_TX.get() {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(AgentEvent::Command(command));
            }
        }
    }

    unsafe fn show_menu(hwnd: HWND) {
        let menu = CreatePopupMenu();
        if menu == 0 {
            return;
        }

        append_menu_item(menu, MENU_OPEN_SETTINGS, "Open Settings");
        append_menu_item(menu, MENU_TEST_REMINDER, "Test Reminder");
        append_menu_item(menu, MENU_NEXT_REMINDER, "Next Reminder");
        AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
        append_menu_item(menu, MENU_PAUSE_30M, "Pause 30m");
        append_menu_item(menu, MENU_RESUME, "Resume");
        AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
        append_menu_item(menu, MENU_EXIT, "Exit");

        let mut point = Point { x: 0, y: 0 };
        GetCursorPos(&mut point);
        SetForegroundWindow(hwnd);
        TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            ptr::null(),
        );
        DestroyMenu(menu);
    }

    unsafe fn append_menu_item(menu: HMENU, id: usize, label: &str) {
        let label = wide_null(label);
        AppendMenuW(menu, MF_STRING, id, label.as_ptr());
    }

    fn add_tray_icon(hwnd: HWND) -> Result<()> {
        let mut data = notify_data(hwnd);
        data.uflags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.ucallback_message = TRAY_CALLBACK_MESSAGE;
        data.hicon = unsafe { LoadIconW(0, IDI_APPLICATION as *const u16) };
        write_wide_fixed(&mut data.sztip, "Reminder Agent");

        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &mut data) };
        if added == 0 {
            return Err(std::io::Error::last_os_error().into());
        }

        data.uversion_or_timeout = NOTIFYICON_VERSION_4;
        unsafe {
            Shell_NotifyIconW(NIM_SETVERSION, &mut data);
        }
        Ok(())
    }

    unsafe fn delete_tray_icon(hwnd: HWND) {
        let mut data = notify_data(hwnd);
        Shell_NotifyIconW(NIM_DELETE, &mut data);
    }

    fn notify_data(hwnd: HWND) -> NotifyIconDataW {
        NotifyIconDataW {
            cb_size: size_of::<NotifyIconDataW>() as u32,
            hwnd,
            uid: 1,
            uflags: 0,
            ucallback_message: 0,
            hicon: 0,
            sztip: [0; 128],
            dw_state: 0,
            dw_state_mask: 0,
            szinfo: [0; 256],
            uversion_or_timeout: 0,
            szinfo_title: [0; 64],
            dw_info_flags: 0,
            guid_item: Guid {
                data1: 0,
                data2: 0,
                data3: 0,
                data4: [0; 8],
            },
            hballoon_icon: 0,
        }
    }

    fn write_wide_fixed(target: &mut [u16], value: &str) {
        let wide = value.encode_utf16().collect::<Vec<_>>();
        let max = target.len().saturating_sub(1);
        let len = wide.len().min(max);
        target[..len].copy_from_slice(&wide[..len]);
        target[len] = 0;
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

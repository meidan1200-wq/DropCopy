//! The icon in the notification area (system tray): shows DropCopy is running, with the app's own
//! icon. Click it to open the received pictures; right-click for the pairing code or to quit.

use std::path::PathBuf;

pub struct Info {
    pub name: String,
    pub save_dir: PathBuf,
    /// The pairing link, for "Show pairing code".
    pub link: String,
}

/// Adds the icon; it stays until the process ends.
pub fn start(info: Info) {
    imp::start(info);
}

/// The tooltip line, kept short: Windows cuts it at 127 characters.
pub fn tooltip(name: &str) -> String {
    let t = format!("DropCopy - receiving as {name}");
    t.chars().take(127).collect()
}

#[cfg(not(windows))]
mod imp {
    pub fn start(_: super::Info) {}
}

#[cfg(windows)]
mod imp {
    use super::Info;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::OnceLock;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const WM_TRAY: u32 = WM_APP + 10;
    const ID_OPEN: usize = 1;
    const ID_PAIR: usize = 2;
    const ID_QUIT: usize = 3;
    const NIN_SELECT: u32 = WM_USER;
    const NIN_KEYSELECT: u32 = WM_USER + 1;

    static INFO: OnceLock<Info> = OnceLock::new();
    static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    unsafe fn data(hwnd: HWND) -> NOTIFYICONDATAW {
        let mut d = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: WM_TRAY,
            ..Default::default()
        };
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let (cx, cy) = (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON));
        // Resource 1 is the app icon (build.rs); Windows picks the best of its sizes.
        if let Ok(h) = LoadImageW(instance, PCWSTR(1 as _), IMAGE_ICON, cx, cy, LR_DEFAULTCOLOR) {
            d.hIcon = HICON(h.0);
        }
        let tip = wide(&super::tooltip(INFO.get().map(|i| i.name.as_str()).unwrap_or("this PC")));
        d.szTip[..tip.len().min(128)].copy_from_slice(&tip[..tip.len().min(128)]);
        d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        d
    }

    unsafe fn add(hwnd: HWND) {
        let d = data(hwnd);
        if d.hIcon.is_invalid() {
            crate::say!("  ! tray: the app icon is missing from this build");
        }
        if !Shell_NotifyIconW(NIM_ADD, &d).as_bool() {
            // No taskbar yet (very early at sign-in): "TaskbarCreated" adds it when it appears.
            crate::say!("  ! tray: the taskbar did not take the icon yet");
        }
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &d);
    }

    unsafe fn remove(hwnd: HWND) {
        let d = data(hwnd);
        let _ = Shell_NotifyIconW(NIM_DELETE, &d);
    }

    unsafe fn open_pictures() {
        if let Some(i) = INFO.get() {
            let _ = std::fs::create_dir_all(&i.save_dir);
            let p: Vec<u16> = i.save_dir.as_os_str().encode_wide().chain(Some(0)).collect();
            ShellExecuteW(None, w!("open"), PCWSTR(p.as_ptr()), None, None, SW_SHOWNORMAL);
        }
    }

    unsafe fn menu(hwnd: HWND, x: i32, y: i32) {
        let Ok(m) = CreatePopupMenu() else { return };
        let title = wide(&super::tooltip(INFO.get().map(|i| i.name.as_str()).unwrap_or("this PC")));
        let _ = AppendMenuW(m, MF_STRING | MF_GRAYED, 0, PCWSTR(title.as_ptr()));
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(m, MF_STRING, ID_OPEN, w!("Open received pictures"));
        let _ = AppendMenuW(m, MF_STRING, ID_PAIR, w!("Show pairing code"));
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(m, MF_STRING, ID_QUIT, w!("Quit (starts again when you sign in)"));
        // Without this the menu does not close when clicking elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(m, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN, x, y, 0, hwnd, None);
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(m);
        match cmd.0 as usize {
            ID_OPEN => open_pictures(),
            ID_PAIR => {
                if let Some(i) = INFO.get() {
                    crate::popup::show_code(i.link.clone());
                }
            }
            ID_QUIT => {
                remove(hwnd);
                crate::say!("  quit from the tray icon");
                std::process::exit(0);
            }
            _ => {}
        }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if Some(&msg) == TASKBAR_CREATED.get() {
            add(hwnd); // Explorer restarted: the icon must be added again
            return LRESULT(0);
        }
        match msg {
            WM_TRAY => {
                // Version 4: the event is in the low word; the anchor point in wParam.
                let event = (lp.0 & 0xffff) as u32;
                let (x, y) = ((wp.0 & 0xffff) as i16 as i32, ((wp.0 >> 16) & 0xffff) as i16 as i32);
                match event {
                    NIN_SELECT | NIN_KEYSELECT => open_pictures(),
                    WM_CONTEXTMENU => menu(hwnd, x, y),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_ENDSESSION | WM_DESTROY => {
                remove(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    pub fn start(info: Info) {
        if INFO.set(info).is_err() {
            return;
        }
        let _ = std::thread::Builder::new().name("tray".into()).spawn(|| unsafe {
            let instance = GetModuleHandleW(None).unwrap_or_default();
            let _ = TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
            let class = w!("DropCopyTray");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&wc);
            // A real (never shown) window: message-only windows miss "TaskbarCreated".
            let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("DropCopy"), WS_OVERLAPPED, 0, 0, 0, 0, None, None, instance, None) else {
                return;
            };
            add(hwnd);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tooltip_fits_windows_limit() {
        assert_eq!(super::tooltip("DESKTOP-4E1E3TB"), "DropCopy - receiving as DESKTOP-4E1E3TB");
        assert!(super::tooltip(&"x".repeat(300)).chars().count() <= 127);
    }
}

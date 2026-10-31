//! The icon in the notification area (system tray): shows DropCopy is running, with the app's own
//! icon. Click it to open the received pictures; right-click for the pairing code, an update
//! check, or to quit. This menu is the only window-style UI on the PC; the rest is the terminal.

use std::path::PathBuf;

pub struct Info {
    pub name: String,
    pub save_dir: PathBuf,
    /// The pairing link, for "Show pairing code".
    pub link: String,
    /// Checks for a newer release and installs it: `Ok(None)` up to date, `Ok(Some(v))` installed
    /// and the new copy started (this one should now quit).
    pub update: fn() -> Result<Option<String>, String>,
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
    const ID_UPDATE: usize = 4;
    const NIIF_USER: u32 = 4;
    const NIIF_LARGE_ICON: u32 = 0x20;
    /// The tray window, so the update thread can show its result.
    static HWND_TRAY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    static UPDATING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
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

    fn copy_wide(dst: &mut [u16], s: &str) {
        let w: Vec<u16> = s.encode_utf16().take(dst.len() - 1).chain(Some(0)).collect();
        dst[..w.len()].copy_from_slice(&w);
    }

    /// A Windows notification from the tray icon (it goes to the notification centre).
    unsafe fn notify(title: &str, text: &str) {
        let hwnd = HWND(HWND_TRAY.load(std::sync::atomic::Ordering::SeqCst) as _);
        if hwnd.0.is_null() {
            return;
        }
        let mut d = data(hwnd);
        d.uFlags = NIF_INFO;
        copy_wide(&mut d.szInfoTitle, title);
        copy_wide(&mut d.szInfo, text);
        d.dwInfoFlags = NOTIFY_ICON_INFOTIP_FLAGS(NIIF_USER | NIIF_LARGE_ICON);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
    }

    /// Off the UI thread: checking takes a moment and downloading longer.
    fn check_for_update() {
        use std::sync::atomic::Ordering::SeqCst;
        if UPDATING.swap(true, SeqCst) {
            return;
        }
        let Some(update) = INFO.get().map(|i| i.update) else { return };
        std::thread::spawn(move || unsafe {
            match update() {
                Ok(None) => notify("DropCopy is up to date", &format!("You have the latest version, v{}.", crate::update::current())),
                Ok(Some(v)) => {
                    notify("DropCopy updated", &format!("Now running v{v}."));
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    remove(HWND(HWND_TRAY.load(SeqCst) as _));
                    crate::say!("  updated to v{v} from the tray; restarting");
                    std::process::exit(0);
                }
                Err(e) => notify("Couldn't update DropCopy", &e),
            }
            UPDATING.store(false, SeqCst);
        });
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

    /// DropCopy's own menu (see `menu.rs`), opened at the icon; the choice comes back as WM_COMMAND.
    unsafe fn menu(hwnd: HWND, x: i32, y: i32) {
        use crate::menu::{Icon, Item};
        let busy = UPDATING.load(std::sync::atomic::Ordering::SeqCst);
        let it = |id, icon, label: &str, detail: String, enabled, separator| Item { id, icon, label: label.into(), detail, enabled, separator };
        let items = vec![
            it(ID_OPEN, Icon::Folder, "Open received pictures", String::new(), true, false),
            it(ID_PAIR, Icon::Qr, "Show pairing code", String::new(), true, false),
            it(ID_UPDATE, Icon::Update, if busy { "Checking for updates…" } else { "Check for updates" }, format!("v{}", crate::update::current()), !busy, false),
            it(ID_QUIT, Icon::Power, "Quit", "starts again at sign-in".into(), true, true),
        ];
        let name = INFO.get().map(|i| i.name.clone()).unwrap_or_else(|| "this PC".into());
        crate::menu::show((x, y), "DropCopy".into(), format!("Receiving as {name}"), items, hwnd.0 as usize);
    }

    unsafe fn run(hwnd: HWND, cmd: usize) {
        match cmd {
            ID_OPEN => open_pictures(),
            ID_PAIR => {
                if let Some(i) = INFO.get() {
                    crate::popup::show_code(i.link.clone());
                }
            }
            ID_UPDATE => check_for_update(),
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
            WM_COMMAND => {
                run(hwnd, wp.0 & 0xffff);
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
            let _ = windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
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
            HWND_TRAY.store(hwnd.0 as usize, std::sync::atomic::Ordering::SeqCst);
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

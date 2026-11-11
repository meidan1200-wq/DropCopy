//! Sending from this PC: to a phone (through the outbox, which the phone fetches) or to another
//! PC a phone introduced (directly, encrypted with the peer key). Used by Explorer's
//! "Send to > DropCopy" (`dropcopy share FILE...`) and the tray's "Send clipboard to …".

use crate::clipboard::Clip;
use crate::config::Config;
use crate::peers::Peer;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub enum Target {
    /// A phone, and how the menu names it: the name it gave ("Galaxy S20+"), with " (2)" for a
    /// second phone of that name.
    Phone { phone: crate::config::Phone, label: String },
    Pc(Peer),
}

impl Target {
    pub fn name(&self) -> &str {
        match self {
            Target::Phone { label, .. } => label,
            Target::Pc(p) => &p.name,
        }
    }
}

/// Phones that have connected (most recent first), then the PCs phones introduced.
pub fn targets(cfg: &Config) -> Vec<Target> {
    let mut t: Vec<Target> = vec![];
    for phone in crate::config::phone_list(&cfg.home) {
        // Two phones that keep their model's name: tell them apart in the menu.
        let same = t.iter().filter(|o| matches!(o, Target::Phone { phone: o, .. } if o.name == phone.name)).count();
        let label = if same == 0 { phone.name.clone() } else { format!("{} ({})", phone.name, same + 1) };
        t.push(Target::Phone { phone, label });
    }
    t.extend(crate::peers::load(&cfg.home).into_iter().map(Target::Pc));
    t
}

/// What to send.
pub struct What {
    pub files: Vec<PathBuf>,
    /// Temporary files that move into the outbox (a clipboard picture).
    pub owned: Vec<PathBuf>,
    pub text: Option<String>,
}

impl What {
    pub fn files(files: Vec<PathBuf>) -> What {
        What { files, owned: vec![], text: None }
    }

    /// The clipboard's contents; a picture is saved as a PNG first.
    pub fn clipboard(cfg: &Config) -> Result<What, String> {
        match crate::clipboard::get()? {
            Clip::Files(f) => Ok(What::files(f)),
            Clip::Text(t) => Ok(What { files: vec![], owned: vec![], text: Some(t) }),
            Clip::Image(png) => {
                let dir = cfg.home.join("outgoing");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let p = dir.join(format!("Clipboard {stamp}.png"));
                std::fs::write(&p, png).map_err(|e| e.to_string())?;
                Ok(What { files: vec![], owned: vec![p], text: None })
            }
            Clip::Empty => Err("The clipboard is empty: copy something first".into()),
        }
    }

    pub fn describe(&self) -> String {
        let n = self.files.len() + self.owned.len();
        match (n, &self.text) {
            (0, Some(t)) => {
                let t: String = t.chars().take(40).collect();
                format!("\u{201c}{t}\u{201d}")
            }
            (1, _) => self.files.iter().chain(&self.owned).next().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            (n, _) => format!("{n} files"),
        }
    }
}

/// How a send ended, for the notification.
pub struct Outcome {
    pub title: String,
    pub text: String,
    pub ok: bool,
}

/// How long to wait for a listening phone to collect before saying it will get it later.
const PHONE_WAIT: Duration = Duration::from_secs(8);

pub fn deliver(cfg: &Config, to: &Target, what: What) -> Outcome {
    let label = what.describe();
    match to {
        Target::Pc(peer) => {
            let mut items: Vec<crate::client::Item> = what.files.iter().chain(&what.owned).cloned().map(crate::client::Item::File).collect();
            items.extend(what.text.clone().map(crate::client::Item::Text));
            let r = crate::client::send_to_peer(cfg, peer, &items);
            for f in &what.owned {
                let _ = std::fs::remove_file(f);
            }
            match r {
                Ok(_) => Outcome { title: format!("Sent to {}", peer.name), text: format!("{label} is on its clipboard - Ctrl+V there"), ok: true },
                Err(e) => Outcome { title: format!("Couldn't send to {}", peer.name), text: e.to_string(), ok: false },
            }
        }
        Target::Phone { phone, label: name } => {
            let id = match crate::outbox::queue(&cfg.home, Some(phone), &what.files, &what.owned, what.text.as_deref()) {
                Ok(id) => id,
                Err(e) => return Outcome { title: format!("Couldn't send to {name}"), text: e.to_string(), ok: false },
            };
            crate::outbox::knock(&cfg.home, &cfg.id, &cfg.key);
            let t0 = Instant::now();
            while t0.elapsed() < PHONE_WAIT {
                if !crate::outbox::is_queued(&cfg.home, &id) {
                    return Outcome { title: format!("Sent to {name}"), text: format!("{label} is on the phone"), ok: true };
                }
                std::thread::sleep(Duration::from_millis(150));
                // Knock again once in a while: Wi-Fi may have dropped the first ones.
                if t0.elapsed().as_millis() % 2000 < 150 {
                    crate::outbox::knock(&cfg.home, &cfg.id, &cfg.key);
                }
            }
            Outcome {
                title: format!("Waiting for {name}"),
                text: format!("{label} goes to the phone as soon as DropCopy there is open or its screen is on"),
                ok: true,
            }
        }
    }
}

/// Explorer's "Send to" folder, where `setup` puts the DropCopy shortcut.
pub fn sendto_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| Path::new(&a).join("Microsoft").join("Windows").join("SendTo"))
}

/// Shows the targets as DropCopy's menu at the mouse pointer and returns the one picked.
pub fn pick(title: &str, status: &str, targets: &[Target]) -> Option<usize> {
    imp::pick(title, status, targets)
}

/// Puts "DropCopy" in Explorer's right-click "Send to" menu, running `exe share <files>`.
pub fn install_shortcut(exe: &Path) -> Result<(), String> {
    imp::install_shortcut(exe)
}

pub fn remove_shortcut() {
    if let Some(d) = sendto_dir() {
        let _ = std::fs::remove_file(d.join("DropCopy.lnk"));
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Target;
    pub fn pick(_: &str, _: &str, targets: &[Target]) -> Option<usize> {
        // No menu here: the first one (the most recent phone).
        (!targets.is_empty()).then_some(0)
    }
    pub fn install_shortcut(_: &std::path::Path) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(windows)]
mod imp {
    use super::Target;
    use crate::menu::{Icon, Item};
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use windows::core::{w, Interface, HSTRING};
    use windows::Win32::Foundation::*;
    use windows::Win32::System::Com::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    static CHOICE: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "system" fn owner(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_COMMAND {
            CHOICE.store(wp.0 & 0xffff, SeqCst);
            PostQuitMessage(0);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }

    pub fn pick(title: &str, status: &str, targets: &[Target]) -> Option<usize> {
        if targets.is_empty() {
            return None;
        }
        unsafe {
            let _ = windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(None).unwrap_or_default();
            let class = w!("DropCopyPicker");
            let wc = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(owner), hInstance: instance.into(), lpszClassName: class, ..Default::default() };
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("DropCopy"), WS_OVERLAPPED, 0, 0, 0, 0, None, None, instance, None).ok()?;
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let items = targets
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let (icon, kind) = match t {
                        Target::Phone { .. } => (Icon::Phone, "Phone"),
                        Target::Pc(_) => (Icon::Laptop, "PC"),
                    };
                    Item { id: i + 1, icon, label: t.name().to_string(), detail: kind.into(), enabled: true, separator: false }
                })
                .collect();
            CHOICE.store(0, SeqCst);
            crate::menu::show((pt.x, pt.y), title.to_string(), status.to_string(), items, hwnd.0 as usize, 0, crate::menu::Dot::Hidden);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let _ = DestroyWindow(hwnd);
            CHOICE.load(SeqCst).checked_sub(1)
        }
    }

    pub fn install_shortcut(exe: &std::path::Path) -> Result<(), String> {
        let dir = super::sendto_dir().ok_or("APPDATA is not set")?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
            let exe_h = HSTRING::from(exe.as_os_str());
            link.SetPath(&exe_h).map_err(|e| e.to_string())?;
            link.SetArguments(w!("share")).map_err(|e| e.to_string())?;
            link.SetDescription(w!("Send to your phone or another PC with DropCopy")).map_err(|e| e.to_string())?;
            link.SetIconLocation(&exe_h, 0).map_err(|e| e.to_string())?;
            let file: IPersistFile = link.cast().map_err(|e| e.to_string())?;
            file.Save(&HSTRING::from(dir.join("DropCopy.lnk").as_os_str()), true).map_err(|e| e.to_string())
        }
    }
}

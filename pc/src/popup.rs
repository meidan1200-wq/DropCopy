//! The drop card: when a photo arrives (or the phone is brought close), a small card with its
//! thumbnail slides in at the bottom-right of the screen. Drag it into any app to drop the file
//! there, click it to open, or ignore it and it fades away. It never takes keyboard focus.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Files received recently, newest batch last, for "show me again when the phone comes close".
static RECENT: Mutex<Vec<(Instant, Vec<PathBuf>)>> = Mutex::new(Vec::new());
const RECENT_FOR: Duration = Duration::from_secs(15 * 60);

pub fn remember(files: &[PathBuf]) {
    if files.is_empty() {
        return;
    }
    let mut r = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    r.retain(|(t, _)| t.elapsed() < RECENT_FOR);
    r.push((Instant::now(), files.to_vec()));
    if r.len() > 20 {
        r.remove(0);
    }
}

/// The newest batch received in the last 15 minutes whose files still exist.
pub fn latest() -> Option<Vec<PathBuf>> {
    let r = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    r.iter()
        .rev()
        .filter(|(t, _)| t.elapsed() < RECENT_FOR)
        .map(|(_, f)| f.iter().filter(|p| p.exists()).cloned().collect::<Vec<_>>())
        .find(|f| !f.is_empty())
}

/// Shows the card for `files` with a heading such as "From your phone".
pub fn show(files: Vec<PathBuf>, heading: &'static str) {
    if !files.is_empty() {
        imp::show(files, heading);
    }
}

/// Builds the drag data exactly as a drag from the phone would, and reads the file list back out.
/// Returns how many files a drop target would receive.
pub fn check_drag_data(files: &[PathBuf]) -> Result<usize, String> {
    imp::check_drag_data(files)
}

#[cfg(not(windows))]
mod imp {
    pub fn show(_: Vec<std::path::PathBuf>, _: &'static str) {}
    pub fn check_drag_data(_: &[std::path::PathBuf]) -> Result<usize, String> {
        Err("drag and drop is only available on Windows".into())
    }
}

#[cfg(windows)]
mod imp {
    use crate::phone_art::{self, Art, MARGIN};
    use std::cell::RefCell;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::mpsc::channel;
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::Com::{CoTaskMemFree, IDataObject};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Ole::{OleInitialize, DROPEFFECT_COPY};
    use windows::Win32::UI::HiDpi::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const WM_APP_SHOW: u32 = WM_APP + 2;
    const WM_MOUSELEAVE: u32 = 0x02A3;
    const TIMER_ANIM: usize = 1;
    const TIMER_HIDE: usize = 2;
    const SHOW_FOR_MS: u32 = 6000;
    const IN_MS: f32 = 220.0;
    const OUT_MS: f32 = 170.0;

    /// A rendered phone, in both states, ready to put on screen.
    struct Card {
        files: Vec<PathBuf>,
        normal: Bitmap,
        hover: Bitmap,
        close: (f32, f32, f32),
        scale: f32,
    }

    /// Premultiplied BGRA pixels in a GDI DIB section.
    struct Bitmap {
        dc: HDC,
        bmp: HBITMAP,
        w: i32,
        h: i32,
    }

    impl Bitmap {
        unsafe fn new(art: &Art) -> Option<Bitmap> {
            let (w, h) = (art.image.width() as i32, art.image.height() as i32);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            let px = phone_art::premultiplied_bgra(&art.image);
            std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u8, px.len());
            let dc = CreateCompatibleDC(None);
            SelectObject(dc, bmp);
            Some(Bitmap { dc, bmp, w, h })
        }
    }

    impl Drop for Bitmap {
        fn drop(&mut self) {
            unsafe {
                let _ = DeleteDC(self.dc);
                let _ = DeleteObject(self.bmp);
            }
        }
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Phase {
        Hidden,
        In(Instant),
        Shown,
        Out(Instant),
    }

    struct State {
        card: Option<Card>,
        phase: Phase,
        hovered: bool,
        press: Option<POINT>,
        home: POINT,
    }

    thread_local! {
        static STATE: RefCell<State> = const { RefCell::new(State {
            card: None, phase: Phase::Hidden, hovered: false, press: None, home: POINT { x: 0, y: 0 },
        }) };
    }

    struct Prepared {
        files: Vec<PathBuf>,
        normal: Art,
        hover: Art,
        scale: f32,
    }

    static UI: OnceLock<Option<usize>> = OnceLock::new();
    static PENDING: Mutex<Option<Prepared>> = Mutex::new(None);

    /// Renders the phone off the UI thread, then hands it over.
    pub fn show(files: Vec<PathBuf>, _heading: &'static str) {
        std::thread::spawn(move || {
            let Some(hwnd) = ui() else { return };
            let scale = unsafe { GetDpiForSystem() }.max(96) as f32 / 96.0;
            let photo = std::fs::read(&files[0]).ok().and_then(|raw| crate::clipboard::decode(&raw)).map(|(i, _)| i);
            let n = files.len();
            let normal = phone_art::render(photo.as_ref(), scale, n, false);
            let hover = phone_art::render(photo.as_ref(), scale, n, true);
            *PENDING.lock().unwrap() = Some(Prepared { files, normal, hover, scale });
            unsafe {
                let _ = PostMessageW(HWND(hwnd as _), WM_APP_SHOW, WPARAM(0), LPARAM(0));
            }
        });
    }

    fn ui() -> Option<usize> {
        *UI.get_or_init(|| {
            let (ready_tx, ready_rx) = channel();
            std::thread::Builder::new()
                .name("popup".into())
                .spawn(move || unsafe {
                    let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
                    let _ = OleInitialize(None);
                    let instance = GetModuleHandleW(None).unwrap_or_default();
                    let class = w!("DropCopyPhone");
                    let wc = WNDCLASSEXW {
                        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                        lpfnWndProc: Some(wndproc),
                        hInstance: instance.into(),
                        hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
                        lpszClassName: class,
                        ..Default::default()
                    };
                    RegisterClassExW(&wc);
                    let hwnd = CreateWindowExW(
                        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                        class, w!("DropCopy"), WS_POPUP, 0, 0, 1, 1, None, None, instance, None,
                    );
                    let Ok(hwnd) = hwnd else {
                        let _ = ready_tx.send(None);
                        return;
                    };
                    let _ = ready_tx.send(Some(hwnd.0 as usize));
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                })
                .ok()?;
            ready_rx.recv().ok().flatten()
        })
    }

    fn ease_out(t: f32) -> f32 {
        1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
    }

    /// Puts the current frame on screen: `lift` 0 = tucked into the taskbar, 1 = fully up.
    unsafe fn present(hwnd: HWND, lift: f32) {
        STATE.with(|s| {
            let s = s.borrow();
            let Some(card) = s.card.as_ref() else { return };
            let bmp = if s.hovered { &card.hover } else { &card.normal };
            let drop = (28.0 * card.scale * (1.0 - lift)) as i32;
            let pos = POINT { x: s.home.x, y: s.home.y + drop };
            let size = SIZE { cx: bmp.w, cy: bmp.h };
            let src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: (255.0 * lift.clamp(0.0, 1.0)) as u8,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(hwnd, None, Some(&pos), Some(&size), bmp.dc, Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
        });
    }

    /// Centred on the taskbar's side of the screen, its foot just above the taskbar.
    unsafe fn home(w: i32, h: i32, scale: f32) -> POINT {
        let mut work = RECT::default();
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut work as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let gap = (6.0 * scale) as i32;
        let foot = (MARGIN * scale) as i32; // shadow margin below the handset
        POINT { x: (work.left + work.right - w) / 2, y: work.bottom - h + foot - gap }
    }

    unsafe fn start_hide(hwnd: HWND) {
        let _ = KillTimer(hwnd, TIMER_HIDE);
        let leaving = STATE.with(|s| {
            let mut s = s.borrow_mut();
            if matches!(s.phase, Phase::Shown | Phase::In(_)) {
                s.phase = Phase::Out(Instant::now());
                true
            } else {
                false
            }
        });
        if leaving {
            SetTimer(hwnd, TIMER_ANIM, 10, None);
        }
    }

    /// Drags the files out with the shell's own data object, so the target app gets exactly what
    /// dragging them from Explorer would give it.
    unsafe fn drag(hwnd: HWND, files: &[PathBuf]) {
        if let Ok(data) = data_object(files) {
            let _ = SHDoDragDrop(hwnd, &data, None, DROPEFFECT_COPY);
        }
    }

    unsafe fn data_object(files: &[PathBuf]) -> Result<IDataObject, String> {
        let mut pidls: Vec<*mut ITEMIDLIST> = Vec::new();
        for f in files {
            let w: Vec<u16> = f.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut p: *mut ITEMIDLIST = std::ptr::null_mut();
            if SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut p, 0, None).is_ok() && !p.is_null() {
                pidls.push(p);
            }
        }
        let result = if pidls.is_empty() {
            Err("the shell does not know these files".to_string())
        } else {
            // Explorer's own way: ask the folder that holds the files for their data object.
            // (Received files all live in the one save folder.)
            let classic = (|| -> windows::core::Result<IDataObject> {
                let mut child: *mut ITEMIDLIST = std::ptr::null_mut();
                let folder: IShellFolder = SHBindToParent(pidls[0], Some(&mut child as *mut _ as *mut _))?;
                let children: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| ILFindLastID(*p) as *const _).collect();
                folder.GetUIObjectOf(HWND::default(), &children, None)
            })();
            classic
                .or_else(|_| {
                    let list: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| *p as *const _).collect();
                    SHCreateShellItemArrayFromIDLists(&list)
                        .and_then(|items| items.BindToHandler::<_, IDataObject>(None, &BHID_DataObject))
                })
                .map_err(|e| e.to_string())
        };
        for p in pidls {
            CoTaskMemFree(Some(p as *const _));
        }
        result
    }

    pub fn check_drag_data(files: &[PathBuf]) -> Result<usize, String> {
        use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
        use windows::Win32::System::Ole::ReleaseStgMedium;
        unsafe {
            let _ = OleInitialize(None);
            let data = data_object(files)?;
            let fmt = FORMATETC { cfFormat: 15, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
            let mut medium = data.GetData(&fmt).map_err(|e| format!("no file list: {e}"))?;
            let hdrop = HDROP(medium.u.hGlobal.0);
            let n = DragQueryFileW(hdrop, u32::MAX, None);
            ReleaseStgMedium(&mut medium);
            Ok(n as usize)
        }
    }

    fn point(lp: LPARAM) -> POINT {
        POINT { x: (lp.0 & 0xffff) as i16 as i32, y: ((lp.0 >> 16) & 0xffff) as i16 as i32 }
    }

    fn on_close(card: &Card, p: POINT) -> bool {
        let (cx, cy, r) = card.close;
        ((p.x as f32 - cx).powi(2) + (p.y as f32 - cy).powi(2)).sqrt() <= r + 2.0 * card.scale
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_APP_SHOW => {
                let Some(p) = PENDING.lock().unwrap().take() else { return LRESULT(0) };
                let (Some(normal), Some(hover)) = (Bitmap::new(&p.normal), Bitmap::new(&p.hover)) else {
                    return LRESULT(0);
                };
                let h = home(normal.w, normal.h, p.scale);
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    let was_up = matches!(s.phase, Phase::Shown);
                    s.card = Some(Card { files: p.files, close: p.normal.close, scale: p.scale, normal, hover });
                    s.home = h;
                    s.hovered = false;
                    // Already up: swap the photo in place. Otherwise rise from the taskbar.
                    s.phase = if was_up { Phase::Shown } else { Phase::In(Instant::now()) };
                });
                present(hwnd, if STATE.with(|s| s.borrow().phase == Phase::Shown) { 1.0 } else { 0.0 });
                let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
                SetTimer(hwnd, TIMER_ANIM, 10, None);
                SetTimer(hwnd, TIMER_HIDE, SHOW_FOR_MS, None);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_ANIM => {
                let phase = STATE.with(|s| s.borrow().phase);
                match phase {
                    Phase::In(t0) => {
                        let t = t0.elapsed().as_millis() as f32 / IN_MS;
                        present(hwnd, ease_out(t));
                        if t >= 1.0 {
                            STATE.with(|s| s.borrow_mut().phase = Phase::Shown);
                            let _ = KillTimer(hwnd, TIMER_ANIM);
                        }
                    }
                    Phase::Out(t0) => {
                        let t = t0.elapsed().as_millis() as f32 / OUT_MS;
                        present(hwnd, 1.0 - ease_out(t));
                        if t >= 1.0 {
                            let _ = KillTimer(hwnd, TIMER_ANIM);
                            let _ = ShowWindow(hwnd, SW_HIDE);
                            STATE.with(|s| {
                                let mut s = s.borrow_mut();
                                s.phase = Phase::Hidden;
                                s.card = None; // frees the bitmaps
                            });
                        }
                    }
                    _ => {
                        let _ = KillTimer(hwnd, TIMER_ANIM);
                    }
                }
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_HIDE => {
                if !STATE.with(|s| s.borrow().hovered) {
                    start_hide(hwnd);
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let pt = point(lp);
                let (was_hovered, press, phase) = STATE.with(|s| {
                    let s = s.borrow();
                    (s.hovered, s.press, s.phase)
                });
                if !was_hovered {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tme);
                    STATE.with(|s| {
                        let mut s = s.borrow_mut();
                        s.hovered = true;
                        // Caught it while it was leaving: bring it back up.
                        if matches!(s.phase, Phase::Out(_)) {
                            s.phase = Phase::In(Instant::now());
                        }
                    });
                    if matches!(phase, Phase::Out(_)) {
                        SetTimer(hwnd, TIMER_ANIM, 10, None);
                    } else {
                        present(hwnd, 1.0);
                    }
                }
                if let Some(p) = press {
                    let (dx, dy) = (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG));
                    if (pt.x - p.x).abs() > dx || (pt.y - p.y).abs() > dy {
                        STATE.with(|s| s.borrow_mut().press = None);
                        let _ = ReleaseCapture();
                        let files = STATE.with(|s| s.borrow().card.as_ref().map(|c| c.files.clone()));
                        if let Some(files) = files {
                            drag(hwnd, &files);
                            start_hide(hwnd);
                        }
                    }
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                STATE.with(|s| s.borrow_mut().hovered = false);
                if STATE.with(|s| s.borrow().phase == Phase::Shown) {
                    present(hwnd, 1.0);
                }
                SetTimer(hwnd, TIMER_HIDE, 2000, None);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                STATE.with(|s| s.borrow_mut().press = Some(point(lp)));
                SetCapture(hwnd);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let pt = point(lp);
                let pressed = STATE.with(|s| s.borrow_mut().press.take()).is_some();
                let _ = ReleaseCapture();
                if pressed {
                    let open = STATE.with(|s| {
                        s.borrow().card.as_ref().filter(|c| !on_close(c, pt)).map(|c| c.files[0].clone())
                    });
                    if let Some(f) = open {
                        let w: Vec<u16> = f.as_os_str().encode_wide().chain(Some(0)).collect();
                        ShellExecuteW(None, w!("open"), PCWSTR(w.as_ptr()), None, None, SW_SHOWNORMAL);
                    }
                    start_hide(hwnd);
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                start_hide(hwnd);
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

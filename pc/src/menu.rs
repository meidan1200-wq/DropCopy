//! The tray icon's menu, drawn by DropCopy instead of the classic Win32 menu: a rounded panel with
//! a soft shadow, a header with the app icon and its status, rows with line icons and a hover
//! highlight. Follows Windows' light or dark app theme.
//!
//! Everything but the text is drawn here in plain Rust (signed distances, anti-aliased at any
//! display scale), so the shapes can be checked on any OS; Windows adds the text with GDI.

use image::{Rgba, RgbaImage};

/// Logical (96-dpi) sizes.
pub const SHADOW: f32 = 16.0;
pub const WIDTH: f32 = 300.0;
const PAD: f32 = 6.0;
const HEADER: f32 = 62.0;
const ROW: f32 = 40.0;
const SEP: f32 = 9.0;
const RADIUS: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Icon {
    Folder,
    Qr,
    Update,
    Power,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: usize,
    pub icon: Icon,
    pub label: String,
    /// Shown muted at the right, e.g. the version.
    pub detail: String,
    pub enabled: bool,
    /// A thin line above this row.
    pub separator: bool,
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub panel: [u8; 3],
    pub border: [u8; 3],
    pub hover: [u8; 3],
    pub line: [u8; 3],
    pub text: [u8; 3],
    pub muted: [u8; 3],
    pub ok: [u8; 3],
}

pub const DARK: Theme = Theme {
    panel: [0x1F, 0x20, 0x24],
    border: [0x34, 0x36, 0x3C],
    hover: [0x2D, 0x2F, 0x36],
    line: [0x33, 0x35, 0x3B],
    text: [0xF1, 0xF2, 0xF4],
    muted: [0x9A, 0x9F, 0xA8],
    ok: [0x3D, 0xD6, 0x8C],
};

pub const LIGHT: Theme = Theme {
    panel: [0xFB, 0xFB, 0xFC],
    border: [0xDD, 0xDF, 0xE3],
    hover: [0xEC, 0xEE, 0xF1],
    line: [0xE4, 0xE6, 0xEA],
    text: [0x1A, 0x1C, 0x20],
    muted: [0x5E, 0x64, 0x6D],
    ok: [0x1E, 0x9E, 0x5A],
};

/// Where everything goes, in output pixels.
#[derive(Debug, Clone)]
pub struct Layout {
    pub scale: f32,
    pub w: u32,
    pub h: u32,
    /// The panel inside the shadow margin: x, y, w, h.
    pub panel: (f32, f32, f32, f32),
    /// The header: icon square (x, y, size) and the two text lines' top y.
    pub header_icon: (f32, f32, f32),
    pub title_y: f32,
    pub status_y: f32,
    pub text_x: f32,
    /// One per item: top and bottom y.
    pub rows: Vec<(f32, f32)>,
    /// y of each separator line.
    pub lines: Vec<f32>,
}

pub fn layout(items: &[Item], scale: f32) -> Layout {
    let s = scale.max(1.0);
    let m = SHADOW * s;
    let (px, pw) = (m, WIDTH * s);
    let mut y = m + PAD * s;
    let header_icon = (px + 16.0 * s, y + (HEADER * s - 32.0 * s) / 2.0, 32.0 * s);
    let title_y = y + 12.0 * s;
    let status_y = y + 33.0 * s;
    y += HEADER * s;
    let mut lines = vec![y + SEP * s / 2.0];
    y += SEP * s;
    let mut rows = vec![];
    for (i, it) in items.iter().enumerate() {
        if it.separator && i > 0 {
            lines.push(y + SEP * s / 2.0);
            y += SEP * s;
        }
        rows.push((y, y + ROW * s));
        y += ROW * s;
    }
    y += PAD * s;
    let ph = y - m;
    Layout {
        scale: s,
        w: (pw + 2.0 * m).ceil() as u32,
        h: (ph + 2.0 * m).ceil() as u32,
        panel: (px, m, pw, ph),
        header_icon,
        title_y,
        status_y,
        text_x: px + 60.0 * s,
        rows,
        lines,
    }
}

/// The row under (x, y) in window pixels, if it can be picked.
pub fn hit(l: &Layout, items: &[Item], x: f32, y: f32) -> Option<usize> {
    let (px, _, pw, _) = l.panel;
    if x < px || x > px + pw {
        return None;
    }
    l.rows.iter().position(|(a, b)| y >= *a && y < *b).filter(|&i| items[i].enabled)
}

/// Window position for a menu opened at `anchor` (the click, in screen pixels), kept inside the
/// work area `(left, top, right, bottom)`: above the point when the taskbar is at the bottom,
/// below it when the taskbar is at the top, its edge lined up with the point like Windows' own.
pub fn place(l: &Layout, anchor: (i32, i32), work: (i32, i32, i32, i32)) -> (i32, i32) {
    let m = (SHADOW * l.scale) as i32;
    let gap = (6.0 * l.scale) as i32;
    let (pw, ph) = (l.panel.2 as i32, l.panel.3 as i32);
    let (left, top, right, bottom) = work;
    let mut px = anchor.0 - pw / 2;
    px = px.min(right - pw - gap).max(left + gap);
    let above = anchor.1 - gap - ph;
    let py = if above >= top { above.min(bottom - ph - gap) } else { (anchor.1 + gap).max(top + gap).min(bottom - ph - gap) };
    (px - m, py - m)
}

fn rbox(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let qx = (px - cx).abs() - (w / 2.0 - r);
    let qy = (py - cy).abs() - (h / 2.0 - r);
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r
}

fn seg(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((px - a.0) * dx + (py - a.1) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
    ((px - a.0 - t * dx).powi(2) + (py - a.1 - t * dy).powi(2)).sqrt()
}

fn cover(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// Signed distance to a line icon in a 16×16 box at (x, y) of size `z` pixels: strokes ~1.4 units.
fn icon_d(icon: Icon, px: f32, py: f32, x: f32, y: f32, z: f32) -> f32 {
    // Into icon units (0..16).
    let k = 16.0 / z;
    let (u, v) = ((px - x) * k, (py - y) * k);
    let half = 0.7; // half stroke
    let d = match icon {
        Icon::Folder => {
            // Body outline, and the tab on top-left.
            let body = rbox(u, v, 1.5, 4.5, 13.0, 9.5, 1.6).abs();
            let tab = seg(u, v, (1.8, 4.3), (2.2, 2.6)).min(seg(u, v, (2.2, 2.6), (6.0, 2.6))).min(seg(u, v, (6.0, 2.6), (7.4, 4.3)));
            body.min(tab)
        }
        Icon::Qr => {
            // Three hollow finder squares and a few modules, like a real code.
            let corner = |cx: f32, cy: f32| rbox(u, v, cx, cy, 5.4, 5.4, 1.0).abs();
            let mut d = corner(1.0, 1.0).min(corner(9.6, 1.0)).min(corner(1.0, 9.6));
            for (cx, cy) in [(10.0, 10.0), (13.0, 13.0), (13.0, 10.0), (10.0, 13.0)] {
                d = d.min(rbox(u, v, cx - 0.2, cy - 0.2, 2.0, 2.0, 0.4) - 0.0).max(-10.0).min(d);
            }
            d
        }
        Icon::Update => {
            // A circular arrow: most of a ring, with an arrowhead at its end.
            let (cx, cy, r) = (8.0, 8.4, 5.6);
            let ang = (v - cy).atan2(u - cx); // -pi..pi, 0 = right
            let ring = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() - r;
            let gap = ang > -1.9 && ang < -0.7; // open at the top-right
            let arc = if gap { f32::MAX } else { ring.abs() };
            let tip = (cx + r * (-0.7f32).cos(), cy + r * (-0.7f32).sin());
            let head = seg(u, v, tip, (tip.0 - 3.2, tip.1 - 0.3)).min(seg(u, v, tip, (tip.0 + 0.4, tip.1 - 3.2)));
            arc.min(head)
        }
        Icon::Power => {
            let (cx, cy, r) = (8.0, 8.8, 5.4);
            let ang = (v - cy).atan2(u - cx);
            let ring = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() - r;
            let gap = ang > -2.2 && ang < -0.94;
            let arc = if gap { f32::MAX } else { ring.abs() };
            arc.min(seg(u, v, (8.0, 1.6), (8.0, 7.6)))
        }
    };
    (d - half) / k
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [f32; 3] {
    let f = |i: usize| a[i] as f32 + (b[i] as f32 - a[i] as f32) * t;
    [f(0), f(1), f(2)]
}

/// Everything except the text: shadow, panel, border, hover, lines, icons, the status dot.
/// Pixels that GDI will write text over are fully opaque. `hover` is a row index.
pub fn draw(l: &Layout, items: &[Item], hover: Option<usize>, t: &Theme) -> RgbaImage {
    let s = l.scale;
    let (px, py, pw, ph) = l.panel;
    let r = RADIUS * s;
    let mut img = RgbaImage::new(l.w, l.h);
    let icon_z = 16.0 * s;
    let dot = (l.text_x + 4.0 * s, l.status_y + 8.0 * s, 3.5 * s);
    for (x, y, out) in img.enumerate_pixels_mut() {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let d = rbox(fx, fy, px, py, pw, ph, r);
        // Soft shadow under the panel, a little lower.
        let ds = rbox(fx, fy, px, py + 3.0 * s, pw, ph, r);
        let blur = 14.0 * s;
        let shadow = 0.32 * (1.0 - ((ds + blur * 0.25) / blur).clamp(0.0, 1.0)).powi(2);
        let a = cover(d);
        if a <= 0.0 {
            *out = Rgba([0, 0, 0, (shadow * 255.0) as u8]);
            continue;
        }
        let mut c = mix(t.panel, t.panel, 0.0);
        // Hover: a rounded highlight inset from the panel edge.
        if let Some(hv) = hover {
            if let Some(&(a0, a1)) = l.rows.get(hv) {
                let dh = rbox(fx, fy, px + 5.0 * s, a0 + 1.0 * s, pw - 10.0 * s, a1 - a0 - 2.0 * s, 6.0 * s);
                c = mix(t.panel, t.hover, cover(dh));
            }
        }
        for &ly in &l.lines {
            let dl = seg(fx, fy, (px + 12.0 * s, ly), (px + pw - 12.0 * s, ly)) - 0.5 * s.max(1.0) * 0.5;
            let k = cover(dl);
            if k > 0.0 {
                c = [c[0] + (t.line[0] as f32 - c[0]) * k, c[1] + (t.line[1] as f32 - c[1]) * k, c[2] + (t.line[2] as f32 - c[2]) * k];
            }
        }
        for (i, it) in items.iter().enumerate() {
            let (a0, a1) = l.rows[i];
            if fy < a0 || fy > a1 {
                continue;
            }
            let ix = px + 20.0 * s;
            let iy = a0 + (a1 - a0 - icon_z) / 2.0;
            let k = cover(icon_d(it.icon, fx, fy, ix, iy, icon_z));
            if k > 0.0 {
                let col = if it.enabled { t.text } else { t.muted };
                c = [c[0] + (col[0] as f32 - c[0]) * k, c[1] + (col[1] as f32 - c[1]) * k, c[2] + (col[2] as f32 - c[2]) * k];
            }
        }
        // Status dot before "Receiving as …".
        let dd = ((fx - dot.0).powi(2) + (fy - dot.1).powi(2)).sqrt() - dot.2;
        let k = cover(dd);
        if k > 0.0 {
            c = [c[0] + (t.ok[0] as f32 - c[0]) * k, c[1] + (t.ok[1] as f32 - c[1]) * k, c[2] + (t.ok[2] as f32 - c[2]) * k];
        }
        // A 1px border, drawn inside the edge.
        let edge = cover(d.abs() - 0.5) * (d > -1.5 * s.max(1.0)) as u8 as f32;
        c = [c[0] + (t.border[0] as f32 - c[0]) * edge, c[1] + (t.border[1] as f32 - c[1]) * edge, c[2] + (t.border[2] as f32 - c[2]) * edge];
        // Panel over shadow (straight alpha out).
        let oa = a + shadow * (1.0 - a);
        let k = a / oa.max(1e-6);
        *out = Rgba([(c[0] * k) as u8, (c[1] * k) as u8, (c[2] * k) as u8, (oa * 255.0).round() as u8]);
    }
    img
}

/// Windows: the menu window itself.
pub fn show(anchor: (i32, i32), title: String, status: String, items: Vec<Item>, owner: usize) {
    imp::show(anchor, title, status, items, owner);
}

#[cfg(not(windows))]
mod imp {
    pub fn show(_: (i32, i32), _: String, _: String, _: Vec<super::Item>, _: usize) {}
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::cell::RefCell;
    use std::time::Instant;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Registry::*;
    use windows::Win32::UI::HiDpi::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const WM_MOUSELEAVE: u32 = 0x02A3;
    const TIMER_FADE: usize = 1;
    const FADE_MS: f32 = 110.0;

    struct Menu {
        hwnd: HWND,
        owner: HWND,
        title: String,
        status: String,
        items: Vec<Item>,
        layout: Layout,
        theme: Theme,
        hover: Option<usize>,
        pos: (i32, i32),
        shown: Instant,
        tracking: bool,
    }

    thread_local! {
        static MENU: RefCell<Option<Menu>> = const { RefCell::new(None) };
        static CLASS: RefCell<bool> = const { RefCell::new(false) };
    }

    fn dark_mode() -> bool {
        let mut v: u32 = 1;
        let mut size = 4u32;
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                w!("AppsUseLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut v as *mut u32 as *mut _),
                Some(&mut size),
            )
        };
        r.is_ok() && v == 0
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    unsafe fn font(px: f32, weight: i32) -> HFONT {
        let make = |face: PCWSTR| {
            CreateFontW(-(px.round() as i32), 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (DEFAULT_PITCH.0 | FF_SWISS.0) as u32, face)
        };
        make(w!("Segoe UI Variable Text"))
    }

    unsafe fn text(dc: HDC, s: &str, f: HFONT, color: [u8; 3], rect: RECT, flags: DRAW_TEXT_FORMAT) {
        let old = SelectObject(dc, f);
        SetTextColor(dc, COLORREF(color[0] as u32 | (color[1] as u32) << 8 | (color[2] as u32) << 16));
        let mut r = rect;
        let mut w = wide(s);
        DrawTextW(dc, &mut w, &mut r, flags | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
        SelectObject(dc, old);
    }

    /// Draws the frame and puts it on screen with the given opacity and lift.
    unsafe fn present(m: &Menu, alpha: f32, lift: f32) {
        let l = &m.layout;
        let s = l.scale;
        let img = draw(l, &m.items, m.hover, &m.theme);
        let (w, h) = (l.w as i32, l.h as i32);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let Ok(bmp) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else { return };
        let px = crate::phone_art::premultiplied_bgra(&img);
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u8, px.len());
        let dc = CreateCompatibleDC(None);
        let old = SelectObject(dc, bmp);
        SetBkMode(dc, TRANSPARENT);

        // Header: the app icon, the name, the status.
        let (ix, iy, iz) = l.header_icon;
        let instance = GetModuleHandleW(None).unwrap_or_default();
        if let Ok(hi) = LoadImageW(instance, PCWSTR(1 as _), IMAGE_ICON, iz as i32, iz as i32, LR_DEFAULTCOLOR) {
            let _ = DrawIconEx(dc, ix as i32, iy as i32, HICON(hi.0), iz as i32, iz as i32, 0, None, DI_NORMAL);
            let _ = DestroyIcon(HICON(hi.0));
        }
        let (pxl, _, pw, _) = l.panel;
        let right = (pxl + pw - 16.0 * s) as i32;
        let title_f = font(15.0 * s, 600);
        let body_f = font(13.0 * s, 400);
        let small_f = font(12.5 * s, 400);
        text(dc, &m.title, title_f, m.theme.text, RECT { left: l.text_x as i32, top: l.title_y as i32, right, bottom: (l.title_y + 22.0 * s) as i32 }, DT_LEFT | DT_TOP);
        text(dc, &m.status, small_f, m.theme.muted, RECT { left: (l.text_x + 13.0 * s) as i32, top: l.status_y as i32, right, bottom: (l.status_y + 18.0 * s) as i32 }, DT_LEFT | DT_TOP);
        for (i, it) in m.items.iter().enumerate() {
            let (a0, a1) = l.rows[i];
            let rect = RECT { left: (pxl + 52.0 * s) as i32, top: a0 as i32, right, bottom: a1 as i32 };
            let col = if it.enabled { m.theme.text } else { m.theme.muted };
            text(dc, &it.label, body_f, col, rect, DT_LEFT | DT_VCENTER);
            if !it.detail.is_empty() {
                text(dc, &it.detail, small_f, m.theme.muted, rect, DT_RIGHT | DT_VCENTER);
            }
        }
        for f in [title_f, body_f, small_f] {
            let _ = DeleteObject(f);
        }
        // GDI leaves alpha at 0 where it drew: the panel's inside is opaque, so restore it there.
        let raw = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
        for (i, p) in img.pixels().enumerate() {
            if p[3] == 255 {
                raw[i * 4 + 3] = 255;
            }
        }
        let pos = POINT { x: m.pos.0, y: m.pos.1 + (lift * 8.0 * s) as i32 };
        let size = SIZE { cx: w, cy: h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: (alpha.clamp(0.0, 1.0) * 255.0) as u8, AlphaFormat: AC_SRC_ALPHA as u8 };
        let _ = UpdateLayeredWindow(m.hwnd, None, Some(&pos), Some(&size), dc, Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
        SelectObject(dc, old);
        let _ = DeleteDC(dc);
        let _ = DeleteObject(bmp);
    }

    unsafe fn close(hwnd: HWND) {
        let _ = KillTimer(hwnd, TIMER_FADE);
        let _ = ShowWindow(hwnd, SW_HIDE);
        MENU.with(|m| m.borrow_mut().take());
        let _ = DestroyWindow(hwnd);
    }

    unsafe fn pick(hwnd: HWND, i: usize) {
        let (owner, id) = MENU.with(|m| m.borrow().as_ref().map(|m| (m.owner, m.items[i].id))).unwrap_or_default();
        close(hwnd);
        let _ = PostMessageW(owner, WM_COMMAND, WPARAM(id), LPARAM(0));
    }

    fn redraw(hover: Option<usize>) {
        MENU.with(|m| {
            if let Some(m) = m.borrow_mut().as_mut() {
                if m.hover != hover {
                    m.hover = hover;
                    let t = (m.shown.elapsed().as_millis() as f32 / FADE_MS).min(1.0);
                    unsafe { present(m, t, 1.0 - ease(t)) };
                }
            }
        });
    }

    fn ease(t: f32) -> f32 {
        1.0 - (1.0 - t).powi(3)
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_TIMER if wp.0 == TIMER_FADE => {
                MENU.with(|m| {
                    if let Some(m) = m.borrow().as_ref() {
                        let t = (m.shown.elapsed().as_millis() as f32 / FADE_MS).min(1.0);
                        present(m, ease(t), 1.0 - ease(t));
                        if t >= 1.0 {
                            let _ = KillTimer(hwnd, TIMER_FADE);
                        }
                    }
                });
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let (x, y) = ((lp.0 & 0xffff) as i16 as f32, ((lp.0 >> 16) & 0xffff) as i16 as f32);
                let tracking = MENU.with(|m| m.borrow().as_ref().map(|m| m.tracking).unwrap_or(true));
                if !tracking {
                    let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                    MENU.with(|m| if let Some(m) = m.borrow_mut().as_mut() { m.tracking = true });
                }
                let h = MENU.with(|m| m.borrow().as_ref().and_then(|m| hit(&m.layout, &m.items, x, y)));
                redraw(h);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                MENU.with(|m| if let Some(m) = m.borrow_mut().as_mut() { m.tracking = false });
                redraw(None);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = ((lp.0 & 0xffff) as i16 as f32, ((lp.0 >> 16) & 0xffff) as i16 as f32);
                match MENU.with(|m| m.borrow().as_ref().map(|m| (hit(&m.layout, &m.items, x, y), m.layout.panel))) {
                    Some((Some(i), _)) => pick(hwnd, i),
                    // Outside the panel (in the shadow): dismiss, like clicking away.
                    Some((None, (px, py, pw, ph))) if x < px || x > px + pw || y < py || y > py + ph => close(hwnd),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                let key = wp.0 as u16;
                if key == VK_ESCAPE.0 {
                    close(hwnd);
                } else if key == VK_UP.0 || key == VK_DOWN.0 {
                    let next = MENU.with(|m| {
                        m.borrow().as_ref().and_then(|m| {
                            let n = m.items.len();
                            let enabled: Vec<usize> = (0..n).filter(|&i| m.items[i].enabled).collect();
                            let cur = m.hover.and_then(|h| enabled.iter().position(|&e| e == h));
                            let k = match (cur, key == VK_DOWN.0) {
                                (None, true) => 0,
                                (None, false) => enabled.len().saturating_sub(1),
                                (Some(c), true) => (c + 1) % enabled.len().max(1),
                                (Some(c), false) => (c + enabled.len() - 1) % enabled.len().max(1),
                            };
                            enabled.get(k).copied()
                        })
                    });
                    redraw(next);
                } else if key == VK_RETURN.0 || key == VK_SPACE.0 {
                    if let Some(i) = MENU.with(|m| m.borrow().as_ref().and_then(|m| m.hover)) {
                        pick(hwnd, i);
                    }
                }
                LRESULT(0)
            }
            // Clicking anywhere else, or switching apps, closes it.
            WM_ACTIVATE if (wp.0 & 0xffff) as u32 == WA_INACTIVE => {
                close(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    pub fn show(anchor: (i32, i32), title: String, status: String, items: Vec<Item>, owner: usize) {
        unsafe {
            if let Some(old) = MENU.with(|m| m.borrow().as_ref().map(|m| m.hwnd)) {
                close(old);
            }
            let instance = GetModuleHandleW(None).unwrap_or_default();
            let class = w!("DropCopyMenu");
            CLASS.with(|c| {
                if !*c.borrow() {
                    let wc = WNDCLASSEXW {
                        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                        lpfnWndProc: Some(wndproc),
                        hInstance: instance.into(),
                        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                        lpszClassName: class,
                        ..Default::default()
                    };
                    RegisterClassExW(&wc);
                    *c.borrow_mut() = true;
                }
            });
            // The monitor the click is on: its scale and its work area (without the taskbar).
            let mon = MonitorFromPoint(POINT { x: anchor.0, y: anchor.1 }, MONITOR_DEFAULTTONEAREST);
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let work = (mi.rcWork.left, mi.rcWork.top, mi.rcWork.right, mi.rcWork.bottom);
            let layout = layout(&items, dx.max(96) as f32 / 96.0);
            let pos = place(&layout, anchor, work);
            let Ok(hwnd) = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class,
                w!("DropCopy"),
                WS_POPUP,
                pos.0,
                pos.1,
                layout.w as i32,
                layout.h as i32,
                None,
                None,
                instance,
                None,
            ) else {
                return;
            };
            let theme = if dark_mode() { DARK } else { LIGHT };
            let m = Menu { hwnd, owner: HWND(owner as _), title, status, items, layout, theme, hover: None, pos, shown: Instant::now(), tracking: false };
            present(&m, 0.0, 1.0);
            MENU.with(|s| *s.borrow_mut() = Some(m));
            let _ = ShowWindow(hwnd, SW_SHOW);
            // Takes focus so a click elsewhere (deactivation) closes it, and the keys work.
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(hwnd);
            SetTimer(hwnd, TIMER_FADE, 10, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<Item> {
        let it = |id, icon, label: &str, sep| Item { id, icon, label: label.into(), detail: String::new(), enabled: true, separator: sep };
        vec![
            it(1, Icon::Folder, "Open received pictures", false),
            it(2, Icon::Qr, "Show pairing code", false),
            it(4, Icon::Update, "Check for updates", false),
            it(3, Icon::Power, "Quit", true),
        ]
    }

    #[test]
    fn rows_stack_without_overlap_and_hit_test() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let l = layout(&items(), scale);
            for w in l.rows.windows(2) {
                assert!(w[0].1 <= w[1].0);
            }
            assert_eq!(l.lines.len(), 2, "after the header, and before Quit");
            assert!(l.rows.last().unwrap().1 < l.panel.1 + l.panel.3);
            let mid = |i: usize| (l.rows[i].0 + l.rows[i].1) / 2.0;
            let x = l.panel.0 + 40.0;
            assert_eq!(hit(&l, &items(), x, mid(0)), Some(0));
            assert_eq!(hit(&l, &items(), x, mid(3)), Some(3));
            assert_eq!(hit(&l, &items(), x, l.header_icon.1), None, "the header is not a row");
            assert_eq!(hit(&l, &items(), 1.0, mid(0)), None, "the shadow is not a row");
        }
        let mut off = items();
        off[2].enabled = false;
        let l = layout(&off, 1.0);
        assert_eq!(hit(&l, &off, l.panel.0 + 40.0, (l.rows[2].0 + l.rows[2].1) / 2.0), None);
    }

    #[test]
    fn opens_above_a_bottom_taskbar_and_stays_on_screen() {
        let l = layout(&items(), 1.5);
        let m = (SHADOW * 1.5) as i32;
        let work = (0, 0, 2560, 1540); // taskbar below 1540
        // Clicked near the right end of the taskbar: above it, pulled in from the screen edge.
        let (x, y) = place(&l, (2500, 1560), work);
        assert!(y + m + l.panel.3 as i32 <= 1540);
        assert!(x + m + l.panel.2 as i32 <= 2560);
        // Taskbar at the top: opens below the click.
        let (_, y2) = place(&l, (1200, 20), (0, 48, 2560, 1600));
        assert!(y2 + m >= 48);
    }

    #[test]
    fn draws_an_opaque_panel_inside_a_soft_shadow() {
        let l = layout(&items(), 1.5);
        let img = draw(&l, &items(), Some(1), &DARK);
        assert_eq!((img.width(), img.height()), (l.w, l.h));
        assert_eq!(img.get_pixel(0, 0)[3], 0, "far corner is clear");
        let (px, py, pw, _) = l.panel;
        let inside = img.get_pixel((px + pw / 2.0) as u32, (py + 10.0) as u32);
        assert_eq!(inside[3], 255);
        // The hovered row is lighter than the panel.
        let (a0, a1) = l.rows[1];
        let row = img.get_pixel((px + pw - 30.0) as u32, ((a0 + a1) / 2.0) as u32);
        assert!(row[0] > DARK.panel[0] + 6, "{row:?}");
        // Under the panel's bottom edge the shadow is partly transparent.
        let below = img.get_pixel((px + pw / 2.0) as u32, (l.panel.1 + l.panel.3 + 6.0) as u32);
        assert!(below[3] > 0 && below[3] < 200, "{below:?}");
    }
}

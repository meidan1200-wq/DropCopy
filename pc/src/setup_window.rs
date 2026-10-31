//! The window a double-click opens instead of a terminal: blue, the logo faint behind, and in the
//! middle either the phone with the pairing code ("Scan me"), three pulsing dots while it waits
//! for the phone, or a check mark when the phone connects. Then it fades away.
//! See docs/setup-window-plan.md. Shapes are drawn here in plain Rust; Windows adds the text.

use image::{Rgba, RgbaImage};

pub const W: f32 = 480.0;
pub const H: f32 = 560.0;
const RADIUS: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stage {
    /// Not paired: the phone with the code.
    Scan,
    /// Paired: dots while waiting for the phone.
    Waiting,
    /// The phone connected; `t` 0..1 of the success animation.
    Connected,
}

const ARROW: &[(f32, f32)] = &[(50.0, 30.0), (58.0, 30.0), (58.0, 52.0), (67.0, 52.0), (54.0, 66.0), (41.0, 52.0), (50.0, 52.0)];
const TRAY: &[(f32, f32)] = &[(32.0, 62.0), (38.0, 62.0), (38.0, 70.0), (70.0, 70.0), (70.0, 62.0), (76.0, 62.0), (76.0, 76.0), (32.0, 76.0)];

fn inside(poly: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut c = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let ((xi, yi), (xj, yj)) = (poly[i], poly[j]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            c = !c;
        }
        j = i;
    }
    c
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

fn over(c: [f32; 3], with: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] + (with[0] - c[0]) * k, c[1] + (with[1] - c[1]) * k, c[2] + (with[2] - c[2]) * k]
}

/// A phone screen showing DropCopy: blue, with the logo large and faded across it. Put on a
/// phone with `phone_art::render` (the tray header, the reconnect stage).
pub fn logo_screen(w: u32, h: u32) -> RgbaImage {
    let (fw, fh) = (w as f32, h as f32);
    let size = fw * 1.25; // the logo's 108-unit canvas; its glyph fills most of the width
    let (ox, oy) = (fw / 2.0 - size / 2.0, fh / 2.0 - size / 2.0);
    RgbaImage::from_fn(w, h, |x, y| {
        let g = y as f32 / fh;
        let c = over([0x23 as f32, 0x7E as f32, 0xF0 as f32], [0x12 as f32, 0x5A as f32, 0xC8 as f32], g);
        let (u, v) = ((x as f32 + 0.5 - ox) / size * 108.0, (y as f32 + 0.5 - oy) / size * 108.0);
        let c = if inside(ARROW, u, v) || inside(TRAY, u, v) { over(c, [255.0; 3], 0.30) } else { c };
        Rgba([c[0] as u8, c[1] as u8, c[2] as u8, 255])
    })
}

/// The small phone with the logo screen, `height` pixels tall (body only).
pub fn logo_phone(height: f32) -> RgbaImage {
    let k = height / crate::phone_art::BODY_H;
    let screen = logo_screen(540, 1200);
    crate::phone_art::render(Some(&screen), k, 1, false).image
}

/// Where the text goes (for GDI), in output pixels: (y of the line, size in px).
pub fn text_lines(stage: Stage, s: f32, phone: Option<&str>) -> Vec<(f32, f32, String)> {
    let hint = |t: &str| (H * s - 70.0 * s, 15.0 * s, t.to_string());
    match stage {
        Stage::Scan => vec![(46.0 * s, 30.0 * s, "Scan me".into()), hint("Open DropCopy on your phone and point it here")],
        Stage::Waiting => vec![(H * s / 2.0 + 132.0 * s, 20.0 * s, "Looking for your phone".into()), hint("Open DropCopy on the phone, or share a photo to it")],
        Stage::Connected => vec![
            (H * s / 2.0 + 70.0 * s, 24.0 * s, connected_line(phone)),
            hint("DropCopy keeps running in the tray"),
        ],
    }
}

/// "Connected to Galaxy S20+", or a plain line for a phone that doesn't say its name (older app).
pub fn connected_line(phone: Option<&str>) -> String {
    match phone {
        Some(n) if !n.is_empty() => format!("Connected to {n}"),
        _ => "Phone connected".into(),
    }
}

/// One frame. `time` is seconds since the window opened (drives the dots); `code` is the phone with
/// the QR code (from `phone_art::render_code`) for the Scan stage; `done` 0..1 for Connected.
pub fn draw(stage: Stage, time: f32, done: f32, code: Option<&RgbaImage>, s: f32) -> RgbaImage {
    let (w, h) = ((W * s).round() as u32, (H * s).round() as u32);
    let (fw, fh) = (w as f32, h as f32);
    let mut img = RgbaImage::new(w, h);
    let top = [0x23 as f32, 0x7E as f32, 0xF0 as f32];
    let bottom = [0x12 as f32, 0x5A as f32, 0xC8 as f32];
    let white = [255.0f32; 3];
    // The logo, large and faint, in the lower right.
    let logo = (fw * 0.42, fh * 0.42, fw * 0.75); // x, y, size (of the 108-unit canvas)
    for (x, y, p) in img.enumerate_pixels_mut() {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let a = cover(rbox(fx, fy, 0.0, 0.0, fw, fh, RADIUS * s));
        if a <= 0.0 {
            continue;
        }
        let g = fy / fh;
        let mut c = over(top, bottom, g);
        let (u, v) = ((fx - logo.0) / logo.2 * 108.0, (fy - logo.1) / logo.2 * 108.0);
        if inside(ARROW, u, v) || inside(TRAY, u, v) {
            c = over(c, white, 0.07);
        }
        match stage {
            Stage::Waiting => {
                for i in 0..3 {
                    let ph = ((time * 1.6 - i as f32 * 0.22) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
                    let r = (7.0 + 3.5 * ph) * s;
                    let cx = fw / 2.0 + (i as f32 - 1.0) * 34.0 * s;
                    // Under the phone when there is one (reconnecting), else in the middle.
                    let dy = if code.is_some() { fh / 2.0 + 104.0 * s } else { fh / 2.0 };
                    let d = ((fx - cx).powi(2) + (fy - dy).powi(2)).sqrt() - r;
                    c = over(c, white, cover(d) * (0.45 + 0.55 * ph));
                }
            }
            Stage::Connected => {
                // A white ring that draws itself, then a check mark.
                let (cx, cy, r) = (fw / 2.0, fh / 2.0 - 20.0 * s, 54.0 * s);
                let ring = (((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() - r).abs() - 3.5 * s;
                let ang = ((fy - cy).atan2(fx - cx) + std::f32::consts::FRAC_PI_2).rem_euclid(std::f32::consts::TAU);
                let drawn = (done / 0.6).min(1.0) * std::f32::consts::TAU;
                if ang <= drawn {
                    c = over(c, white, cover(ring));
                }
                let k = ((done - 0.5) / 0.5).clamp(0.0, 1.0);
                if k > 0.0 {
                    let a1 = (cx - 22.0 * s, cy + 2.0 * s);
                    let a2 = (cx - 6.0 * s, cy + 18.0 * s);
                    let a3 = (cx + 24.0 * s, cy - 16.0 * s);
                    let d = seg(fx, fy, a1, a2).min(seg(fx, fy, a2, a3)) - 5.0 * s;
                    c = over(c, white, cover(d) * k);
                }
            }
            Stage::Scan => {}
        }
        *p = Rgba([c[0] as u8, c[1] as u8, c[2] as u8, (a * 255.0) as u8]);
    }
    if let (Stage::Scan | Stage::Waiting, Some(ph)) = (stage, code) {
        // Scan: the phone with the code, a little below the title. Waiting (reconnecting): the
        // phone with the logo screen, above the dots.
        let lift = if stage == Stage::Scan { 14.0 } else { -40.0 };
        let ox = (w as i64 - ph.width() as i64) / 2;
        let oy = ((h as i64 - ph.height() as i64) / 2 + (lift * s) as i64).max(0);
        image::imageops::overlay(&mut img, ph, ox, oy);
    }
    img
}

/// Opens the window and runs until it is closed or fades after the phone connects.
/// `connected` is polled to move from Scan/Waiting to Connected.
/// `connected` is polled: `Some(name)` once a phone got through (`name` may be empty).
pub fn run(link: String, paired: bool, connected: impl Fn() -> Option<String>) {
    imp::run(link, paired, &connected);
}

#[cfg(not(windows))]
mod imp {
    pub fn run(_: String, _: bool, _: &dyn Fn() -> Option<String>) {}
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::time::Instant;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::HiDpi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            // Drag it anywhere; Esc or a click on the close spot (top right) closes.
            WM_NCHITTEST => {
                let r = DefWindowProcW(hwnd, msg, wp, lp);
                if r.0 == HTCLIENT as isize { LRESULT(HTCAPTION as isize) } else { r }
            }
            WM_KEYDOWN if wp.0 as u16 == 0x1B => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_NCRBUTTONUP | WM_RBUTTONUP => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    unsafe fn present(hwnd: HWND, img: &RgbaImage, stage: Stage, phone: Option<&str>, s: f32, alpha: f32, pos: (i32, i32)) {
        let (w, h) = (img.width() as i32, img.height() as i32);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let Ok(bmp) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else { return };
        let px = crate::phone_art::premultiplied_bgra(img);
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u8, px.len());
        let dc = CreateCompatibleDC(None);
        let old = SelectObject(dc, bmp);
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00FFFFFF));
        for (y, size, line) in text_lines(stage, s, phone) {
            let weight = if size > 18.0 * s { 600 } else { 400 };
            let f = CreateFontW(-(size as i32), 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, ANTIALIASED_QUALITY.0 as u32, 0, w!("Segoe UI Variable Display"));
            let o = SelectObject(dc, f);
            let mut r = RECT { left: 0, top: y as i32, right: w, bottom: (y + size * 1.6) as i32 };
            let mut t: Vec<u16> = line.encode_utf16().collect();
            DrawTextW(dc, &mut t, &mut r, DT_CENTER | DT_SINGLELINE | DT_NOPREFIX);
            SelectObject(dc, o);
            let _ = DeleteObject(f);
        }
        // GDI cleared alpha where it drew text: the window is opaque inside, restore it.
        let raw = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
        for (i, p) in img.pixels().enumerate() {
            if p[3] == 255 {
                raw[i * 4 + 3] = 255;
            }
        }
        let size = SIZE { cx: w, cy: h };
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: (alpha.clamp(0.0, 1.0) * 255.0) as u8, AlphaFormat: AC_SRC_ALPHA as u8 };
        let _ = UpdateLayeredWindow(hwnd, None, Some(&POINT { x: pos.0, y: pos.1 }), Some(&size), dc, Some(&POINT::default()), COLORREF(0), Some(&blend), ULW_ALPHA);
        SelectObject(dc, old);
        let _ = DeleteDC(dc);
        let _ = DeleteObject(bmp);
    }

    pub fn run(link: String, paired: bool, connected: &dyn Fn() -> Option<String>) {
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let s = GetDpiForSystem().max(96) as f32 / 96.0;
            let code = if paired {
                Some(logo_phone(190.0 * s)) // reconnecting: the phone showing DropCopy
            } else {
                (|| {
                    let qr = qrcode::QrCode::with_error_correction_level(link.as_bytes(), qrcode::EcLevel::L).ok()?;
                    let m: Vec<bool> = qr.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
                    Some(crate::phone_art::render_code(&m, qr.width(), s, false).image)
                })()
            };
            let instance = GetModuleHandleW(None).unwrap_or_default();
            let class = w!("DropCopySetup");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hIcon: LoadIconW(instance, PCWSTR(1 as _)).unwrap_or_default(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let (w, h) = ((W * s).round() as i32, (H * s).round() as i32);
            let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            let pos = ((sw - w) / 2, (sh - h) / 2);
            // A taskbar button (not a tool window), so it can be found and closed like an app.
            let Ok(hwnd) = CreateWindowExW(WS_EX_LAYERED | WS_EX_APPWINDOW, class, w!("DropCopy"), WS_POPUP | WS_SYSMENU, pos.0, pos.1, w, h, None, None, instance, None) else { return };
            let mut phone: Option<String> = None;
            let mut stage = if paired { Stage::Waiting } else { Stage::Scan };
            let t0 = Instant::now();
            let mut done_at: Option<Instant> = None;
            let mut last_poll = Instant::now();
            present(hwnd, &draw(stage, 0.0, 0.0, code.as_ref(), s), stage, None, s, 0.0, pos);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let mut msg = MSG::default();
            loop {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        return;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                if done_at.is_none() && last_poll.elapsed().as_millis() > 400 {
                    last_poll = Instant::now();
                    if let Some(n) = connected() {
                        phone = Some(n);
                        stage = Stage::Connected;
                        done_at = Some(Instant::now());
                    }
                }
                let time = t0.elapsed().as_secs_f32();
                let fade_in = (time / 0.25).min(1.0);
                let (done, alpha) = match done_at {
                    Some(t) => {
                        let e = t.elapsed().as_secs_f32();
                        (e / 0.9, fade_in * (1.0 - ((e - 3.0) / 0.4).clamp(0.0, 1.0)))
                    }
                    None => (0.0, fade_in),
                };
                if done_at.is_some_and(|t| t.elapsed().as_secs_f32() > 3.4) {
                    let _ = DestroyWindow(hwnd);
                    continue;
                }
                // The phone with the code is still: redraw it only while fading in.
                let animating = stage != Stage::Scan || fade_in < 1.0;
                if animating {
                    present(hwnd, &draw(stage, time, done.min(1.0), code.as_ref(), s), stage, phone.as_deref(), s, alpha, pos);
                }
                std::thread::sleep(std::time::Duration::from_millis(if animating { 16 } else { 100 }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_phone_when_it_says() {
        assert_eq!(connected_line(Some("Galaxy S20+")), "Connected to Galaxy S20+");
        assert_eq!(connected_line(Some("")), "Phone connected");
        assert_eq!(connected_line(None), "Phone connected");
    }

    #[test]
    fn each_stage_draws_a_rounded_blue_window() {
        for st in [Stage::Scan, Stage::Waiting, Stage::Connected] {
            let img = draw(st, 0.3, 1.0, None, 1.5);
            assert_eq!(img.width(), (W * 1.5) as u32);
            assert_eq!(img.get_pixel(0, 0)[3], 0, "rounded corner");
            let mid = img.get_pixel(10, img.height() / 2);
            assert!(mid[2] > 180 && mid[0] < 80, "blue: {mid:?}");
        }
        // Reconnecting: the logo phone is drawn above the dots.
        let ph = logo_phone(190.0);
        let r = draw(Stage::Waiting, 0.0, 0.0, Some(&ph), 1.0);
        let mid = r.get_pixel((W / 2.0) as u32, (H / 2.0 - 40.0) as u32);
        assert!(mid[2] > 150 && mid[3] == 255, "phone screen is blue: {mid:?}");
        // Waiting: the dots are white in the middle; Connected: the check is drawn.
        let w = draw(Stage::Waiting, 0.0, 0.0, None, 1.0);
        let c = w.get_pixel((W / 2.0) as u32, (H / 2.0) as u32);
        assert!(c[0] > 100, "{c:?}");
        let d = draw(Stage::Connected, 0.0, 1.0, None, 1.0);
        let ring = d.get_pixel((W / 2.0) as u32, (H / 2.0 - 20.0 - 54.0) as u32);
        assert!(ring[0] > 200, "ring {ring:?}");
    }
}

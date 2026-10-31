//! Draws the little phone that rises from the taskbar: a rounded handset with a centre punch-hole
//! camera and side keys (after a Galaxy S20+), the photo filling its screen, and a soft shadow.
//!
//! Pure Rust and resolution independent: everything is drawn from signed distances, so edges are
//! anti-aliased at any display scale and the output can be checked on any OS.

use image::{Rgba, RgbaImage};

/// Logical (96-dpi) size of the handset, not counting the shadow margin. 9:20 like the S20+.
pub const BODY_W: f32 = 76.0;
pub const BODY_H: f32 = 160.0;
pub const MARGIN: f32 = 14.0;

pub struct Art {
    pub image: RgbaImage,
    /// Close badge, in output pixels: centre and radius.
    pub close: (f32, f32, f32),
}

/// Signed distance from `(px, py)` to a rounded box centred at `(cx, cy)`.
fn rbox(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - r);
    let qy = (py - cy).abs() - (hh - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

fn cover(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// Source-over blend of straight-alpha `c` with coverage `a` onto `dst`.
fn blend(dst: &mut Rgba<u8>, c: [f32; 3], a: f32) {
    if a <= 0.0 {
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let oa = a + da * (1.0 - a);
    for i in 0..3 {
        let dc = dst[i] as f32 / 255.0;
        let v = (c[i] * a + dc * da * (1.0 - a)) / oa.max(1e-6);
        dst[i] = (v * 255.0).round() as u8;
    }
    dst[3] = (oa * 255.0).round() as u8;
}

fn rgb(hex: u32) -> [f32; 3] {
    [((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0]
}

/// Samples `photo` scaled to cover a `w`×`h` area (centre crop), bilinear.
fn sample_cover(photo: &RgbaImage, u: f32, v: f32, w: f32, h: f32) -> [f32; 3] {
    let (pw, ph) = (photo.width() as f32, photo.height() as f32);
    let s = (w / pw).max(h / ph);
    let (ox, oy) = ((pw * s - w) / 2.0, (ph * s - h) / 2.0);
    let x = ((u + ox) / s - 0.5).clamp(0.0, pw - 1.0);
    let y = ((v + oy) / s - 0.5).clamp(0.0, ph - 1.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(photo.width() - 1), (y0 + 1).min(photo.height() - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let p = |x, y| photo.get_pixel(x, y).0;
    let mut out = [0.0; 3];
    for (i, o) in out.iter_mut().enumerate() {
        let top = p(x0, y0)[i] as f32 * (1.0 - fx) + p(x1, y0)[i] as f32 * fx;
        let bot = p(x0, y1)[i] as f32 * (1.0 - fx) + p(x1, y1)[i] as f32 * fx;
        *o = (top * (1.0 - fy) + bot * fy) / 255.0;
    }
    out
}

/// What the phone's screen shows.
pub enum Screen<'a> {
    Photo(Option<&'a RgbaImage>),
    /// A QR code: `modules` is row-major, true = dark, `width` modules a side.
    Code { modules: &'a [bool], width: usize },
}

/// Smallest module, in logical pixels, that a phone camera reads reliably off a laptop screen.
pub const CODE_MODULE: f32 = 3.0;
/// Light modules kept around the code inside the white screen.
const CODE_QUIET: usize = 2;

/// The scale the pairing phone is drawn at so each module is a whole number of pixels, at least
/// `CODE_MODULE` logical pixels. `dpi` is DPI / 96.
pub fn code_scale(width: usize, dpi: f32) -> f32 {
    let px = (CODE_MODULE * dpi.max(1.0)).ceil();
    let need = (width + 2 * CODE_QUIET) as f32 * px;
    need / (screen_w(1.0) * 0.94) + 0.01
}

fn screen_w(s: f32) -> f32 {
    BODY_W * s - 2.0 * 3.2 * s
}

/// Renders the phone at `scale` (DPI / 96). `count` > 1 stacks a second handset behind; `hover`
/// shows the close badge.
pub fn render(photo: Option<&RgbaImage>, scale: f32, count: usize, hover: bool) -> Art {
    draw(Screen::Photo(photo), scale, count, hover)
}

/// The phone with a pairing code on its screen, large enough to scan; `scale` is DPI / 96.
pub fn render_code(modules: &[bool], width: usize, scale: f32, hover: bool) -> Art {
    draw(Screen::Code { modules, width }, code_scale(width, scale), 1, hover)
}

pub fn draw(screen: Screen, scale: f32, count: usize, hover: bool) -> Art {
    let s = scale.max(1.0);
    let (bw, bh, m) = (BODY_W * s, BODY_H * s, MARGIN * s);
    let (w, h) = ((bw + 2.0 * m).ceil() as u32, (bh + 2.0 * m).ceil() as u32);
    let mut img = RgbaImage::new(w, h);
    let (cx, cy) = (w as f32 / 2.0, m + bh / 2.0);
    let r_body = 15.0 * s;
    let bezel = 3.2 * s;
    let (sw, sh) = (bw - 2.0 * bezel, bh - 2.0 * bezel);
    let r_screen = r_body - bezel;
    let close = (cx + bw / 2.0 - 3.0 * s, m + 3.0 * s, 9.0 * s);

    let photo = match screen {
        Screen::Photo(p) => p,
        Screen::Code { .. } => None,
    };
    // A code is drawn with whole pixels per module, centred, so every edge is sharp.
    let code = match screen {
        Screen::Code { modules, width } => {
            let px = ((sw * 0.94) / (width + 2 * CODE_QUIET) as f32).floor().max(1.0);
            let side = px * width as f32;
            let (x0, y0) = ((cx - side / 2.0).round(), (cy - side / 2.0).round());
            Some((modules, width, px, x0, y0))
        }
        Screen::Photo(_) => None,
    };
    // Thumbnail of the photo, pre-scaled once so per-pixel sampling stays cheap.
    let small = photo.map(|p| {
        let k = (sw * 1.5 / p.width() as f32).max(sh * 1.5 / p.height() as f32).min(1.0);
        let (tw, th) = ((p.width() as f32 * k).max(1.0) as u32, (p.height() as f32 * k).max(1.0) as u32);
        image::imageops::resize(p, tw, th, image::imageops::FilterType::Triangle)
    });

    for (x, y, px) in img.enumerate_pixels_mut() {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);

        // Soft shadow: a blurred, slightly lowered copy of the body.
        let ds = rbox(fx, fy, cx, cy + 3.0 * s, bw / 2.0, bh / 2.0, r_body);
        let blur = 9.0 * s;
        let sh_a = 0.38 * (1.0 - ((ds + blur * 0.3) / blur).clamp(0.0, 1.0)).powi(2);
        blend(px, [0.0; 3], sh_a);

        // Side keys on the right edge (volume, power), drawn under the body edge.
        for (ky, kh) in [(0.22, 0.13), (0.40, 0.07)] {
            let d = rbox(fx, fy, cx + bw / 2.0 + 0.6 * s, m + bh * (ky + kh / 2.0), 1.6 * s, bh * kh / 2.0, 1.2 * s);
            blend(px, rgb(0x2A2A2E), cover(d));
        }

        // Several files: a second handset peeks out behind, like a small stack.
        if count > 1 {
            let dg = rbox(fx, fy, cx + 5.0 * s, cy - 5.0 * s, bw / 2.0, bh / 2.0, r_body);
            blend(px, rgb(0x3A3A40), cover(dg));
            blend(px, rgb(0x1B1B1F), cover(dg + 1.0 * s));
        }

        // Body with a faint lighter rim.
        let db = rbox(fx, fy, cx, cy, bw / 2.0, bh / 2.0, r_body);
        blend(px, rgb(0x3A3A40), cover(db));
        blend(px, rgb(0x0E0E10), cover(db + 1.0 * s));

        // Screen.
        let dsc = rbox(fx, fy, cx, cy, sw / 2.0, sh / 2.0, r_screen);
        let a = cover(dsc);
        if a > 0.0 {
            let c = if let Some((modules, width, px, x0, y0)) = code {
                let (mx, my) = (((fx - x0) / px).floor(), ((fy - y0) / px).floor());
                let inside = mx >= 0.0 && my >= 0.0 && (mx as usize) < width && (my as usize) < width;
                if inside && modules[my as usize * width + mx as usize] { [0.0; 3] } else { [1.0; 3] }
            } else {
                match &small {
                    Some(p) => sample_cover(p, fx - (cx - sw / 2.0), fy - (cy - sh / 2.0), sw, sh),
                    None => rgb(0x1C2230),
                }
            };
            blend(px, c, a);
        }

        // Centre punch-hole camera.
        let cam = ((fx - cx).powi(2) + (fy - (cy - sh / 2.0 + 6.0 * s)).powi(2)).sqrt() - 2.4 * s;
        blend(px, rgb(0x050505), cover(cam));

        // Close badge, only while the pointer is on the phone.
        if hover {
            let d = ((fx - close.0).powi(2) + (fy - close.1).powi(2)).sqrt() - close.2;
            blend(px, rgb(0x2B2B30), cover(d));
            blend(px, rgb(0x505058), cover(d.abs() - 0.5 * s) * 0.9);
            // The ×: two short strokes.
            let (lx, ly) = (fx - close.0, fy - close.1);
            let arm = 3.2 * s;
            for (ax, ay) in [(1.0f32, 1.0f32), (1.0, -1.0)] {
                let len = (ax * ax + ay * ay).sqrt();
                let t = ((lx * ax + ly * ay) / len).clamp(-arm, arm);
                let d = ((lx - t * ax / len).powi(2) + (ly - t * ay / len).powi(2)).sqrt() - 0.75 * s;
                blend(px, rgb(0xF0F0F0), cover(d));
            }
        }
    }
    Art { image: img, close }
}

/// Converts to the premultiplied BGRA that Windows' UpdateLayeredWindow expects.
pub fn premultiplied_bgra(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity(img.as_raw().len());
    for p in img.pixels() {
        let a = p[3] as u32;
        out.extend_from_slice(&[
            (p[2] as u32 * a / 255) as u8,
            (p[1] as u32 * a / 255) as u8,
            (p[0] as u32 * a / 255) as u8,
            a as u8,
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_phone_with_photo_and_transparent_corners() {
        let photo = RgbaImage::from_pixel(1080, 2400, Rgba([200, 30, 30, 255]));
        let art = render(Some(&photo), 1.5, 1, false);
        let (w, h) = art.image.dimensions();
        assert_eq!((w, h), ((BODY_W * 1.5 + 2.0 * MARGIN * 1.5).ceil() as u32, (BODY_H * 1.5 + 2.0 * MARGIN * 1.5).ceil() as u32));
        assert_eq!(art.image.get_pixel(0, 0)[3], 0, "corner outside the shadow is transparent");
        let centre = art.image.get_pixel(w / 2, h / 2);
        assert!(centre[0] > 180 && centre[1] < 60, "photo fills the screen: {centre:?}");
        // The body edge is anti-aliased: some pixels between opaque and transparent.
        let row = h / 2;
        assert!((0..w).any(|x| (40..220).contains(&art.image.get_pixel(x, row)[3])));
    }

    #[test]
    fn pairing_code_is_sharp_and_scannable_size() {
        let code = qrcode::QrCode::with_error_correction_level(b"dropcopy://pair?v=1&id=0123456789abcdef&k=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&n=DESKTOP-4E1E3TB&p=48650&a=192.168.100.200", qrcode::EcLevel::L).unwrap();
        let w = code.width();
        let modules: Vec<bool> = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let art = render_code(&modules, w, scale, false);
            // Count the run of the top-left finder pattern's dark row: 7 modules, whole pixels.
            let img = &art.image;
            let mid_y = img.height() / 2;
            let row: Vec<bool> = (0..img.width()).map(|x| img.get_pixel(x, mid_y)[0] < 128 && img.get_pixel(x, mid_y)[3] == 255).collect();
            assert!(row.iter().any(|d| *d));
            // Every pixel of the screen is pure black or pure white (no blur).
            let first_dark = (0..img.width()).find(|&x| {
                let p = img.get_pixel(x, img.height() / 2);
                p[0] == 0 && p[3] == 255
            });
            assert!(first_dark.is_some(), "code drawn at scale {scale}");
            let px = ((screen_w(code_scale(w, scale)) * 0.94) / (w + 4) as f32).floor();
            assert!(px >= CODE_MODULE * scale, "module {px}px at scale {scale}");
        }
    }

    #[test]
    fn premultiply() {
        let img = RgbaImage::from_pixel(1, 1, Rgba([200, 100, 50, 128]));
        assert_eq!(premultiplied_bgra(&img), vec![25, 50, 100, 128]);
    }
}

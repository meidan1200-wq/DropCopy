//! Draws the app icon (the phone app's adaptive icon: a white arrow dropping into a tray on blue)
//! and writes `assets/dropcopy.ico` with every size Windows asks for, plus PNG previews.
//! `cargo run --example make_icon [OUTDIR]`

use image::{Rgba, RgbaImage};

/// The phone icon's 108-unit canvas, cropped to its glyph (as in `assets/dropcopy.svg`).
const VIEW: (f32, f32, f32) = (20.0, 19.0, 68.0);
const BLUE: [u8; 3] = [0x1A, 0x73, 0xE8];
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

fn in_rounded(x: f32, y: f32) -> bool {
    let (x0, y0, s) = VIEW;
    let r = 15.0;
    let qx = ((x - (x0 + s / 2.0)).abs() - (s / 2.0 - r)).max(0.0);
    let qy = ((y - (y0 + s / 2.0)).abs() - (s / 2.0 - r)).max(0.0);
    qx * qx + qy * qy <= r * r
}

/// Renders at `n`×`n` with 8×8 supersampling, so edges are smooth at 16 px.
fn render(n: u32) -> RgbaImage {
    let (x0, y0, s) = VIEW;
    let ss = 8;
    RgbaImage::from_fn(n, n, |px, py| {
        let (mut bg, mut fg) = (0u32, 0u32);
        for sy in 0..ss {
            for sx in 0..ss {
                let x = x0 + (px as f32 + (sx as f32 + 0.5) / ss as f32) * s / n as f32;
                let y = y0 + (py as f32 + (sy as f32 + 0.5) / ss as f32) * s / n as f32;
                if in_rounded(x, y) {
                    bg += 1;
                    if inside(ARROW, x, y) || inside(TRAY, x, y) {
                        fg += 1;
                    }
                }
            }
        }
        let total = (ss * ss) as f32;
        if bg == 0 {
            return Rgba([0, 0, 0, 0]);
        }
        let f = fg as f32 / bg as f32;
        let mix = |b: u8| (b as f32 * (1.0 - f) + 255.0 * f).round() as u8;
        Rgba([mix(BLUE[0]), mix(BLUE[1]), mix(BLUE[2]), (255.0 * bg as f32 / total).round() as u8])
    })
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "assets".into()));
    std::fs::create_dir_all(&out).unwrap();
    let sizes = [16u32, 20, 24, 32, 40, 48, 64, 256];
    let pngs: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&n| {
            let img = render(n);
            let mut v = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png).unwrap();
            if n == 32 || n == 256 {
                img.save(out.join(format!("icon-{n}.png"))).unwrap();
            }
            v
        })
        .collect();
    // ICO: header, one directory entry per size, then the PNG images (Vista+ reads PNG entries).
    let mut ico = vec![0, 0, 1, 0];
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (n, png) in sizes.iter().zip(&pngs) {
        let b = if *n >= 256 { 0 } else { *n as u8 };
        ico.extend_from_slice(&[b, b, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for png in &pngs {
        ico.extend_from_slice(png);
    }
    std::fs::write(out.join("dropcopy.ico"), &ico).unwrap();
    println!("wrote {} ({} bytes)", out.join("dropcopy.ico").display(), ico.len());
}

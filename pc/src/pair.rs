//! The pairing link and its QR code. Scanning it once is the whole setup on the phone.

use crate::config::Config;
use crate::util::{b64url, hex, urlenc};
use qrcode::render::unicode::Dense1x2;
use qrcode::{EcLevel, QrCode};
use std::net::{Ipv4Addr, UdpSocket};
use std::path::{Path, PathBuf};

/// The address other machines on the LAN reach us at. Sends nothing: connecting a UDP
/// socket only asks the OS which interface it would route through.
pub fn primary_ipv4() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    match s.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

pub fn link(cfg: &Config, port: u16) -> String {
    let addr = primary_ipv4().map(|ip| format!("&a={ip}")).unwrap_or_default();
    format!(
        "dropcopy://pair?v=1&id={}&k={}&n={}&p={port}{addr}",
        hex(&cfg.id),
        b64url(&cfg.key),
        urlenc(&cfg.name)
    )
}

/// Light modules drawn as blocks, which reads correctly on the usual dark terminal.
/// `invert` is for terminals with a light background.
pub fn terminal_qr(link: &str, invert: bool) -> String {
    let code = QrCode::with_error_correction_level(link, EcLevel::L).expect("pairing link fits in a QR code");
    let (dark, light) = if invert { (Dense1x2::Dark, Dense1x2::Light) } else { (Dense1x2::Light, Dense1x2::Dark) };
    code.render::<Dense1x2>().dark_color(dark).light_color(light).quiet_zone(true).build()
}

/// Writes the QR code as a PNG (for terminals that render the blocks badly) and returns its path.
pub fn png_qr(link: &str, dir: &Path) -> Result<PathBuf, String> {
    let code = QrCode::with_error_correction_level(link, EcLevel::L).map_err(|e| e.to_string())?;
    let w = code.width();
    let scale = 10u32;
    let quiet = 4u32;
    let size = (w as u32 + 2 * quiet) * scale;
    let colors = code.to_colors();
    let img = image::GrayImage::from_fn(size, size, |x, y| {
        let (mx, my) = ((x / scale) as i64 - quiet as i64, (y / scale) as i64 - quiet as i64);
        let dark = mx >= 0
            && my >= 0
            && (mx as usize) < w
            && (my as usize) < w
            && colors[my as usize * w + mx as usize] == qrcode::Color::Dark;
        image::Luma([if dark { 0 } else { 255 }])
    });
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join("pair.png");
    img.save(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn print_pairing(cfg: &Config, port: u16, invert: bool) {
    let l = link(cfg, port);
    crate::say!("\n  Pair your phone: open DropCopy on the phone and tap \"Scan pairing code\".\n");
    crate::say!("{}", terminal_qr(&l, invert));
    crate::say!("  QR unreadable? Run `dropcopy pair --png`, or paste this link into the app:\n  {l}\n");
}

//! Renders the taskbar phone to PNGs for a visual check: `cargo run --example phone_preview IMAGE OUTDIR`
fn main() {
    let mut a = std::env::args().skip(1);
    let photo = image::open(a.next().expect("IMAGE")).unwrap().to_rgba8();
    let out = std::path::PathBuf::from(a.next().expect("OUTDIR"));
    for (name, scale, count, hover) in [("1x", 1.0, 1, false), ("150", 1.5, 1, false), ("150-hover-3", 1.5, 3, true), ("200", 2.0, 1, false)] {
        let t = std::time::Instant::now();
        let art = dropcopy::phone_art::render(Some(&photo), scale, count, hover);
        println!("{name}: {}x{} in {:?}", art.image.width(), art.image.height(), t.elapsed());
        art.image.save(out.join(format!("phone-{name}.png"))).unwrap();
    }
    // The pairing phone, with a link the length of a real one.
    let link = "dropcopy://pair?v=1&id=0123456789abcdef&k=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&n=DESKTOP-4E1E3TB&p=48650&a=192.168.100.200";
    let qr = qrcode::QrCode::with_error_correction_level(link, qrcode::EcLevel::L).unwrap();
    let modules: Vec<bool> = qr.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
    for scale in [1.0, 1.5] {
        let art = dropcopy::phone_art::render_code(&modules, qr.width(), scale, scale > 1.0);
        println!("code {scale}: {}x{} ({} modules)", art.image.width(), art.image.height(), qr.width());
        art.image.save(out.join(format!("code-{scale}.png"))).unwrap();
    }
}

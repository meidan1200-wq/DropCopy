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
}

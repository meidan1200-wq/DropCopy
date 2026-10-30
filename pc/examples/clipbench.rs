//! Times the clipboard image preparation: `cargo run --release --example clipbench photo.jpg`
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("usage: clipbench IMAGE");
    let raw = std::fs::read(&path).unwrap();
    let t = Instant::now();
    let (img, rotated) = dropcopy::clipboard::decode(&raw).expect("decodable image");
    let t_decode = t.elapsed();
    let t = Instant::now();
    let dib = dropcopy::clipboard::dib(&img);
    println!(
        "{}x{} rotated={rotated}: decode {:?}, dib {:?} ({} MB)",
        img.width(),
        img.height(),
        t_decode,
        t.elapsed(),
        dib.len() >> 20
    );
}

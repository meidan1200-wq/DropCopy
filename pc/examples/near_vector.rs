//! Prints a proximity-token test vector shared with the Android tests.
fn main() {
    let t = dropcopy::near::token(&[7u8; 32], 29_000_000);
    println!("{}", dropcopy::util::hex(&t));
}

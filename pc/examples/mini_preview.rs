fn main() {
    use dropcopy::setup_window::*;
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    draw_mini(Stage::Waiting, 0.15, 0.0, 1.5).save(out.join("mini-wait.png")).unwrap();
    draw_mini(Stage::Connected, 0.0, 1.0, 1.5).save(out.join("mini-done.png")).unwrap();
    logo_phone(40.0 * 1.5).save(out.join("menu-phone.png")).unwrap();
}

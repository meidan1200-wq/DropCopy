//! DropCopy: share on the phone, Ctrl+V on the PC.
//!
//! Everything stays on the local network. See `../PROTOCOL.md` for the wire format.

#[macro_export]
macro_rules! say {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        // A hidden (autostarted) instance has no console; never panic on a dead stdout.
        let _ = writeln!(std::io::stdout(), $($t)*);
        if let Some(log) = $crate::LOG.get() {
            if let Ok(mut f) = log.lock() {
                let _ = writeln!(f, $($t)*);
            }
        }
    }};
}

/// Set by a background (autostarted) instance, which has no console: everything `say!` prints
/// also goes to `log.txt` in the settings folder.
pub static LOG: std::sync::OnceLock<std::sync::Mutex<std::fs::File>> = std::sync::OnceLock::new();

/// Starts logging to `dir/log.txt`, keeping only the last run.
pub fn log_to(dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(f) = std::fs::File::create(dir.join("log.txt")) {
        let _ = LOG.set(std::sync::Mutex::new(f));
    }
}

pub mod client;
pub mod clipboard;
pub mod config;
pub mod near;
pub mod phone_art;
pub mod popup;
pub mod pair;
pub mod proto;
pub mod server;
pub mod update;
pub mod util;

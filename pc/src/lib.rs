//! DropCopy: share on the phone, Ctrl+V on the PC.
//!
//! Everything stays on the local network. See `../PROTOCOL.md` for the wire format.

#[macro_export]
macro_rules! say {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        // A hidden (autostarted) instance has no console; never panic on a dead stdout.
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

pub mod client;
pub mod clipboard;
pub mod config;
pub mod pair;
pub mod proto;
pub mod server;
pub mod util;

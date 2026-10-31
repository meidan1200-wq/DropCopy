//! Setting up the PC from the phone: the phone serves DropCopy-Setup.exe over the local network
//! with its pairing link appended to the file (a Windows program still runs with data after it).
//! On first start the PC reads that link from its own file and takes its id and key, so the phone,
//! which already holds them, connects with no code to scan.
//!
//! Tail of the file: link (utf-8) | u32 LE length of the link | "DCSETUP1".

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const MAGIC: &[u8; 8] = b"DCSETUP1";

/// Appends `link` to a program (what the phone does; here for tests and tools).
pub fn append(exe: &mut Vec<u8>, link: &str) {
    exe.extend_from_slice(link.as_bytes());
    exe.extend_from_slice(&(link.len() as u32).to_le_bytes());
    exe.extend_from_slice(MAGIC);
}

/// The id and key from a pairing link appended to the file at `path`, if any.
pub fn read(path: &Path) -> Option<([u8; 8], [u8; 32])> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    if len < 12 {
        return None;
    }
    let mut tail = [0u8; 12];
    f.seek(SeekFrom::End(-12)).ok()?;
    f.read_exact(&mut tail).ok()?;
    if &tail[4..] != MAGIC {
        return None;
    }
    let n = u32::from_le_bytes(tail[..4].try_into().ok()?) as u64;
    if n == 0 || n > 2048 || n + 12 > len {
        return None;
    }
    let mut link = vec![0u8; n as usize];
    f.seek(SeekFrom::End(-12 - n as i64)).ok()?;
    f.read_exact(&mut link).ok()?;
    parse(std::str::from_utf8(&link).ok()?)
}

/// id and key from "dropcopy://pair?v=1&id=<hex16>&k=<b64url>&…".
pub fn parse(link: &str) -> Option<([u8; 8], [u8; 32])> {
    let q = link.strip_prefix("dropcopy://pair?")?;
    let get = |k: &str| q.split('&').filter_map(|kv| kv.split_once('=')).find(|(a, _)| *a == k).map(|(_, v)| v);
    let id = crate::util::unhex::<8>(get("id")?)?;
    let key = <[u8; 32]>::try_from(crate::util::unb64url(get("k")?)?).ok()?;
    Some((id, key))
}

/// On a first start from a phone-served setup: take the phone's id and key. Never replaces a key
/// a phone already uses (then the code is scanned as usual), and each served setup is used once.
/// Returns true when it did.
pub fn adopt(cfg: &mut crate::config::Config, exe: &Path) -> bool {
    let Some((id, key)) = read(exe) else { return false };
    let marker = cfg.home.join(format!("handoff-{}", crate::util::hex(&id)));
    if cfg.is_paired() || marker.exists() || (cfg.id == id && cfg.key == key) {
        return false;
    }
    cfg.id = id;
    cfg.key = key;
    if cfg.save().is_err() {
        return false;
    }
    let _ = std::fs::write(marker, b"set up from the phone\n");
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_link_the_phone_appended_and_adopts_it_once() {
        let dir = std::env::temp_dir().join(format!("dropcopy-handoff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let key = [9u8; 32];
        let link = format!("dropcopy://pair?v=1&id=0011223344556677&k={}&n=Your%20PC&p=48650", crate::util::b64url(&key));
        let mut exe = b"MZ...a program...".to_vec();
        let plain = dir.join("plain.exe");
        std::fs::write(&plain, &exe).unwrap();
        assert_eq!(read(&plain), None, "a normal download carries nothing");
        append(&mut exe, &link);
        let served = dir.join("DropCopy-Setup.exe");
        std::fs::write(&served, &exe).unwrap();
        assert_eq!(read(&served), Some(([0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77], key)));

        let (mut cfg, _) = crate::config::Config::load_or_create(dir.join("home")).unwrap();
        assert!(adopt(&mut cfg, &served));
        assert_eq!(cfg.key, key);
        let (again, _) = crate::config::Config::load_or_create(dir.join("home")).unwrap();
        assert_eq!(again.key, key, "saved");
        // Later starts of the same file change nothing (e.g. after `dropcopy pair --new`).
        cfg.rekey().unwrap();
        assert!(!adopt(&mut cfg, &served));
        // A PC whose key a phone already uses keeps it.
        let (mut other, _) = crate::config::Config::load_or_create(dir.join("home2")).unwrap();
        other.mark_paired();
        assert!(!adopt(&mut other, &served));
    }
}

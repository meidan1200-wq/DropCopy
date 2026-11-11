//! What this PC is sending to a phone. A phone can't be connected to at any moment (Android stops
//! listening apps), so the PC keeps what you send in `outbox\` in the settings folder and the
//! phone fetches it: at once when it is listening (the PC knocks with an offer packet), otherwise
//! the next time it talks to the PC. Each send is one folder:
//!
//!   outbox\<created ms>-<random>\to.txt     the phone's name ("Galaxy S20+"), empty for any phone
//!                                paths.txt  files to send, by their full path (not copied)
//!                                text.txt   text for the phone's clipboard
//!                                files\     files that live only here (a clipboard picture)
//!
//! A folder is removed once a phone confirms it has everything, or after a week.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DIR: &str = "outbox";
const KEEP: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    pub to: Option<String>,
    pub files: Vec<PathBuf>,
    pub texts: Vec<String>,
}

fn root(home: &Path) -> PathBuf {
    home.join(DIR)
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

/// Queues files (sent from where they are), files to move in (temporary ones), and text.
pub fn queue(home: &Path, to: Option<&str>, files: &[PathBuf], owned: &[PathBuf], text: Option<&str>) -> io::Result<String> {
    // Sorts in the order things were sent, also within one millisecond.
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst) % 1_000_000;
    let id = format!("{}-{seq:06}{}", now_ms(), crate::util::hex(&crate::proto::random::<3>()));
    let dir = root(home).join(&id);
    let tmp = root(home).join(format!(".{id}"));
    fs::create_dir_all(tmp.join("files"))?;
    fs::write(tmp.join("to.txt"), to.unwrap_or(""))?;
    let paths: Vec<String> = files.iter().map(|p| std::path::absolute(p).unwrap_or(p.clone()).display().to_string()).collect();
    fs::write(tmp.join("paths.txt"), paths.join("\n"))?;
    if let Some(t) = text {
        fs::write(tmp.join("text.txt"), t)?;
    }
    for f in owned {
        let name = f.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| "file".into());
        let dest = tmp.join("files").join(name);
        if fs::rename(f, &dest).is_err() {
            fs::copy(f, &dest)?;
            let _ = fs::remove_file(f);
        }
    }
    // Appears whole or not at all: a fetch never sees half an entry.
    fs::rename(&tmp, &dir)?;
    Ok(id)
}

fn read(dir: &Path) -> Option<Entry> {
    let id = dir.file_name()?.to_str()?.to_string();
    if id.starts_with('.') {
        return None;
    }
    let to = fs::read_to_string(dir.join("to.txt")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let mut files: Vec<PathBuf> = fs::read_to_string(dir.join("paths.txt"))
        .unwrap_or_default()
        .lines()
        .map(|l| PathBuf::from(l.trim()))
        .filter(|p| p.is_file())
        .collect();
    if let Ok(rd) = fs::read_dir(dir.join("files")) {
        let mut own: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file()).collect();
        own.sort();
        files.extend(own);
    }
    let texts = fs::read_to_string(dir.join("text.txt")).ok().into_iter().filter(|t| !t.is_empty()).collect();
    Some(Entry { id, to, files, texts })
}

fn created(id: &str) -> Option<u128> {
    id.split('-').next()?.parse().ok()
}

fn for_device(e: &Entry, device: Option<&str>) -> bool {
    match (&e.to, device) {
        (None, _) => true,
        (Some(t), Some(d)) => t.eq_ignore_ascii_case(d),
        (Some(_), None) => false,
    }
}

/// Everything waiting for this phone (by the name it gives), oldest first. Drops expired entries
/// and ones whose files are all gone.
pub fn pending(home: &Path, device: Option<&str>) -> Vec<Entry> {
    let Ok(rd) = fs::read_dir(root(home)) else { return vec![] };
    let mut out = vec![];
    let now = now_ms();
    for d in rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()) {
        let Some(e) = read(&d) else { continue };
        let old = created(&e.id).is_none_or(|c| now.saturating_sub(c) > KEEP.as_millis());
        if old || (e.files.is_empty() && e.texts.is_empty()) {
            let _ = fs::remove_dir_all(&d);
            continue;
        }
        if for_device(&e, device) {
            out.push(e);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// True if a phone of that name (or any phone, `None`) has something waiting.
pub fn waiting(home: &Path, device: Option<&str>) -> bool {
    match device {
        Some(_) => !pending(home, device).is_empty(),
        None => fs::read_dir(root(home)).map(|rd| rd.filter_map(|e| e.ok()).any(|e| !e.file_name().to_string_lossy().starts_with('.'))).unwrap_or(false),
    }
}

/// Removes entries the phone confirmed.
pub fn delivered(home: &Path, ids: &[String]) {
    for id in ids {
        if !id.is_empty() && !id.contains(['/', '\\', '.']) {
            let _ = fs::remove_dir_all(root(home).join(id));
        }
    }
}

/// Whether the entry is still queued (a sender waits on this to say "delivered").
pub fn is_queued(home: &Path, id: &str) -> bool {
    root(home).join(id).is_dir()
}

/// Where the phone was last heard saying hello: written by the background copy, read by
/// `dropcopy share` (a separate process) to knock on the phone right away.
const PHONE_AT: &str = "phone-at.txt";

pub fn save_phone_at(home: &Path, addr: std::net::SocketAddr) {
    let _ = fs::write(home.join(PHONE_AT), format!("{addr} {}", now_ms() / 1000));
}

/// The phone's listening address, if it said hello in the last 10 minutes.
pub fn phone_at(home: &Path) -> Option<std::net::SocketAddr> {
    let s = fs::read_to_string(home.join(PHONE_AT)).ok()?;
    let (a, t) = s.trim().split_once(' ')?;
    let t: u128 = t.parse().ok()?;
    (now_ms() / 1000).saturating_sub(t).lt(&600).then(|| a.parse().ok()).flatten()
}

/// Asks the phone to come and fetch (it listens while its screen is on). Twice: Wi-Fi drops some.
pub fn knock(home: &Path, id: &[u8; 8], key: &[u8; 32]) -> bool {
    let Some(addr) = phone_at(home).or_else(crate::near::phone) else { return false };
    let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") else { return false };
    let pkt = crate::proto::offer_packet(id, key, &crate::proto::random());
    sock.send_to(&pkt, addr).is_ok() | sock.send_to(&pkt, addr).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dropcopy-outbox-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn queue_fetch_deliver() {
        let h = home("q");
        let f = h.join("photo.jpg");
        fs::write(&f, b"jpeg").unwrap();
        let clip = h.join("clipboard.png");
        fs::write(&clip, b"png").unwrap();
        let a = queue(&h, Some("Galaxy S20+"), &[f.clone()], &[clip.clone()], None).unwrap();
        let b = queue(&h, None, &[], &[], Some("a link")).unwrap();
        assert!(!clip.exists(), "moved into the outbox");
        assert!(f.exists(), "a file you send stays where it is");

        let mine = pending(&h, Some("galaxy s20+"));
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].id, a);
        assert_eq!(mine[0].files.len(), 2);
        assert_eq!(fs::read(&mine[0].files[1]).unwrap(), b"png");
        assert_eq!(mine[1].texts, vec!["a link".to_string()]);
        assert_eq!(pending(&h, Some("Pixel 9")).len(), 1, "only the one for any phone");
        assert!(waiting(&h, None));

        delivered(&h, &[a.clone(), "../x".into()]);
        assert!(!is_queued(&h, &a));
        assert!(is_queued(&h, &b));
        delivered(&h, &[b]);
        assert!(!waiting(&h, None));
    }

    #[test]
    fn entries_whose_files_are_gone_are_dropped() {
        let h = home("gone");
        let f = h.join("x.txt");
        fs::write(&f, b"x").unwrap();
        let id = queue(&h, None, &[f.clone()], &[], None).unwrap();
        fs::remove_file(&f).unwrap();
        assert!(pending(&h, None).is_empty());
        assert!(!is_queued(&h, &id));
    }
}

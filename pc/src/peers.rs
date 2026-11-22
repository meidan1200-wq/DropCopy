//! Other PCs this one can send to. A phone paired with both introduces them: it hands this PC a
//! *peer key* for each other PC (`proto::peer_key`), never that PC's pairing key. Stored as one
//! link per line in `peers.txt` in the settings folder:
//!
//!   dropcopy://peer?v=2&id=<16 hex>&k=<peer key, base64url>&n=<name>&p=<port>&a=<last ipv4>

use crate::util::{b64url, hex, unb64url, unhex, urlenc};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    /// The other PC's id.
    pub id: [u8; 8],
    /// The key that other PC accepts from this one (derived from its pairing key and our id).
    pub key: [u8; 32],
    pub name: String,
    pub port: u16,
    /// Where it was last reached; discovery finds it when this is stale.
    pub host: Option<String>,
}

const FILE: &str = "peers.txt";
/// More than anyone has; a cap keeps a confused phone from filling the file.
const MAX_PEERS: usize = 16;
static LOCK: Mutex<()> = Mutex::new(());

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Peer {
    pub fn to_link(&self) -> String {
        let a = self.host.as_deref().map(|h| format!("&a={h}")).unwrap_or_default();
        format!("dropcopy://peer?v=2&id={}&k={}&n={}&p={}{a}", hex(&self.id), b64url(&self.key), urlenc(&self.name), self.port)
    }

    pub fn parse(link: &str) -> Option<Peer> {
        let rest = link.trim().strip_prefix("dropcopy://peer?")?;
        let get = |k: &str| rest.split('&').filter_map(|kv| kv.split_once('=')).find(|(key, _)| *key == k).map(|(_, v)| url_decode(v));
        let id = unhex::<8>(&get("id")?)?;
        let key: [u8; 32] = unb64url(&get("k")?)?.try_into().ok()?;
        let name: String = get("n").unwrap_or_else(|| "PC".into()).chars().filter(|c| !c.is_control()).take(64).collect();
        let port = get("p").and_then(|p| p.parse().ok()).unwrap_or(crate::proto::TCP_PORT);
        let host = get("a").filter(|a| a.parse::<std::net::IpAddr>().is_ok());
        Some(Peer { id, key, name: if name.trim().is_empty() { "PC".into() } else { name.trim().into() }, port, host })
    }

    pub fn hex_id(&self) -> String {
        hex(&self.id)
    }
}

fn path(home: &Path) -> PathBuf {
    home.join(FILE)
}

pub fn load(home: &Path) -> Vec<Peer> {
    fs::read_to_string(path(home)).unwrap_or_default().lines().filter_map(Peer::parse).collect()
}

fn save(home: &Path, peers: &[Peer]) {
    let text: String = peers.iter().map(|p| p.to_link() + "\n").collect();
    let p = path(home);
    let tmp = p.with_extension("tmp");
    if fs::write(&tmp, text).is_ok() {
        crate::config::restrict_permissions(&tmp);
        let _ = fs::rename(tmp, p);
    }
}

/// Adds or updates the PCs a phone introduced (never this PC itself). Returns the names of PCs
/// that are new here.
pub fn merge(home: &Path, me: &[u8; 8], introduced: Vec<Peer>) -> Vec<String> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut peers = load(home);
    let before = peers.clone();
    let mut added = vec![];
    for p in introduced.into_iter().filter(|p| &p.id != me) {
        let room = peers.len() < MAX_PEERS;
        match peers.iter_mut().find(|q| q.id == p.id) {
            Some(q) => {
                q.key = p.key;
                q.name = p.name;
                q.port = p.port;
                // The phone's idea of the address may be older than ours.
                if q.host.is_none() {
                    q.host = p.host;
                }
            }
            None if room => {
                added.push(p.name.clone());
                peers.push(p);
            }
            None => {}
        }
    }
    if peers != before {
        save(home, &peers);
    }
    added
}

/// Remembers where a peer was just reached.
pub fn reached(home: &Path, id: &[u8; 8], host: &str, port: u16) {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut peers = load(home);
    if let Some(p) = peers.iter_mut().find(|p| &p.id == id) {
        if p.host.as_deref() != Some(host) || p.port != port {
            p.host = Some(host.to_string());
            p.port = port;
            save(home, &peers);
        }
    }
}

/// The name a peer goes by, if it is known here.
pub fn name_of(home: &Path, id: &[u8; 8]) -> Option<String> {
    load(home).into_iter().find(|p| &p.id == id).map(|p| p.name)
}

/// Splits a `T_PEERS` body: `(len u16 | link)*`.
pub fn parse_list(body: &[u8]) -> Vec<Peer> {
    let mut out = vec![];
    let mut at = 0;
    while at + 2 <= body.len() && out.len() < MAX_PEERS {
        let n = u16::from_be_bytes([body[at], body[at + 1]]) as usize;
        let Some(link) = body.get(at + 2..at + 2 + n) else { break };
        if let Some(p) = Peer::parse(&String::from_utf8_lossy(link)) {
            out.push(p);
        }
        at += 2 + n;
    }
    out
}

pub fn encode_list(peers: &[Peer]) -> Vec<u8> {
    let mut b = vec![];
    for p in peers {
        let l = p.to_link();
        b.extend_from_slice(&(l.len() as u16).to_be_bytes());
        b.extend_from_slice(l.as_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dropcopy-peers-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn peer(id: u8, name: &str) -> Peer {
        Peer { id: [id; 8], key: [id; 32], name: name.into(), port: 48650, host: Some("192.168.1.20".into()) }
    }

    #[test]
    fn links_roundtrip_with_odd_names() {
        let p = peer(1, "מחשב של מידן & Co");
        assert_eq!(Peer::parse(&p.to_link()), Some(p));
        assert_eq!(Peer::parse("dropcopy://pair?v=1&id=0101010101010101&k=AA"), None);
        assert_eq!(Peer::parse("dropcopy://peer?v=2&id=01&k=AA"), None);
        let list = vec![peer(1, "A"), peer(2, "B")];
        assert_eq!(parse_list(&encode_list(&list)), list);
        assert!(parse_list(&[0, 200, b'x']).is_empty(), "a cut-off list is ignored, not a panic");
    }

    #[test]
    fn merge_skips_self_and_keeps_addresses() {
        let h = home("merge");
        let me = [9u8; 8];
        assert_eq!(merge(&h, &me, vec![peer(1, "Desk"), peer(9, "Me")]), vec!["Desk".to_string()]);
        assert_eq!(load(&h).len(), 1);
        reached(&h, &[1; 8], "10.0.0.5", 48651);
        let mut renamed = peer(1, "Desk PC");
        renamed.key = [7; 32];
        assert!(merge(&h, &me, vec![renamed]).is_empty());
        let p = &load(&h)[0];
        assert_eq!((p.name.as_str(), p.key, p.host.as_deref(), p.port), ("Desk PC", [7; 32], Some("10.0.0.5"), 48650));
        assert_eq!(name_of(&h, &[1; 8]).as_deref(), Some("Desk PC"));
    }
}

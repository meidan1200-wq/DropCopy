//! v2: a PC sending to another PC it was introduced to, a phone fetching what a PC queued for it,
//! and the status answer a phone uses to pick the PC in use. Real sockets, real files.

use dropcopy::client::{self, Item};
use dropcopy::clipboard::ClipMode;
use dropcopy::config::Config;
use dropcopy::peers::{self, Peer};
use dropcopy::proto::*;
use dropcopy::server::{local_addr, ServeOpts, Server};
use std::io::{BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dropcopy-v2-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn pc(dir: &std::path::Path, name: &str) -> (Config, SocketAddr) {
    let (mut cfg, _) = Config::load_or_create(dir.join(name)).unwrap();
    cfg.save_dir = dir.join(name).join("received");
    cfg.name = name.into();
    let server = Server::bind(Arc::new(cfg.clone()), ServeOpts { port: 0, clip: ClipMode::Off, discovery: false, popup: false, pairing: false }).unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], local_addr(&server).port()));
    std::thread::spawn(move || server.run());
    (cfg, addr)
}

/// What a phone does: connect with the pairing key, send `records`, END, then read the answer
/// (and with FETCH, the items before it). Returns (records received, OK body).
fn phone(addr: SocketAddr, key: &[u8; 32], records: &[(u8, Vec<u8>)], ack: bool) -> (Vec<(u8, Vec<u8>)>, Vec<u8>) {
    let s = TcpStream::connect(addr).unwrap();
    let mut w = s.try_clone().unwrap();
    let mut r = BufReader::new(s);
    let (mut tx, mut rx) = client_handshake(&mut r, &mut w, key).unwrap();
    let mut buf = Vec::new();
    for (t, b) in records {
        tx.write(&mut w, *t, b, &mut buf).unwrap();
    }
    tx.write(&mut w, T_END, b"", &mut buf).unwrap();
    w.flush().unwrap();
    let mut got = vec![];
    loop {
        let t = rx.read(&mut r, &mut buf).unwrap().expect("an answer");
        if t == T_OK {
            if ack {
                tx.write(&mut w, T_OK, &[0, 0], &mut buf).unwrap();
                w.flush().unwrap();
            }
            return (got, buf[1..].to_vec());
        }
        assert_ne!(t, T_ERR, "{}", String::from_utf8_lossy(&buf[1..]));
        got.push((t, buf[1..].to_vec()));
    }
}

#[test]
fn phone_fetches_what_the_pc_queued_once_it_confirms() {
    let dir = temp("fetch");
    let (cfg, addr) = pc(&dir, "Laptop");
    let photo = dir.join("photo.jpg");
    let data: Vec<u8> = (0..1_300_000u32).map(|i| (i * 7 % 251) as u8).collect();
    std::fs::write(&photo, &data).unwrap();
    let to = |name: &str, id: Option<&str>| dropcopy::config::Phone { name: name.into(), id: id.map(Into::into) };
    dropcopy::outbox::queue(&cfg.home, Some(&to("Galaxy S20+", None)), &[photo.clone()], &[], None).unwrap();
    dropcopy::outbox::queue(&cfg.home, Some(&to("Pixel 9", None)), &[], &[], Some("for the other phone")).unwrap();

    // A plain check-in learns something is waiting (and that the PC speaks v2).
    let device = (T_DEVICE, b"Galaxy S20+".to_vec());
    let (_, ok) = phone(addr, &cfg.key, &[device.clone()], false);
    assert_eq!(ok, vec![0, 0, OK_V2 | OK_WAITING]);

    // A fetch that never confirms leaves it queued.
    let (items, _) = phone(addr, &cfg.key, &[device.clone(), (T_FETCH, vec![])], false);
    assert_eq!(items.iter().filter(|(t, _)| *t == T_DATA).map(|(_, b)| b.len()).sum::<usize>(), data.len());
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(dropcopy::outbox::pending(&cfg.home, Some("Galaxy S20+"), None).len(), 1);

    // Confirmed: delivered and gone; the other phone's item stays.
    let (items, _) = phone(addr, &cfg.key, &[device.clone(), (T_FETCH, vec![])], true);
    let header = parse_file_header(&items[0].1).unwrap();
    assert_eq!((items[0].0, header.name.as_str(), header.size), (T_FILE, "photo.jpg", data.len() as u64));
    let body: Vec<u8> = items.iter().filter(|(t, _)| *t == T_DATA).flat_map(|(_, b)| b.clone()).collect();
    assert_eq!(body, data);
    assert_eq!(items.last().unwrap().0, T_FILE_END);
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(dropcopy::outbox::pending(&cfg.home, Some("Galaxy S20+"), None).len() == 0);
    let (_, ok) = phone(addr, &cfg.key, &[device.clone()], false);
    assert_eq!(ok[2] & OK_WAITING, 0);
    let (items, _) = phone(addr, &cfg.key, &[(T_DEVICE, b"Pixel 9".to_vec()), (T_FETCH, vec![])], true);
    assert_eq!(items, vec![(T_TEXT, b"for the other phone".to_vec())]);

    // Two phones called the same: what was sent to one (by its id) never goes to the other.
    dropcopy::outbox::queue(&cfg.home, Some(&to("Galaxy S20+", Some("a1"))), &[], &[], Some("for a1")).unwrap();
    let twin = [device.clone(), (T_PHONE, b"b2".to_vec())];
    let (_, ok) = phone(addr, &cfg.key, &twin, false);
    assert_eq!(ok[2] & OK_WAITING, 0, "nothing waits for the other phone");
    let (items, _) = phone(addr, &cfg.key, &[twin[0].clone(), twin[1].clone(), (T_FETCH, vec![])], true);
    assert!(items.is_empty());
    let (items, _) = phone(addr, &cfg.key, &[device.clone(), (T_PHONE, b"a1".to_vec()), (T_FETCH, vec![])], true);
    assert_eq!(items, vec![(T_TEXT, b"for a1".to_vec())]);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let phones = dropcopy::config::phone_list(&cfg.home);
    assert_eq!(phones.iter().filter(|p| p.name == "Galaxy S20+").count(), 2, "both remembered: {phones:?}");
}

#[test]
fn a_phone_introduces_two_pcs_and_they_send_to_each_other() {
    let dir = temp("peers");
    let (a, a_addr) = pc(&dir, "Laptop");
    let (b, b_addr) = pc(&dir, "Desktop");

    // The phone knows both pairing keys; each PC gets a peer key for the other, never its key.
    let intro = |to: &Config, other: &Config, other_addr: SocketAddr| Peer {
        id: other.id,
        key: peer_key(&other.key, &to.id),
        name: other.name.clone(),
        port: other_addr.port(),
        host: Some("127.0.0.1".into()),
    };
    let to_a = peers::encode_list(&[intro(&a, &b, b_addr), Peer { id: a.id, key: [0; 32], name: "self".into(), port: 1, host: None }]);
    phone(a_addr, &a.key, &[(T_PEERS, to_a)], false);
    phone(b_addr, &b.key, &[(T_PEERS, peers::encode_list(&[intro(&b, &a, a_addr)]))], false);
    let a_peers = peers::load(&a.home);
    assert_eq!(a_peers.len(), 1, "never itself");
    assert_eq!(a_peers[0].name, "Desktop");
    assert_ne!(a_peers[0].key, b.key);

    let f = dir.join("notes.txt");
    std::fs::write(&f, "from the laptop").unwrap();
    client::send_to_peer(&a, &a_peers[0], &[Item::File(f), Item::Text("hi desktop".into())]).unwrap();
    assert_eq!(std::fs::read_to_string(b.save_dir.join("notes.txt")).unwrap(), "from the laptop");

    let b_peers = peers::load(&b.home);
    std::fs::write(dir.join("back.txt"), "and back").unwrap();
    client::send_to_peer(&b, &b_peers[0], &[Item::File(dir.join("back.txt"))]).unwrap();
    assert_eq!(std::fs::read_to_string(a.save_dir.join("back.txt")).unwrap(), "and back");

    // A peer key for a different PC id is refused, and so is a forged key.
    let mut wrong = a_peers[0].clone();
    wrong.key = [1; 32];
    assert!(client::send_to_peer(&a, &wrong, &[Item::Text("x".into())]).is_err());
    let mut impostor = a.clone();
    impostor.id = [0x42; 8];
    assert!(client::send_to_peer(&impostor, &a_peers[0], &[Item::Text("x".into())]).is_err());
}

#[test]
fn status_tells_a_paired_phone_how_long_the_pc_has_been_idle() {
    let dir = temp("status");
    let (mut cfg, _) = Config::load_or_create(dir.join("home")).unwrap();
    cfg.save_dir = dir.join("received");
    std::env::set_var("DROPCOPY_IDLE", "42");
    let server = Server::bind(Arc::new(cfg.clone()), ServeOpts { port: 0, clip: ClipMode::Off, discovery: true, popup: false, pairing: false }).unwrap();
    if !server.discovering() {
        eprintln!("skipped: the discovery port is taken on this machine");
        return;
    }
    let port = local_addr(&server).port();
    std::thread::spawn(move || server.run());
    let hint = Some(SocketAddr::from(([127, 0, 0, 1], port)));
    let (addr, st) = client::ask_status(&cfg.id, &cfg.key, &[0; 8], hint, std::time::Duration::from_secs(2)).unwrap();
    assert_eq!((addr.port(), st.idle_s), (port, 42));
    assert_ne!(st.flags & OK_V2 as u16, 0);
    // A stranger with another key gets no answer at all.
    assert!(client::ask_status(&cfg.id, &[7; 32], &[0; 8], hint, std::time::Duration::from_millis(600)).is_err());
}

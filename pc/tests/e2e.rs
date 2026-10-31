//! Real sockets, real files: a receiver and a sender in one process.

use dropcopy::client::{self, Item};
use dropcopy::clipboard::ClipMode;
use dropcopy::config::Config;
use dropcopy::server::{handle, local_addr, ServeOpts, Server};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

/// The listener binds 0.0.0.0, which Windows refuses as a destination; connect via loopback.
fn loopback(addr: std::net::SocketAddr) -> String {
    format!("127.0.0.1:{}", addr.port())
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dropcopy-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn config(dir: &std::path::Path) -> Config {
    let (mut cfg, created) = Config::load_or_create(dir.join("home")).unwrap();
    assert!(created);
    cfg.save_dir = dir.join("received");
    cfg
}

#[test]
fn files_and_text_arrive_intact() {
    let dir = temp("intact");
    let cfg = config(&dir);
    let key = cfg.key;
    let id = cfg.id;
    let save_dir = cfg.save_dir.clone();
    let server = Server::bind(Arc::new(cfg), ServeOpts { port: 0, clip: ClipMode::Off, discovery: false, popup: false, pairing: false }).unwrap();
    let addr = local_addr(&server);
    std::thread::spawn(move || server.run());

    // 3 MB of non-repeating data spans several records; a second file has the same name.
    let big: Vec<u8> = (0..3_000_123u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("photo.jpg"), &big).unwrap();
    std::fs::write(src.join("empty.txt"), b"").unwrap();

    let (sender, _) = Config::load_or_create(dir.join("home")).unwrap();
    assert_eq!((sender.key, sender.id), (key, id));
    let items = vec![
        Item::File(src.join("photo.jpg")),
        Item::File(src.join("photo.jpg")),
        Item::File(src.join("empty.txt")),
        Item::Text("שלום clipboard".into()),
    ];
    let sent = client::send(&sender, Some(&loopback(addr)), &items).unwrap();
    assert_eq!(sent.bytes, 2 * big.len() as u64);

    assert_eq!(std::fs::read(save_dir.join("photo.jpg")).unwrap(), big);
    assert_eq!(std::fs::read(save_dir.join("photo (2).jpg")).unwrap(), big);
    assert_eq!(std::fs::read(save_dir.join("empty.txt")).unwrap(), b"");
    let leftovers: Vec<_> = std::fs::read_dir(&save_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn wrong_key_is_refused_and_leaves_nothing() {
    let dir = temp("wrongkey");
    let cfg = config(&dir);
    let save_dir = cfg.save_dir.clone();
    let server = Server::bind(Arc::new(cfg), ServeOpts { port: 0, clip: ClipMode::Off, discovery: false, popup: false, pairing: false }).unwrap();
    let addr = local_addr(&server);
    std::thread::spawn(move || server.run());

    let (mut stranger, _) = Config::load_or_create(dir.join("other")).unwrap();
    stranger.last_peer = None;
    std::fs::write(dir.join("x.bin"), b"data").unwrap();
    let err = client::send(&stranger, Some(&loopback(addr)), &[Item::File(dir.join("x.bin"))]);
    assert!(err.is_err());
    assert!(!save_dir.exists() || std::fs::read_dir(&save_dir).unwrap().next().is_none());
}

#[test]
fn interrupted_transfer_removes_partial_file() {
    use dropcopy::proto::*;
    let dir = temp("interrupt");
    let cfg = config(&dir);
    let key = cfg.key;
    let save_dir = cfg.save_dir.clone();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let srv = std::thread::spawn(move || {
        let (s, _) = l.accept().unwrap();
        handle(s, &cfg)
    });
    {
        let mut c = std::net::TcpStream::connect(addr).unwrap();
        let mut c2 = c.try_clone().unwrap();
        let (mut tx, _) = client_handshake(&mut c, &mut c2, &key).unwrap();
        let mut buf = Vec::new();
        tx.write(&mut c2, T_FILE, &file_header(10, "half.jpg", "image/jpeg"), &mut buf).unwrap();
        tx.write(&mut c2, T_DATA, b"12345", &mut buf).unwrap();
    } // dropped: connection closes mid-file
    assert!(srv.join().unwrap().is_err());
    assert_eq!(std::fs::read_dir(&save_dir).unwrap().count(), 0);
}

#[test]
fn discovery_finds_the_receiver() {
    // Uses the fixed UDP port; skipped if a real receiver already owns it.
    if std::net::UdpSocket::bind(("0.0.0.0", dropcopy::proto::UDP_PORT)).is_err() {
        eprintln!("skipped: UDP port in use by a running receiver");
        return;
    }
    let dir = temp("discover");
    let cfg = config(&dir);
    let server = match Server::bind(Arc::new(cfg), ServeOpts { port: 0, clip: ClipMode::Off, discovery: true, popup: false, pairing: false }) {
        Ok(s) => s,
        Err(_) => return,
    };
    let port = server.port;
    std::thread::spawn(move || server.run());
    let (sender, _) = Config::load_or_create(dir.join("home")).unwrap();
    let found = client::discover(&sender, std::time::Duration::from_secs(2)).unwrap();
    assert_eq!(found.port(), port);

    // An unpaired phone asking who is here gets the PC's name, and nothing secret.
    let phone = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    phone.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
    phone.send_to(b"DCQ1\x01\x02\x03\x04\x05\x06\x07\x08", ("127.0.0.1", dropcopy::proto::UDP_PORT)).unwrap();
    let mut buf = [0u8; 128];
    let (n, _) = phone.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..12], b"DCA1\x01\x02\x03\x04\x05\x06\x07\x08");
    assert_eq!(std::str::from_utf8(&buf[12..n]).unwrap(), sender.name);
    assert!(n <= 12 + 64);
}

#[test]
fn a_phone_that_connects_marks_the_pc_paired() {
    let dir = temp("paired");
    let cfg = config(&dir);
    assert!(!cfg.is_paired());
    let server = Server::bind(Arc::new(cfg), ServeOpts { port: 0, clip: ClipMode::Off, discovery: false, popup: false, pairing: false }).unwrap();
    let addr = local_addr(&server);
    std::thread::spawn(move || server.run());
    let (mut phone, _) = Config::load_or_create(dir.join("home")).unwrap();
    client::send(&phone, Some(&loopback(addr)), &[Item::Text("hi".into())]).unwrap();
    // The receiver records it just after acknowledging the transfer.
    let t0 = std::time::Instant::now();
    while !phone.is_paired() && t0.elapsed() < std::time::Duration::from_secs(3) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(phone.is_paired());
    // A new key forgets it, so the next start shows the code again.
    phone.rekey().unwrap();
    assert!(!phone.is_paired());
}

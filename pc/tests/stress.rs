//! Limits: big files, many files, many phones at once, and a hostile network. Each test states the
//! number it holds the receiver to, so a slowdown or a leak fails loudly instead of creeping in.
//!
//! Throughput numbers assume an optimised build: `cargo test --release --test stress`.

use dropcopy::client::{self, Item};
use dropcopy::clipboard::ClipMode;
use dropcopy::config::Config;
use dropcopy::proto::{self, Records};
use dropcopy::server::{local_addr, ServeOpts, Server};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// One stress test at a time: each measures speed, memory or CPU for the whole process, which
/// tests running alongside would distort.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn turn() -> std::sync::MutexGuard<'static, ()> {
    TURN.lock().unwrap_or_else(|e| e.into_inner())
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dropcopy-stress-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Rig {
    dir: PathBuf,
    addr: String,
    sock: SocketAddr,
    phone: Config,
    save_dir: PathBuf,
}

fn rig(name: &str) -> Rig {
    let dir = temp(name);
    let (mut cfg, _) = Config::load_or_create(dir.join("home")).unwrap();
    cfg.save_dir = dir.join("received");
    let save_dir = cfg.save_dir.clone();
    let server = Server::bind(Arc::new(cfg), ServeOpts { port: 0, clip: ClipMode::Off, discovery: false, popup: false, pairing: false }).unwrap();
    let port = local_addr(&server).port();
    std::thread::spawn(move || server.run());
    let (phone, _) = Config::load_or_create(dir.join("home")).unwrap();
    Rig { dir, addr: format!("127.0.0.1:{port}"), sock: SocketAddr::from(([127, 0, 0, 1], port)), phone, save_dir }
}

/// Peak resident memory of this process, in MB (Linux; elsewhere 0 so the check is skipped).
fn peak_mb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmHWM:")).map(|l| l.split_whitespace().nth(1).unwrap_or("0").parse::<u64>().unwrap_or(0) / 1024))
        .unwrap_or(0)
}

fn pseudo_random_file(path: &Path, size: usize, seed: u32) {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut x = seed.wrapping_mul(2654435761) | 1;
    let mut chunk = vec![0u8; 1 << 20];
    let mut left = size;
    while left > 0 {
        for b in chunk.iter_mut() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = x as u8;
        }
        let n = left.min(chunk.len());
        f.write_all(&chunk[..n]).unwrap();
        left -= n;
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    let (mut fa, mut fb) = (std::fs::File::open(a).unwrap(), std::fs::File::open(b).unwrap());
    let (mut ba, mut bb) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
    loop {
        let na = fa.read(&mut ba).unwrap();
        let mut nb = 0;
        while nb < na {
            let k = fb.read(&mut bb[nb..na]).unwrap();
            if k == 0 {
                return false;
            }
            nb += k;
        }
        if na == 0 {
            return fb.read(&mut bb[..1]).unwrap() == 0;
        }
        if ba[..na] != bb[..na] {
            return false;
        }
    }
}

#[test]
fn a_big_video_streams_fast_in_bounded_memory() {
    let _turn = turn();
    let r = rig("big");
    // A 4K phone video; smaller in an unoptimised build, where the cipher alone is slow.
    let size = if cfg!(debug_assertions) { 64 << 20 } else { 512 << 20 };
    let src = r.dir.join("video.mp4");
    pseudo_random_file(&src, size, 7);
    let before = peak_mb();
    let sent = client::send(&r.phone, Some(&r.addr), &[Item::File(src.clone())]).unwrap();
    let mbps = size as f64 / sent.elapsed.as_secs_f64() / 1048576.0;
    let grew = peak_mb().saturating_sub(before);
    println!("{} MB in {:?}: {mbps:.0} MB/s, peak memory grew {grew} MB", size >> 20, sent.elapsed);
    assert!(same_file(&src, &r.save_dir.join("video.mp4")), "arrives intact");
    // Streams in 512 KB records: memory must not grow with the file.
    assert!(grew < 64, "memory grew {grew} MB for a {} MB file", size >> 20);
    if !cfg!(debug_assertions) {
        // Loopback, encrypted both ways in one process: far above any Wi-Fi link.
        assert!(mbps > 150.0, "{mbps:.0} MB/s");
    }
}

#[test]
fn hundreds_of_photos_in_one_share() {
    let _turn = turn();
    let r = rig("many");
    let src = r.dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let mut items = vec![];
    for i in 0..500 {
        let p = src.join(format!("IMG_{i:04}.jpg"));
        pseudo_random_file(&p, 1000 + i * 97, i as u32);
        items.push(Item::File(p));
    }
    let t0 = Instant::now();
    client::send(&r.phone, Some(&r.addr), &items).unwrap();
    println!("500 files in {:?}", t0.elapsed());
    for i in 0..500 {
        let name = format!("IMG_{i:04}.jpg");
        assert!(same_file(&src.join(&name), &r.save_dir.join(&name)), "{name}");
    }
    assert!(t0.elapsed() < Duration::from_secs(10));
}

#[test]
fn many_phones_at_once() {
    let _turn = turn();
    let r = rig("concurrent");
    let phones = 24;
    let srcs: Vec<PathBuf> = (0..phones)
        .map(|i| {
            let p = r.dir.join(format!("phone{i}.png"));
            pseudo_random_file(&p, 3 << 20, 100 + i as u32);
            p
        })
        .collect();
    let t0 = Instant::now();
    let handles: Vec<_> = srcs
        .iter()
        .cloned()
        .map(|p| {
            let (phone, addr) = (r.phone.clone(), r.addr.clone());
            std::thread::spawn(move || client::send(&phone, Some(&addr), &[Item::File(p)]).map(|_| ()))
        })
        .collect();
    for h in handles {
        h.join().unwrap().unwrap();
    }
    println!("{phones} phones × 3 MB at once in {:?}", t0.elapsed());
    for p in &srcs {
        assert!(same_file(p, &r.save_dir.join(p.file_name().unwrap())));
    }
}

#[test]
fn file_names_cannot_escape_the_save_folder() {
    let _turn = turn();
    let r = rig("names");
    let names = ["../../escape.txt", "..\\..\\escape2.txt", "/etc/passwd", "C:\\Windows\\win.ini", "CON", "nul.txt", "a:b*c?.txt", &"long".repeat(200), "", ".", "..", "שלום עולם.jpg", "trailing. "];
    for n in names {
        send_named(&r, n, b"x");
    }
    // Everything landed inside the save folder, nothing outside it.
    for e in std::fs::read_dir(&r.dir).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        assert!(["home", "received"].contains(&name.as_str()), "escaped: {name}");
    }
    let saved: Vec<String> = std::fs::read_dir(&r.save_dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(saved.len(), names.len(), "{saved:?}");
    for s in &saved {
        assert!(!s.contains('/') && !s.contains('\\') && !s.contains(':') && s.len() <= 255, "{s}");
        let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
        assert!(!["CON", "NUL", "PRN", "AUX"].contains(&stem.as_str()), "reserved Windows name: {s}");
    }
}

/// One file under an arbitrary name, written record by record like a phone would.
fn send_named(r: &Rig, name: &str, data: &[u8]) {
    let mut s = TcpStream::connect(r.sock).unwrap();
    let mut rd = s.try_clone().unwrap();
    let (mut tx, mut rx) = proto::client_handshake(&mut rd, &mut s, &r.phone.key).unwrap();
    let mut buf = Vec::new();
    tx.write(&mut s, proto::T_FILE, &proto::file_header(data.len() as u64, name, "text/plain"), &mut buf).unwrap();
    tx.write(&mut s, proto::T_DATA, data, &mut buf).unwrap();
    tx.write(&mut s, proto::T_FILE_END, &[], &mut buf).unwrap();
    tx.write(&mut s, proto::T_END, &[], &mut buf).unwrap();
    s.flush().unwrap();
    let mut reply = Vec::new();
    assert_eq!(rx.read(&mut rd, &mut reply).unwrap(), Some(proto::T_OK), "{name:?}");
}

#[test]
fn hostile_connections_do_not_hurt_real_ones() {
    let _turn = turn();
    let r = rig("hostile");
    let before = peak_mb();
    // Garbage, a half hello, a claimed 4 GB record, and many that connect and say nothing.
    let mut junk = vec![];
    for i in 0..200 {
        let mut s = TcpStream::connect(r.sock).unwrap();
        match i % 4 {
            0 => {
                let _ = s.write_all(&[0xAB; 4096]);
            }
            1 => {
                let _ = s.write_all(b"DCP1\x01\x02");
            }
            2 => {
                let _ = s.write_all(b"DCP1");
                let _ = s.write_all(&[7u8; 16]);
                let mut resp = [0u8; 52];
                let _ = s.read_exact(&mut resp);
                let _ = s.write_all(&u32::MAX.to_be_bytes());
            }
            _ => {}
        }
        junk.push(s);
    }
    // A real phone still gets through, quickly, while all of that is open.
    let src = r.dir.join("real.jpg");
    pseudo_random_file(&src, 2 << 20, 9);
    let t0 = Instant::now();
    client::send(&r.phone, Some(&r.addr), &[Item::File(src.clone())]).unwrap();
    assert!(t0.elapsed() < Duration::from_secs(3), "{:?}", t0.elapsed());
    assert!(same_file(&src, &r.save_dir.join("real.jpg")));
    let grew = peak_mb().saturating_sub(before);
    assert!(grew < 100, "memory grew {grew} MB under 200 hostile connections");
    // Nothing half-written is left from the ones that gave up.
    drop(junk);
    std::thread::sleep(Duration::from_millis(300));
    let parts = std::fs::read_dir(&r.save_dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".part")).count();
    assert_eq!(parts, 0);
}

#[test]
fn a_silent_connection_is_dropped_soon() {
    let _turn = turn();
    let r = rig("silent");
    let mut s = TcpStream::connect(r.sock).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let t0 = Instant::now();
    let mut b = [0u8; 1];
    let n = s.read(&mut b).unwrap_or(0);
    assert_eq!(n, 0, "closed without a reply");
    // Someone who connects and never says hello holds a thread for seconds, not minutes.
    assert!(t0.elapsed() < Duration::from_secs(8), "{:?}", t0.elapsed());
}

#[test]
fn a_transfer_cut_off_midway_leaves_nothing_behind() {
    let _turn = turn();
    let r = rig("cutoff");
    let mut s = TcpStream::connect(r.sock).unwrap();
    let mut rd = s.try_clone().unwrap();
    let (mut tx, _rx) = proto::client_handshake(&mut rd, &mut s, &r.phone.key).unwrap();
    let mut buf = Vec::new();
    tx.write(&mut s, proto::T_FILE, &proto::file_header(10 << 20, "half.mp4", "video/mp4"), &mut buf).unwrap();
    tx.write(&mut s, proto::T_DATA, &vec![1u8; 512 << 10], &mut buf).unwrap();
    drop(s); // Wi-Fi gone
    drop(rd);
    std::thread::sleep(Duration::from_millis(500));
    let left: Vec<_> = std::fs::read_dir(&r.save_dir).map(|d| d.flatten().map(|e| e.file_name()).collect()).unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn records_reject_tampering_and_replay() {
    let _turn = turn();
    let key = [5u8; 32];
    let (mut a, mut b) = (Records::new(&key), Records::new(&key));
    let mut wire = Vec::new();
    let mut buf = Vec::new();
    a.write(&mut wire, proto::T_TEXT, b"hello", &mut buf).unwrap();
    a.write(&mut wire, proto::T_TEXT, b"again", &mut buf).unwrap();
    let first_len = 4 + u32::from_be_bytes(wire[..4].try_into().unwrap()) as usize;
    // Flip one bit in the first record: refused.
    let mut bad = wire.clone();
    bad[10] ^= 1;
    assert!(b.read(&mut &bad[..], &mut buf).is_err());
    // Replaying the first record where the second belongs: refused (the nonce is a counter).
    let (mut c, mut d) = (Records::new(&key), Vec::new());
    let mut replay = wire[..first_len].to_vec();
    replay.extend_from_slice(&wire[..first_len]);
    let mut rd = &replay[..];
    assert_eq!(c.read(&mut rd, &mut d).unwrap(), Some(proto::T_TEXT));
    assert!(c.read(&mut rd, &mut d).is_err());
}

#[test]
fn discovery_survives_a_flood() {
    let _turn = turn();
    if std::net::UdpSocket::bind(("0.0.0.0", proto::UDP_PORT)).is_err() {
        eprintln!("skipped: UDP port in use by a running receiver");
        return;
    }
    let dir = temp("flood");
    let (mut cfg, _) = Config::load_or_create(dir.join("home")).unwrap();
    cfg.save_dir = dir.join("received");
    let server = Server::bind(Arc::new(cfg.clone()), ServeOpts { port: 0, clip: ClipMode::Off, discovery: true, popup: false, pairing: false }).unwrap();
    std::thread::spawn(move || server.run());
    let flood = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let t0 = Instant::now();
    for i in 0..20_000u32 {
        let mut p = [0u8; 44];
        p[..4].copy_from_slice(if i % 2 == 0 { b"DCH1" } else { b"DCN1" });
        p[4..8].copy_from_slice(&i.to_be_bytes());
        let _ = flood.send_to(&p[..(i as usize % 45)], ("127.0.0.1", proto::UDP_PORT));
    }
    println!("20k junk packets sent in {:?}", t0.elapsed());
    let found = client::discover(&cfg, Duration::from_secs(3)).unwrap();
    assert!(found.port() > 0);
}

#[test]
fn a_connection_flood_is_capped_and_recovers() {
    let _turn = turn();
    let r = rig("flood-tcp");
    // 400 sockets that never speak: past the cap (256) the receiver closes them at once.
    let mut held = vec![];
    for _ in 0..400 {
        if let Ok(s) = TcpStream::connect(r.sock) {
            held.push(s);
        }
    }
    std::thread::sleep(Duration::from_millis(300));
    let mut closed = 0;
    for s in held.iter_mut() {
        s.set_read_timeout(Some(Duration::from_millis(5))).unwrap();
        if matches!(s.read(&mut [0u8; 1]), Ok(0)) {
            closed += 1;
        }
    }
    assert!(closed >= 400 - 256, "only {closed} of the excess were refused");
    // Within the hello timeout the silent ones are let go, and a real phone gets in again.
    let src = r.dir.join("after.jpg");
    pseudo_random_file(&src, 1 << 20, 3);
    std::thread::sleep(Duration::from_secs(5));
    client::send(&r.phone, Some(&r.addr), &[Item::File(src.clone())]).unwrap();
    assert!(same_file(&src, &r.save_dir.join("after.jpg")));
}

/// CPU time this process has used, in ms (Linux; elsewhere None and the check is skipped).
fn cpu_ms() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/stat").ok()?;
    let f: Vec<&str> = s.rsplit(')').next()?.split_whitespace().collect();
    let ticks: u64 = f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?;
    Some(ticks * 10) // USER_HZ is 100 on Linux
}

#[test]
fn an_idle_receiver_uses_no_cpu_and_check_ins_are_quick() {
    let _turn = turn();
    let r = rig("idle");
    // 200 check-ins in a row (what "Check connection" and every send start with).
    let t0 = Instant::now();
    for _ in 0..200 {
        client::send(&r.phone, Some(&r.addr), &[]).unwrap();
    }
    let per = t0.elapsed() / 200;
    println!("check-in: {per:?} each");
    assert!(per < Duration::from_millis(if cfg!(debug_assertions) { 40 } else { 10 }), "{per:?}");
    // Then idle: blocked in accept, no timers spinning. (Other tests may run alongside, so this
    // only catches a busy loop, which would burn a whole core.)
    if let Some(before) = cpu_ms() {
        std::thread::sleep(Duration::from_secs(3));
        let used = cpu_ms().unwrap() - before;
        println!("3 s idle: {used} ms CPU for the whole test process");
        assert!(used < 2500, "{used} ms");
    }
}

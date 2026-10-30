//! Sender, used by `dropcopy send` (PC to PC, and the tests). The Android app speaks the
//! same protocol from `android/app/src/main/java/dev/dropcopy/Proto.java`.

use crate::config::Config;
use crate::proto::*;
use crate::util::mime_for;
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub enum Item {
    File(PathBuf),
    Text(String),
}

pub struct Sent {
    pub to: SocketAddr,
    pub bytes: u64,
    pub elapsed: Duration,
}

/// Broadcasts a discovery request and waits for the first authentic reply.
pub fn discover(cfg: &Config, timeout: Duration) -> io::Result<SocketAddr> {
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.set_broadcast(true)?;
    let nonce: [u8; 16] = random();
    let pkt = discover_packet(&cfg.id, &nonce);
    let targets = [Ipv4Addr::BROADCAST, Ipv4Addr::LOCALHOST];
    let deadline = Instant::now() + timeout;
    let mut next_send = Instant::now();
    let mut buf = [0u8; 64];
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "no DropCopy receiver answered on this network"));
        }
        if now >= next_send {
            for t in targets {
                let _ = sock.send_to(&pkt, (t, UDP_PORT));
            }
            next_send = now + Duration::from_millis(250);
        }
        sock.set_read_timeout(Some((deadline.min(next_send) - now).max(Duration::from_millis(5))))?;
        if let Ok((n, from)) = sock.recv_from(&mut buf) {
            if let Some(port) = parse_reply(&buf[..n], &cfg.id, &cfg.key, &nonce) {
                return Ok(SocketAddr::new(from.ip(), port));
            }
        }
    }
}

fn resolve(target: &str) -> io::Result<SocketAddr> {
    if let Ok(ip) = target.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, TCP_PORT));
    }
    target
        .to_socket_addrs()
        .or_else(|_| (target, TCP_PORT).to_socket_addrs())?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("cannot resolve {target}")))
}

/// Connects to `target` if given, else the last known address, else whoever answers discovery.
pub fn connect(cfg: &Config, target: Option<&str>) -> io::Result<TcpStream> {
    if let Some(t) = target {
        return TcpStream::connect_timeout(&resolve(t)?, Duration::from_secs(3));
    }
    if let Some(last) = cfg.last_peer.as_deref().and_then(|p| p.parse::<SocketAddr>().ok()) {
        if let Ok(s) = TcpStream::connect_timeout(&last, Duration::from_millis(300)) {
            return Ok(s);
        }
    }
    let addr = discover(cfg, Duration::from_secs(2))?;
    TcpStream::connect_timeout(&addr, Duration::from_secs(3))
}

pub fn send(cfg: &Config, target: Option<&str>, items: &[Item]) -> io::Result<Sent> {
    let t0 = Instant::now();
    let stream = connect(cfg, target)?;
    let to = stream.peer_addr()?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    let mut r = stream.try_clone()?;
    let mut w = io::BufWriter::with_capacity(CHUNK + 64, stream);
    let (mut tx, mut rx) = client_handshake(&mut r, &mut w, &cfg.key)?;

    let mut buf = Vec::with_capacity(CHUNK + 64);
    let mut chunk = vec![0u8; CHUNK - 1];
    let mut bytes = 0u64;
    for item in items {
        match item {
            Item::Text(t) => tx.write(&mut w, T_TEXT, t.as_bytes(), &mut buf)?,
            Item::File(path) => {
                let mut f = File::open(path)?;
                let size = f.metadata()?.len();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                tx.write(&mut w, T_FILE, &file_header(size, name, mime_for(path)), &mut buf)?;
                loop {
                    let n = f.read(&mut chunk)?;
                    if n == 0 {
                        break;
                    }
                    tx.write(&mut w, T_DATA, &chunk[..n], &mut buf)?;
                    bytes += n as u64;
                }
                tx.write(&mut w, T_FILE_END, b"", &mut buf)?;
            }
        }
    }
    tx.write(&mut w, T_END, b"", &mut buf)?;
    w.flush()?;
    match rx.read(&mut r, &mut buf)? {
        Some(T_OK) => Ok(Sent { to, bytes, elapsed: t0.elapsed() }),
        Some(T_ERR) => Err(io::Error::other(format!("receiver: {}", String::from_utf8_lossy(&buf[1..])))),
        _ => Err(io::Error::other("receiver closed the connection without confirming")),
    }
}

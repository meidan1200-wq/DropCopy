//! The receiver: answers discovery, accepts transfers, saves files, fills the clipboard.

use crate::clipboard::{self, ClipMode};
use crate::config::Config;
use crate::proto::*;
use crate::util::{human_size, sanitize_filename, unique_path};
use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub struct ServeOpts {
    pub port: u16,
    pub clip: ClipMode,
    /// Answer UDP discovery broadcasts (off only in tests that run side by side).
    pub discovery: bool,
    /// Show the little phone above the taskbar when photos arrive.
    pub popup: bool,
    /// When an unpaired phone asks, show the pairing code above the taskbar.
    pub pairing: bool,
}

/// What one connection delivered.
#[derive(Debug, Default)]
pub struct Received {
    pub files: Vec<PathBuf>,
    pub texts: Vec<String>,
    pub bytes: u64,
    /// A proximity preview: saved to a temporary folder and shown on the taskbar phone only.
    pub nearby: bool,
    /// Delete the files once pasted, or once something else is copied.
    pub delete_after_paste: bool,
    /// The phone's name, if it said (v0.5+); for another PC, its name.
    pub device: Option<String>,
    /// The phone's own id (v2), which stays the same while the app is installed.
    pub phone_id: Option<String>,
    /// Another PC (introduced by a phone) sent this, not a phone: its id.
    pub peer: Option<[u8; 8]>,
    /// The phone asked for what is waiting for it (v2).
    pub fetch: bool,
    /// Items handed to the phone from the outbox.
    pub fetched: usize,
    /// PCs the phone introduced that are new here.
    pub introduced: Vec<String>,
    /// The PCs the phone introduced, before they are saved.
    peers: Vec<crate::peers::Peer>,
}

pub struct Server {
    pub listener: TcpListener,
    pub port: u16,
    cfg: Arc<Config>,
    clip: ClipMode,
    clip_lock: Arc<Mutex<()>>,
    popup: bool,
    active: Arc<std::sync::atomic::AtomicUsize>,
    discovering: bool,
}

impl Server {
    /// Binds TCP and starts the UDP discovery responder. `port` 0 picks any free port.
    pub fn bind(cfg: Arc<Config>, opts: ServeOpts) -> io::Result<Server> {
        let listener = TcpListener::bind(("0.0.0.0", opts.port))?;
        let port = listener.local_addr()?.port();
        let mut discovering = false;
        match UdpSocket::bind(("0.0.0.0", UDP_PORT)) {
            _ if !opts.discovery => {}
            Ok(sock) => {
                discovering = true;
                let cfg = cfg.clone();
                thread::Builder::new()
                    .name("discovery".into())
                    .spawn(move || discovery_loop(sock, &cfg, port, opts.pairing))?;
            }
            Err(e) => say!("  ! discovery port {UDP_PORT} unavailable ({e}); phones will use the last known address"),
        }
        Ok(Server { listener, port, cfg, clip: opts.clip, clip_lock: Arc::new(Mutex::new(())), popup: opts.popup, active: Arc::default(), discovering })
    }

    /// Whether it answers discovery (the UDP port may be taken by something else).
    pub fn discovering(&self) -> bool {
        self.discovering
    }

    pub fn run(self) -> io::Result<()> {
        for conn in self.listener.incoming() {
            let stream = match conn {
                Ok(s) => s,
                Err(e) => {
                    say!("  ! accept failed: {e}");
                    continue;
                }
            };
            use std::sync::atomic::Ordering::SeqCst;
            if self.active.fetch_add(1, SeqCst) >= MAX_CONNECTIONS {
                self.active.fetch_sub(1, SeqCst);
                drop(stream);
                continue;
            }
            let active = self.active.clone();
            let cfg = self.cfg.clone();
            let clip = self.clip;
            let lock = self.clip_lock.clone();
            let popup = self.popup;
            thread::spawn(move || {
                struct Slot(Arc<std::sync::atomic::AtomicUsize>);
                impl Drop for Slot {
                    fn drop(&mut self) {
                        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                let _slot = Slot(active);
                let mut peer = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
                let t0 = Instant::now();
                match handle(stream, &cfg) {
                    Ok(r) => {
                        if r.peer.is_none() {
                            // Only a phone holding the key gets this far: it is paired.
                            cfg.mark_paired();
                            crate::near::heard_from_phone();
                            if let Some(d) = &r.device {
                                crate::config::remember_phone(&cfg.home, d, r.phone_id.as_deref());
                            }
                            crate::popup::hide_code();
                        }
                        for name in &r.introduced {
                            say!("  + {name}: your phone introduced it - you can send to it from the tray or Explorer's Send to");
                        }
                        if r.fetched > 0 {
                            say!("  -> {} item(s) delivered to {}", r.fetched, r.device.as_deref().unwrap_or("the phone"));
                        }
                        if r.nearby {
                            // Brought close: show it on the little phone; nothing touches the clipboard.
                            crate::cleanup::track(&r.files);
                            if popup && !r.files.is_empty() {
                                say!("  <- nearby preview from {peer}");
                                crate::popup::show_nearby(r.files.clone());
                            } else {
                                crate::cleanup::delete_later(r.files.clone(), Duration::ZERO);
                            }
                            return;
                        }
                        if r.delete_after_paste {
                            crate::cleanup::track(&r.files);
                        } else if !r.files.is_empty() {
                            crate::popup::remember(&r.files);
                        }
                        if let Some(d) = &r.device {
                            peer = format!("{d} ({peer})");
                        }
                        if r.fetch && r.files.is_empty() && r.texts.is_empty() {
                            return; // came to collect, not to send
                        }
                        report(&r, &peer, t0, clip, &lock)
                    }
                    // A phone races several connections and closes the ones that lose.
                    Err(e) if e.kind() == io::ErrorKind::ConnectionAborted => {}
                    Err(e) => say!("  x from {peer}: {e}"),
                }
            });
        }
        Ok(())
    }
}

/// What this PC answers a v2 discovery request with.
pub fn status_for(cfg: &Config, port: u16, phone: bool) -> Status {
    let near = crate::near::status();
    let fresh = near.last.filter(|(_, age)| age.as_secs() < 5);
    let dbm = fresh.map(|(s, _)| s.round() as i16).unwrap_or(i16::MIN);
    let mut flags = OK_V2 as u16;
    if phone && dbm != i16::MIN && dbm >= near.threshold {
        flags |= STATUS_CLOSE;
    }
    if phone && crate::outbox::waiting(&cfg.home, None, None) {
        flags |= OK_WAITING as u16;
    }
    let idle = crate::util::idle_seconds().map(|s| s.min(u16::MAX as u64 - 1) as u16).unwrap_or(u16::MAX);
    Status { port, idle_s: idle, near_dbm: if phone { dbm } else { i16::MIN }, flags }
}

/// Between knocks on a listening phone that has something waiting.
const KNOCK_GAP: Duration = Duration::from_secs(10);

fn discovery_loop(sock: UdpSocket, cfg: &Config, port: u16, pairing: bool) {
    let mut buf = [0u8; TOAST_MAX];
    let mut last_shown: Option<Instant> = None;
    let mut last_knock: Option<Instant> = None;
    loop {
        // Windows reports an ICMP "port unreachable" from an earlier send as a recv error.
        let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
        if n == 28 && &buf[..4] == DISCOVER_MAGIC && buf[4..12] == cfg.id {
            let _ = sock.send_to(&reply_packet(&cfg.id, &cfg.key, &buf[12..28], port), from);
        } else if let Some((who, nonce, kf)) = parse_ask(&buf[..n], &cfg.id, &cfg.key) {
            let phone = who == [0u8; 8];
            let _ = sock.send_to(&status_packet(&cfg.id, &kf, &who, &nonce, &status_for(cfg, port, phone)), from);
        } else if is_hello(&buf[..n], &cfg.id, &cfg.key) {
            crate::near::phone_is_at(from);
            if crate::near::phone() == Some(from) {
                crate::outbox::save_phone_at(&cfg.home, from);
            }
            // The phone is listening: if something waits for it, ask it to come and get it now.
            if last_knock.is_none_or(|t| t.elapsed() > KNOCK_GAP) && crate::outbox::waiting(&cfg.home, None, None) {
                last_knock = Some(Instant::now());
                let pkt = offer_packet(&cfg.id, &cfg.key, &random());
                let _ = sock.send_to(&pkt, from);
            }
        } else if n == 28 && &buf[..4] == NEAR_ASK_MAGIC && buf[4..12] == cfg.id {
            let _ = sock.send_to(&near_answer(&cfg.id, &cfg.key, &buf[12..28], &crate::near::status()), from);
        } else if n > 4 && &buf[..4] == TOAST_MAGIC && from.ip().is_loopback() {
            let msg = String::from_utf8_lossy(&buf[4..n]).into_owned();
            let (title, text) = msg.split_once('\0').unwrap_or((&msg, ""));
            crate::tray::toast(title, text);
        } else if n == 4 && &buf[..4] == STOP_MAGIC && from.ip().is_loopback() {
            say!("  stopping: a newer copy is taking over");
            std::process::exit(0);
        } else if n == 12 && &buf[..4] == PAIR_ASK_MAGIC {
            // The phone repeats its question a few times; show the code once per opening.
            let fresh = last_shown.is_none_or(|t| t.elapsed() > PAIR_ASK_QUIET);
            let _ = sock.send_to(&pair_answer(&buf[4..12], &cfg.name), from);
            if fresh {
                last_shown = Some(Instant::now());
                say!("  {} is asking to pair - showing the pairing code on screen", from.ip());
                if pairing {
                    crate::popup::show_code(crate::pair::link(cfg, port));
                }
            }
        }
    }
}

const HELLO_TIMEOUT: Duration = Duration::from_secs(4);
/// Connections handled at once; more are closed straight away, so a flood from the network
/// cannot use up threads and memory. Far above what phones ever need.
const MAX_CONNECTIONS: usize = 256;

/// How long after showing the pairing code further requests only get an answer. Anyone on the
/// network can ask, so this also keeps a noisy device from flashing the code over and over.
const PAIR_ASK_QUIET: Duration = Duration::from_secs(20);

fn report(r: &Received, peer: &str, t0: Instant, clip: ClipMode, lock: &Mutex<()>) {
    let ms = t0.elapsed().as_millis();
    if r.files.is_empty() && r.texts.is_empty() {
        say!("  <- {peer} checked in ({ms} ms) - phone and PC can see each other");
        return;
    }
    for f in &r.files {
        let size = fs::metadata(f).map(|m| m.len()).unwrap_or(0);
        say!("  <- {}  {}  ({peer}, {ms} ms)", f.display(), human_size(size));
    }
    for t in &r.texts {
        let preview: String = t.chars().take(60).collect();
        say!("  <- text: {preview}{}", if t.chars().count() > 60 { "..." } else { "" });
    }
    if clip == ClipMode::Off {
        return;
    }
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    let t1 = Instant::now();
    let result = if let Some(text) = r.texts.last().filter(|_| r.files.is_empty()) {
        clipboard::set_text(text)
    } else if !r.files.is_empty() {
        clipboard::set_files(&r.files, clip, r.delete_after_paste)
    } else {
        Ok(())
    };
    if r.delete_after_paste && !r.files.is_empty() {
        say!("     deleted from this PC once pasted, or when something else is copied");
    }
    match result {
        Ok(()) => say!("     on the clipboard - Ctrl+V to paste ({} ms)", t1.elapsed().as_millis()),
        Err(e) => say!("  ! clipboard: {e} (the file is saved)"),
    }
}

/// Runs one transfer. Partial files are removed if the transfer does not finish.
pub fn handle(stream: TcpStream, cfg: &Config) -> io::Result<Received> {
    stream.set_nodelay(true)?;
    // A phone says hello at once; something that connects and stays silent is let go quickly.
    stream.set_read_timeout(Some(HELLO_TIMEOUT))?;
    let mut w = stream.try_clone()?;
    let mut r = BufReader::with_capacity(256 * 1024, stream);
    let (mut rx, mut tx, peer) = server_handshake_any(&mut r, &mut w, &cfg.key).map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            io::Error::new(io::ErrorKind::ConnectionAborted, "closed before saying hello")
        } else {
            e
        }
    })?;
    r.get_ref().set_read_timeout(Some(Duration::from_secs(30)))?;

    let mut part: Option<(File, PathBuf, String)> = None;
    let result = receive(&mut r, &mut rx, cfg, &mut part);
    let mut buf = Vec::new();
    match result {
        Ok(mut got) => {
            got.peer = peer;
            if let Some(p) = &peer {
                // Another PC: it is named by what the phone told us, else by what it says.
                got.device = crate::peers::name_of(&cfg.home, p).or(got.device.take()).map(|d| format!("{d} (PC)"));
                got.fetch = false;
                got.phone_id = None;
                got.peers.clear();
            }
            if !got.peers.is_empty() {
                got.introduced = crate::peers::merge(&cfg.home, &cfg.id, std::mem::take(&mut got.peers));
            }
            let count = (got.files.len() + got.texts.len()).min(u16::MAX as usize) as u16;
            if got.fetch {
                let mut w = io::BufWriter::with_capacity(CHUNK + 64, w);
                let entries = crate::outbox::pending(&cfg.home, got.device.as_deref(), got.phone_id.as_deref());
                let mut items = vec![];
                for e in &entries {
                    items.extend(e.files.iter().cloned().map(crate::client::Item::File));
                    items.extend(e.texts.iter().cloned().map(crate::client::Item::Text));
                }
                crate::client::write_items(&mut tx, &mut w, &items, &mut buf)?;
                tx.write(&mut w, T_OK, &[&count.to_be_bytes()[..], &[OK_V2]].concat(), &mut buf)?;
                w.flush()?;
                // Removed only once the phone says it has them all; otherwise they wait for next time.
                if !entries.is_empty() {
                    if let Ok(Some(T_OK)) = rx.read(&mut r, &mut buf) {
                        crate::outbox::delivered(&cfg.home, &entries.iter().map(|e| e.id.clone()).collect::<Vec<_>>());
                        got.fetched = items.len();
                    }
                }
            } else {
                let waiting = peer.is_none() && crate::outbox::waiting(&cfg.home, got.device.as_deref(), got.phone_id.as_deref());
                let flags = OK_V2 | if waiting { OK_WAITING } else { 0 };
                tx.write(&mut w, T_OK, &[&count.to_be_bytes()[..], &[flags]].concat(), &mut buf)?;
                w.flush()?;
            }
            Ok(got)
        }
        Err(e) => {
            if let Some((f, p, _)) = part.take() {
                drop(f);
                let _ = fs::remove_file(p);
            }
            let _ = tx.write(&mut w, T_ERR, e.to_string().as_bytes(), &mut buf);
            Err(e)
        }
    }
}

fn receive(
    r: &mut BufReader<TcpStream>,
    rx: &mut Records,
    cfg: &Config,
    part: &mut Option<(File, PathBuf, String)>,
) -> io::Result<Received> {
    let mut got = Received::default();
    let mut buf = Vec::with_capacity(CHUNK + 64);
    let proto_err = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    loop {
        let Some(typ) = rx.read(r, &mut buf)? else {
            return Err(proto_err("connection closed before the transfer finished (wrong pairing key, or the phone gave up)"));
        };
        let body = &buf[1..];
        match typ {
            T_FILE => {
                if part.is_some() {
                    return Err(proto_err("new file before the previous one ended"));
                }
                let h = parse_file_header(body)?;
                let dir = if got.nearby { crate::cleanup::nearby_dir(&cfg.home) } else { cfg.save_dir.clone() };
                fs::create_dir_all(&dir)?;
                let mut name = sanitize_filename(&h.name);
                if std::path::Path::new(&name).extension().is_none() {
                    if let Some(ext) = crate::util::ext_for_mime(&h.mime) {
                        name = format!("{name}.{ext}");
                    }
                }
                let tmp = dir.join(format!(".{name}.{}.part", crate::util::hex(&random::<4>())));
                *part = Some((File::create(&tmp)?, tmp, name));
            }
            T_DATA => {
                let (f, _, _) = part.as_mut().ok_or_else(|| proto_err("data outside a file"))?;
                f.write_all(body)?;
                got.bytes += body.len() as u64;
            }
            T_FILE_END => {
                let (f, tmp, name) = part.take().ok_or_else(|| proto_err("file end outside a file"))?;
                drop(f);
                let dest = unique_path(tmp.parent().unwrap_or(&cfg.save_dir), &name);
                fs::rename(&tmp, &dest)?;
                got.files.push(std::path::absolute(&dest).unwrap_or(dest));
            }
            T_TEXT => got.texts.push(String::from_utf8_lossy(body).into_owned()),
            T_DEVICE => {
                let name: String = String::from_utf8_lossy(body).chars().filter(|c| !c.is_control()).take(64).collect();
                got.device = Some(name.trim().to_string()).filter(|n| !n.is_empty());
            }
            T_FETCH => got.fetch = true,
            T_PHONE => {
                let id: String = String::from_utf8_lossy(body).chars().filter(char::is_ascii_alphanumeric).take(32).collect();
                got.phone_id = Some(id).filter(|i| !i.is_empty());
            }
            T_PEERS => got.peers = crate::peers::parse_list(body),
            T_OPTS => {
                let flags = body.first().copied().unwrap_or(0);
                got.nearby = flags & OPT_NEARBY != 0;
                got.delete_after_paste = flags & OPT_DELETE_AFTER_PASTE != 0;
            }
            T_END => {
                if part.is_some() {
                    return Err(proto_err("transfer ended inside a file"));
                }
                return Ok(got);
            }
            other => return Err(proto_err(&format!("unknown record type {other:#x}"))),
        }
    }
}

/// For callers that want the bound address (tests, `--port 0`).
pub fn local_addr(s: &Server) -> SocketAddr {
    s.listener.local_addr().expect("bound listener has an address")
}

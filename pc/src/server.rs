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
    /// The phone's name, if it said (v0.5+).
    pub device: Option<String>,
}

pub struct Server {
    pub listener: TcpListener,
    pub port: u16,
    cfg: Arc<Config>,
    clip: ClipMode,
    clip_lock: Arc<Mutex<()>>,
    popup: bool,
    active: Arc<std::sync::atomic::AtomicUsize>,
}

impl Server {
    /// Binds TCP and starts the UDP discovery responder. `port` 0 picks any free port.
    pub fn bind(cfg: Arc<Config>, opts: ServeOpts) -> io::Result<Server> {
        let listener = TcpListener::bind(("0.0.0.0", opts.port))?;
        let port = listener.local_addr()?.port();
        match UdpSocket::bind(("0.0.0.0", UDP_PORT)) {
            _ if !opts.discovery => {}
            Ok(sock) => {
                let cfg = cfg.clone();
                thread::Builder::new()
                    .name("discovery".into())
                    .spawn(move || discovery_loop(sock, &cfg, port, opts.pairing))?;
            }
            Err(e) => say!("  ! discovery port {UDP_PORT} unavailable ({e}); phones will use the last known address"),
        }
        Ok(Server { listener, port, cfg, clip: opts.clip, clip_lock: Arc::new(Mutex::new(())), popup: opts.popup, active: Arc::default() })
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
                        // Only a phone holding the key gets this far: it is paired.
                        cfg.mark_paired();
                        if let Some(d) = &r.device {
                            crate::config::remember_phone(&cfg.home, d);
                        }
                        crate::popup::hide_code();
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

fn discovery_loop(sock: UdpSocket, cfg: &Config, port: u16, pairing: bool) {
    let mut buf = [0u8; 64];
    let mut last_shown: Option<Instant> = None;
    loop {
        // Windows reports an ICMP "port unreachable" from an earlier send as a recv error.
        let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
        if n == 28 && &buf[..4] == DISCOVER_MAGIC && buf[4..12] == cfg.id {
            let _ = sock.send_to(&reply_packet(&cfg.id, &cfg.key, &buf[12..28], port), from);
        } else if is_hello(&buf[..n], &cfg.id, &cfg.key) {
            crate::near::phone_is_at(from);
        } else if n == 28 && &buf[..4] == NEAR_ASK_MAGIC && buf[4..12] == cfg.id {
            let _ = sock.send_to(&near_answer(&cfg.id, &cfg.key, &buf[12..28], &crate::near::status()), from);
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
    let (mut rx, mut tx) = server_handshake(&mut r, &mut w, &cfg.key).map_err(|e| {
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
        Ok(got) => {
            let count = (got.files.len() + got.texts.len()).min(u16::MAX as usize) as u16;
            tx.write(&mut w, T_OK, &count.to_be_bytes(), &mut buf)?;
            w.flush()?;
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

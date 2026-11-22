#![cfg_attr(all(windows, feature = "gui"), windows_subsystem = "windows")]
use dropcopy::client::{self, Item};
use dropcopy::clipboard::ClipMode;
use dropcopy::config::{default_home, Config};
use dropcopy::proto::TCP_PORT;
use dropcopy::server::{ServeOpts, Server};
use dropcopy::util::human_size;
use dropcopy::{pair, say};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

const USAGE: &str = "\
DropCopy - share on your phone, Ctrl+V on this PC. Local network only, no cloud.

USAGE
  dropcopy                      Windows: make sure it runs in the background (and starts
                                with Windows), then show pairing and live activity here.
                                Closing the window does not stop it.
  dropcopy serve                receive in this window only (stops when it closes)
  dropcopy pair [--new] [--png] [--invert]
                                show the pairing QR code
                                  --new     new key: every paired phone must scan again
                                  --png     also save it as an image and open it
                                  --invert  for terminals with a light background
  dropcopy share [--to NAME] [--text TEXT] [FILE...]
                                send to your phone or another PC (Explorer: right-click >
                                Send to > DropCopy); no FILE or text: the clipboard
  dropcopy send [--to HOST[:PORT]] [--text TEXT] [FILE...]
                                send to a DropCopy receiver by address (testing)
  dropcopy dir [PATH]           show or change where received files are saved
  dropcopy name [NAME]          show or change the name phones see
  dropcopy setup                Windows: run in the background, start with Windows
  dropcopy unsetup              stop it and undo setup
  dropcopy set [SETTING VALUE]  popup, proximity, updates (run alone to list them)
  dropcopy near                 live Bluetooth readings, to tune how close counts as close
  dropcopy update               install the newest version now
  dropcopy doctor               check network, drag and drop, updates
  dropcopy version

OPTIONS
  --clip both|image|file|off    what goes on the clipboard (default: both)
  --port N                      TCP port (default 48650)
  --home DIR                    settings folder (default: per-user app data)
";

struct Args {
    cmd: Option<String>,
    rest: Vec<String>,
    home: Option<PathBuf>,
    port: u16,
    clip: ClipMode,
    to: Option<String>,
    texts: Vec<String>,
    new_key: bool,
    png: bool,
    invert: bool,
    hidden: bool,
    background: bool,
    /// `send`: T_OPTS flags, for trying the phone's options from a terminal (`--opts 2`).
    opts: Option<u8>,
    /// `send`: introduce this sender by name, like a phone does (`--as "Galaxy S20+"`).
    device: Option<String>,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        cmd: None,
        rest: vec![],
        home: None,
        port: TCP_PORT,
        clip: ClipMode::Both,
        to: None,
        texts: vec![],
        new_key: false,
        png: false,
        invert: false,
        hidden: false,
        background: false,
        opts: None,
        device: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" | "help" => return Err(String::new()),
            "--home" => a.home = Some(PathBuf::from(value("--home")?)),
            "--port" => a.port = value("--port")?.parse().map_err(|_| "--port needs a number")?,
            "--clip" => a.clip = ClipMode::parse(&value("--clip")?).ok_or("--clip is both, image, file or off")?,
            "--no-clipboard" => a.clip = ClipMode::Off,
            "--to" => a.to = Some(value("--to")?),
            "--text" => a.texts.push(value("--text")?),
            "--new" => a.new_key = true,
            "--png" => a.png = true,
            "--invert" => a.invert = true,
            "--hidden" => a.hidden = true,
            "--as" => a.device = Some(value("--as")?),
            "--opts" => a.opts = Some(value("--opts")?.parse().map_err(|_| "--opts needs a number")?),
            "--background" => a.background = true,
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ if a.cmd.is_none() => a.cmd = Some(arg),
            _ => a.rest.push(arg),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    setup::attach_terminal();
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}\n");
            }
            eprint!("{USAGE}");
            return ExitCode::from(if e.is_empty() { 0 } else { 2 });
        }
    };
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(a: Args) -> Result<(), String> {
    let home = a.home.clone().unwrap_or_else(default_home);
    let (mut cfg, _) = Config::load_or_create(home).map_err(|e| format!("settings: {e}"))?;
    // Downloaded from the phone (it appended its pairing to the file): take its key, no scanning.
    let from_phone = std::env::current_exe().map(|exe| dropcopy::handoff::adopt(&mut cfg, &exe)).unwrap_or(false);
    if from_phone {
        say!("  Set up from your phone: it connects by itself in a moment.");
    }
    match a.cmd.as_deref() {
        None if cfg!(windows) && !a.hidden && !a.background => launch(cfg, &a, from_phone),
        None | Some("serve") | Some("receive") => serve(cfg, &a),
        Some("pair") => {
            if a.new_key {
                cfg.rekey().map_err(|e| e.to_string())?;
                say!("  New pairing key. Phones paired before must scan this code again.");
            }
            pair::print_pairing(&cfg, a.port, a.invert);
            if a.png {
                let p = pair::png_qr(&pair::link(&cfg, a.port), &cfg.home)?;
                say!("  Saved {}", p.display());
                open_file(&p);
            }
            Ok(())
        }
        Some("send") => send(&mut cfg, &a),
        Some("share") => share(&cfg, &a),
        Some("dir") => {
            if let Some(p) = a.rest.first() {
                let p = std::path::absolute(p).map_err(|e| e.to_string())?;
                std::fs::create_dir_all(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                cfg.save_dir = p;
                cfg.save().map_err(|e| e.to_string())?;
            }
            say!("{}", cfg.save_dir.display());
            Ok(())
        }
        Some("name") => {
            if !a.rest.is_empty() {
                cfg.name = a.rest.join(" ");
                cfg.save().map_err(|e| e.to_string())?;
                say!("  Renamed. Re-pair (dropcopy pair) for phones to show the new name.");
            }
            say!("{}", cfg.name);
            Ok(())
        }
        Some("setup") => setup::install(),
        Some("update") => update_now(),
        Some("near") => near_diagnostic(&cfg),
        Some("doctor") => doctor(&cfg),
        Some("version") | Some("--version") => {
            say!("dropcopy {}", dropcopy::update::current());
            Ok(())
        }
        Some("set") => match a.rest.as_slice() {
            [k, v @ ..] if !v.is_empty() => {
                cfg.set(k, &v.join(" "))?;
                say!("  {k} = {}  (restart DropCopy to apply: dropcopy setup)", v.join(" "));
                Ok(())
            }
            _ => {
                say!("  dropcopy set SETTING VALUE\n");
                for (k, d) in dropcopy::config::SETTINGS {
                    say!("  {k:<12} {d}");
                }
                Ok(())
            }
        },
        Some("unsetup") => setup::uninstall(),
        Some(other) => Err(format!("unknown command {other} (try --help)")),
    }
}

/// What a double-click does: the background copy does the receiving, so this window is only a
/// view of it, and closing it changes nothing.
fn launch(cfg: Config, a: &Args, from_phone: bool) -> Result<(), String> {
    say!("DropCopy v{} - share on your phone, Ctrl+V on this PC.\n", dropcopy::update::current());
    let fresh_log = match setup::ensure_background(&cfg, from_phone) {
        Ok(setup::Bg::Running) => {
            say!("  Running in the background. It starts with Windows, and closing this window doesn't stop it.");
            false
        }
        Ok(setup::Bg::Started) => {
            say!("  Set up: DropCopy now runs in the background and starts with Windows.");
            say!("  Closing this window doesn't stop it. (To remove it: dropcopy unsetup)");
            true
        }
        Err(e) => {
            say!("  ! couldn't start in the background ({e}); receiving in this window instead.\n");
            return serve(cfg, a);
        }
    };
    // Double-clicked (this console is ours alone): the setup window instead of the terminal.
    if setup::own_console() {
        let log = cfg.home.join("log.txt");
        let start = std::fs::metadata(&log).map(|m| m.len()).unwrap_or(0);
        // Set up from the phone: it holds the key already, so no code; just wait for it.
        let paired = cfg.is_paired() || from_phone;
        let home = cfg.home.clone();
        setup::hide_console();
        dropcopy::setup_window::run(pair::link(&cfg, a.port), paired, move || {
            // A phone got through: the log gained a "<-" line since the window opened.
            let text = std::fs::read(&log).unwrap_or_default();
            let new = if (text.len() as u64) >= start { &text[start as usize..] } else { &text[..] };
            let got = String::from_utf8_lossy(new).lines().any(|l| l.starts_with("  <-")) || (!paired && home.join("paired").exists());
            got.then(|| dropcopy::config::last_phone(&home).unwrap_or_default())
        });
        return Ok(());
    }
    say!("  Saving to {}", cfg.save_dir.display());
    let pairing = !cfg.is_paired();
    if pairing {
        pair::print_pairing(&cfg, a.port, a.invert);
    } else {
        say!("  Your phone is paired. Reinstalled the app? Just open it on the phone:");
        say!("  the pairing code pops up above the taskbar.");
    }
    say!("\n  Activity (live):");
    tail_log(&cfg.home.join("log.txt"), fresh_log, pairing)
}

/// Follows the background copy's log, leaving out its start-up banner.
fn tail_log(path: &std::path::Path, from_start: bool, pairing: bool) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom};
    const BANNER: [&str; 5] = ["DropCopy - receiving", "  Saving to", "  Pair another", "  Waiting for", "  Pair your phone"];
    let mut pos = if from_start { 0 } else { std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) };
    let mut partial = String::new();
    let mut announced = !pairing;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let Ok(mut f) = std::fs::File::open(path) else { continue };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < pos {
            pos = 0; // restarted (e.g. after an update): a new log
            partial.clear();
        }
        if len == pos || f.seek(SeekFrom::Start(pos)).is_err() {
            continue;
        }
        let mut chunk = Vec::new();
        if f.read_to_end(&mut chunk).is_err() {
            continue;
        }
        pos += chunk.len() as u64;
        partial.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(i) = partial.find('\n') {
            let line: String = partial.drain(..=i).collect();
            let line = line.trim_end();
            if line.is_empty() || BANNER.iter().any(|b| line.starts_with(b)) {
                continue;
            }
            say!("{line}");
            if !announced && line.starts_with("  <-") {
                announced = true;
                say!("\n  Paired. You can close this window - DropCopy keeps running.\n");
            }
        }
    }
}

fn serve(cfg: Config, a: &Args) -> Result<(), String> {
    if a.hidden && setup::detach() {
        return Ok(());
    }
    if a.background {
        dropcopy::log_to(&cfg.home);
        // Copies that updated themselves never ran setup again: add Explorer's "Send to" entry here.
        if cfg!(windows) && dropcopy::sendto::sendto_dir().is_some_and(|d| !d.join("DropCopy.lnk").exists()) {
            if let Ok(exe) = std::env::current_exe() {
                let _ = dropcopy::sendto::install_shortcut(&exe);
            }
        }
    }
    let cfg = Arc::new(cfg);
    dropcopy::update::cleanup();
    dropcopy::cleanup::init(&cfg.home);
    let opts = || ServeOpts { port: a.port, clip: a.clip, discovery: true, popup: cfg.popup, pairing: cfg.popup };
    let mut bound = Server::bind(cfg.clone(), opts());
    // Just restarted by an update: the old copy may still be letting go of the port.
    for _ in 0..40 {
        match &bound {
            Err(e) if a.background && e.kind() == std::io::ErrorKind::AddrInUse => {
                std::thread::sleep(std::time::Duration::from_millis(250));
                bound = Server::bind(cfg.clone(), opts());
            }
            _ => break,
        }
    }
    let server = bound.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            format!("port {} is in use - DropCopy is probably already running", a.port)
        } else {
            format!("cannot listen on port {}: {e}", a.port)
        }
    })?;
    say!("DropCopy - receiving as \"{}\" on port {}", cfg.name, server.port);
    say!("  Saving to {}", cfg.save_dir.display());
    if a.background {
        // Nobody reads a QR code in a log; an unpaired phone asks, and the code pops up.
    } else if !cfg.is_paired() {
        pair::print_pairing(&cfg, server.port, a.invert);
    } else {
        say!("  Pair another phone: dropcopy pair (or just open the app on it)");
    }
    say!("  Waiting for your phone... (Ctrl+C to stop)\n");
    setup::warn_if_public_network();
    dropcopy::tray::start(dropcopy::tray::Info {
        name: cfg.name.clone(),
        save_dir: cfg.save_dir.clone(),
        home: cfg.home.clone(),
        link: pair::link(&cfg, server.port),
        update: update_from_tray,
        config: (*cfg).clone(),
    });
    if cfg.near && cfg.popup {
        let me = cfg.clone();
        dropcopy::near::watch(cfg.key, cfg.near_rssi, move |r| {
            let cfg = &me;
            if r.came_near {
                say!("  phone nearby ({:.0} dBm)", r.smoothed);
                // It sends its newest photo, which then shows on the taskbar phone. A phone too old
                // to answer gets the last photo it sent instead.
                if !dropcopy::near::pull(&cfg.id, &cfg.key) {
                    if let Some(files) = dropcopy::popup::latest() {
                        dropcopy::popup::show(files, "Nearby");
                    }
                }
            }
        });
    }
    start_updater(a.background && cfg.auto_update);
    server.run().map_err(|e| e.to_string())
}

/// The background copy installs new releases by itself; an interactive window just mentions them.
fn start_updater(install: bool) {
    use dropcopy::update;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(if install { 45 } else { 5 }));
        loop {
            match update::check() {
                Ok(Some(rel)) if install => match std::env::current_exe().map_err(|e| e.to_string()).and_then(|exe| update::apply(&exe, &rel).map(|v| (exe, v))) {
                    Ok((exe, v)) => {
                        say!("  updated to v{v}; restarting");
                        if setup::restart(&exe) {
                            std::process::exit(0);
                        }
                    }
                    Err(e) => say!("  ! update to v{} failed: {e}", rel.version),
                },
                Ok(Some(rel)) => {
                    say!("  A new version is available: v{} (you have v{}). Run `dropcopy update`.", rel.version, update::current());
                    return;
                }
                _ => {}
            }
            if !install {
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs(6 * 3600));
        }
    });
}

/// The tray's "Check for updates": install the newest release now and start it.
fn update_from_tray() -> Result<Option<String>, String> {
    use dropcopy::update;
    let Some(rel) = update::check()? else { return Ok(None) };
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let v = update::apply(&exe, &rel)?;
    if !setup::restart(&exe) {
        return Err(format!("v{v} is installed and starts the next time you sign in"));
    }
    Ok(Some(v.to_string()))
}

fn update_now() -> Result<(), String> {
    use dropcopy::update;
    let Some(rel) = update::check()? else {
        say!("  DropCopy v{} is up to date.", update::current());
        return Ok(());
    };
    say!("  Downloading v{}...", rel.version);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let v = update::apply(&exe, &rel)?;
    say!("  Updated to v{v}.");
    setup::refresh_background(&exe);
    Ok(())
}

/// Checks the pieces a first run depends on and says plainly what is wrong.
fn doctor(cfg: &Config) -> Result<(), String> {
    let ok = |b: bool| if b { "ok " } else { "!! " };
    say!("  DropCopy v{}  ({})", dropcopy::update::current(), std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default());
    let ip = dropcopy::pair::primary_ipv4();
    say!("  {} network: {}", ok(ip.is_some()), ip.map(|i| i.to_string()).unwrap_or("no network right now".into()));
    let port_free = std::net::TcpListener::bind(("0.0.0.0", dropcopy::proto::TCP_PORT)).is_ok();
    say!("  {} receiver: {}", "   ", if port_free { "not running" } else { "running (port in use)" });
    std::fs::create_dir_all(&cfg.save_dir).map_err(|e| e.to_string())?;
    let probe = cfg.save_dir.join(".dropcopy-doctor.png");
    image::RgbaImage::from_pixel(4, 8, image::Rgba([40, 80, 160, 255])).save(&probe).map_err(|e| e.to_string())?;
    match dropcopy::popup::check_drag_data(std::slice::from_ref(&probe)) {
        Ok(n) => say!("  {} drag and drop: a drop would deliver {n} file", ok(n == 1)),
        Err(e) => say!("  !!  drag and drop: {e}"),
    }
    let _ = std::fs::remove_file(probe);
    say!("  {} popup: {}   proximity: {} (close at {} dBm)   auto-update: {}", "   ",
        if cfg.popup { "on" } else { "off" }, if cfg.near { "on" } else { "off" }, cfg.near_rssi, if cfg.auto_update { "on" } else { "off" });
    match dropcopy::update::check() {
        Ok(Some(r)) => say!("  {} updates: v{} available (dropcopy update)", "   ", r.version),
        Ok(None) => say!("  ok  updates: up to date"),
        Err(e) => say!("  !!  updates: {e}"),
    }
    Ok(())
}

/// Live Bluetooth readings, to see how strong "close" is on this PC.
fn near_diagnostic(cfg: &Config) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Bluetooth proximity is only available on Windows".into());
    }
    say!("  Watching for your phone over Bluetooth (turn on \"Show on PC when near\" in the app).");
    say!("  Move it towards the laptop and away. It counts as close at {} dBm. Ctrl+C to stop.\n", cfg.near_rssi);
    dropcopy::near::watch(cfg.key, cfg.near_rssi, |r| {
        let bar = "#".repeat(((r.smoothed + 100.0).max(0.0) / 2.0) as usize);
        say!("  {:>4} dBm  smoothed {:>6.1}  {bar}{}", r.rssi, r.smoothed, if r.came_near { "  <- close" } else { "" });
    });
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn send(cfg: &mut Config, a: &Args) -> Result<(), String> {
    let mut items: Vec<Item> = a.device.clone().map(Item::Device).into_iter().collect();
    items.extend(a.opts.map(Item::Options));
    items.extend(a.texts.iter().cloned().map(Item::Text));
    for f in &a.rest {
        let p = PathBuf::from(f);
        if !p.is_file() {
            return Err(format!("{f}: not a file"));
        }
        items.push(Item::File(p));
    }
    if items.is_empty() {
        return Err("nothing to send (give files or --text)".into());
    }
    let sent = client::send(cfg, a.to.as_deref(), &items).map_err(|e| e.to_string())?;
    let ms = sent.elapsed.as_secs_f64() * 1000.0;
    let rate = sent.bytes as f64 / sent.elapsed.as_secs_f64().max(1e-6) / 1048576.0;
    say!("  -> {} item(s), {} to {} in {ms:.0} ms ({rate:.0} MB/s)", items.len(), human_size(sent.bytes), sent.to);
    let peer = sent.to.to_string();
    if cfg.last_peer.as_deref() != Some(peer.as_str()) {
        cfg.last_peer = Some(peer);
        let _ = cfg.save();
    }
    Ok(())
}

/// Explorer's "Send to > DropCopy" (and `dropcopy share` in a terminal): pick a phone or PC,
/// send, and say how it went in a Windows notification.
fn share(cfg: &Config, a: &Args) -> Result<(), String> {
    use dropcopy::sendto::{self, What};
    let mut what = What::files(a.rest.iter().map(PathBuf::from).filter(|p| p.is_file()).collect());
    if !a.texts.is_empty() {
        what.text = Some(a.texts.join("\n"));
    }
    if what.files.is_empty() && what.text.is_none() {
        if !a.rest.is_empty() {
            let msg = "Folders can't be sent yet - pick the files inside";
            dropcopy::tray::toast_from_anywhere("Nothing sent", msg);
            return Err(msg.into());
        }
        what = What::clipboard(cfg)?;
    }
    let targets = sendto::targets(cfg);
    if targets.is_empty() {
        let msg = "Pair your phone first: open DropCopy on it. PCs appear here once your phone has met them both.";
        dropcopy::tray::toast_from_anywhere("Nowhere to send yet", msg);
        return Err(msg.into());
    }
    let chosen = match &a.to {
        Some(name) => targets.iter().position(|t| t.name().eq_ignore_ascii_case(name)).ok_or_else(|| format!("no phone or PC called {name}"))?,
        None => match sendto::pick("Send with DropCopy", &what.describe(), &targets) {
            Some(i) => i,
            None => return Ok(()),
        },
    };
    let out = sendto::deliver(cfg, &targets[chosen], what);
    say!("  {}: {}", out.title, out.text);
    dropcopy::tray::toast_from_anywhere(&out.title, &out.text);
    if out.ok { Ok(()) } else { Err(out.text) }
}

fn open_file(p: &std::path::Path) {
    use std::process::Command;
    let _ = if cfg!(windows) {
        Command::new("cmd").args(["/C", "start", ""]).arg(p).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(p).spawn()
    } else {
        Command::new("xdg-open").arg(p).spawn()
    };
}

#[cfg(windows)]
mod setup {
    use dropcopy::say;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

    /// Restarts this command without a console window. Returns true in the parent, which should exit.
    pub fn detach() -> bool {
        let Ok(exe) = std::env::current_exe() else { return false };
        let mut args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--hidden").collect();
        args.push("--background".into());
        spawn_hidden(&exe.display().to_string(), &args)
    }

    fn spawn_hidden(exe: &str, args: &[String]) -> bool {
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        Command::new(exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
            .spawn()
            .is_ok()
    }

    fn quiet(cmd: &mut Command) -> bool {
        cmd.stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
    }

    /// Where the autostarted copy lives, so startup doesn't depend on the Downloads folder.
    fn installed_exe() -> Option<std::path::PathBuf> {
        let base = std::env::var_os("LOCALAPPDATA")?;
        Some(std::path::PathBuf::from(base).join("DropCopy").join("dropcopy.exe"))
    }

    /// Every other DropCopy process: any version, from any folder, under any download name
    /// ("dropcopy (1).exe"), receiving or just showing a window. Never this one.
    fn other_copies() -> Vec<u32> {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
        let me = std::process::id();
        let mut out = vec![];
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return out;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snap, &mut e) != 0;
            while more {
                let len = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
                let name = String::from_utf16_lossy(&e.szExeFile[..len]);
                if e.th32ProcessID != me && dropcopy::util::is_dropcopy_exe(&name) {
                    out.push(e.th32ProcessID);
                }
                more = Process32NextW(snap, &mut e) != 0;
            }
            CloseHandle(snap);
        }
        out
    }

    /// Stops every other copy, so an update never leaves two running. Asks first (a receiver
    /// quits on its own), then ends whatever is left, older versions included.
    fn stop_background() {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::*;
        if let Ok(s) = std::net::UdpSocket::bind("127.0.0.1:0") {
            let _ = s.send_to(dropcopy::proto::STOP_MAGIC, ("127.0.0.1", dropcopy::proto::UDP_PORT));
        }
        for _ in 0..10 {
            if other_copies().is_empty() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        for pid in other_copies() {
            unsafe {
                let h = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid);
                if !h.is_null() {
                    TerminateProcess(h, 0);
                    WaitForSingleObject(h, 3000); // until its files and port are free
                    CloseHandle(h);
                }
            }
        }
    }

    /// One setup at a time, even if the exe is double-clicked twice.
    fn setup_lock() -> Option<windows_sys::Win32::Foundation::HANDLE> {
        use windows_sys::Win32::System::Threading::*;
        let name: Vec<u16> = "Local\\DropCopySetup\0".encode_utf16().collect();
        unsafe {
            let m = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if m.is_null() {
                return None;
            }
            WaitForSingleObject(m, 30_000);
            Some(m)
        }
    }

    static FROM_TERMINAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// The app build has no console of its own. Started from a terminal, it borrows that terminal's
    /// so commands still print there; double-clicked, there is none and nothing flashes.
    pub fn attach_terminal() {
        if !cfg!(feature = "gui") {
            return;
        }
        use windows_sys::Win32::Storage::FileSystem::*;
        use windows_sys::Win32::System::Console::*;
        unsafe {
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                return;
            }
            FROM_TERMINAL.store(true, std::sync::atomic::Ordering::SeqCst);
            let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
            let h = CreateFileW(name.as_ptr(), 0x4000_0000 | 0x8000_0000, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut());
            if !h.is_null() && h != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
                SetStdHandle(STD_OUTPUT_HANDLE, h);
                SetStdHandle(STD_ERROR_HANDLE, h);
            }
            // The terminal printed its prompt already (it doesn't wait for an app build): start on a fresh line.
            println!();
        }
    }

    /// True when started by a double-click rather than from a terminal.
    pub fn own_console() -> bool {
        if cfg!(feature = "gui") {
            return !FROM_TERMINAL.load(std::sync::atomic::Ordering::SeqCst);
        }
        use windows_sys::Win32::System::Console::GetConsoleProcessList;
        let mut ids = [0u32; 4];
        unsafe { GetConsoleProcessList(ids.as_mut_ptr(), 4) == 1 }
    }

    pub fn hide_console() {
        unsafe { windows_sys::Win32::System::Console::FreeConsole() };
    }

    pub enum Bg {
        /// Already running, and at least as new as this copy.
        Running,
        /// Installed or updated from this copy, and started.
        Started,
    }

    /// The version of the installed copy, by asking it.
    fn installed_version(exe: &std::path::Path) -> Option<dropcopy::update::Version> {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = Command::new(exe).arg("version").creation_flags(CREATE_NO_WINDOW).stderr(Stdio::null()).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        dropcopy::update::Version::parse(text.trim().strip_prefix("dropcopy ")?)
    }

    fn registered() -> bool {
        quiet(Command::new("reg").args(["query", RUN_KEY, "/v", "DropCopy"]))
    }

    fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
        a == b || matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
    }

    /// Makes sure the background copy is installed, current and running, without a second copy
    /// ever fighting over the port. Never replaces a newer installed copy with an older download.
    /// `restart`: the key just changed (set up from the phone), so a running copy must start over.
    pub fn ensure_background(cfg: &dropcopy::config::Config, restart: bool) -> Result<Bg, String> {
        let current = std::env::current_exe().map_err(|e| e.to_string())?;
        let target = installed_exe().ok_or("LOCALAPPDATA is not set")?;
        // A discovery probe, not a test bind: binding would make Windows' firewall ask about this copy.
        let running = dropcopy::client::discover(cfg, std::time::Duration::from_millis(700)).is_ok();
        let installed = target.exists().then(|| installed_version(&target)).flatten();
        if let Some(v) = installed.filter(|v| *v > dropcopy::update::current()) {
            if running && !restart {
                return Ok(Bg::Running);
            }
            if running {
                stop_background(); // it still holds the old key
            }
            say!("  (v{v} is installed; starting that one)");
            return if spawn_hidden(&target.display().to_string(), &["--background".to_string()]) { Ok(Bg::Started) } else { Err("could not start it".into()) };
        }
        if running && !restart && registered() && same_file(&current, &target) {
            return Ok(Bg::Running);
        }
        install_with(false)?;
        Ok(Bg::Started)
    }

    pub fn install() -> Result<(), String> {
        install_with(true)
    }

    fn install_with(verbose: bool) -> Result<(), String> {
        let _lock = setup_lock(); // released when this process ends
        let current = std::env::current_exe().map_err(|e| e.to_string())?;
        let target = installed_exe().ok_or("LOCALAPPDATA is not set")?;
        stop_background();
        std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        if current != target {
            // Copy beside it, then swap names: works even if the old copy hasn't quite let go.
            let tmp = target.with_file_name("dropcopy.new.exe");
            std::fs::copy(&current, &tmp).map_err(|e| format!("copying to {}: {e}", tmp.display()))?;
            if target.exists() {
                dropcopy::update::swap(&target, &tmp)?;
            } else {
                std::fs::rename(&tmp, &target).map_err(|e| format!("installing to {}: {e}", target.display()))?;
            }
        }
        let exe = target.display().to_string();
        let value = format!("\"{exe}\" --hidden");
        if !quiet(Command::new("reg").args(["add", RUN_KEY, "/v", "DropCopy", "/t", "REG_SZ", "/d", &value, "/f"])) {
            return Err("could not add DropCopy to startup".into());
        }
        // Right-click a file > Send to > DropCopy: to the phone or another PC.
        if let Err(e) = dropcopy::sendto::install_shortcut(&target) {
            say!("  ! Send to shortcut: {e}");
        }
        if verbose {
            say!("  Installed to {exe}");
            say!("  It starts hidden every time you sign in, and waits quietly when there is no network.");
        }
        // Transfers are authenticated with the pairing key, so allowing every profile is safe and
        // keeps it working when Windows labels a home Wi-Fi "Public".
        let rule = format!(
            "advfirewall firewall add rule name=DropCopy dir=in action=allow enable=yes profile=any program=\"{exe}\""
        );
        // The path never changes, so a rule made once (as administrator) stays right.
        if quiet(Command::new("netsh").raw_arg("advfirewall firewall show rule name=DropCopy"))
            || quiet(Command::new("netsh").raw_arg(&rule))
        {
            if verbose {
                say!("  Firewall: allowed.");
            }
        } else {
            say!("  Firewall: Windows will ask once when it starts - click Allow (tick both boxes).");
            say!("  (Or run `dropcopy setup` from an administrator terminal to skip the prompt.)");
        }
        if !spawn_hidden(&exe, &["--background".to_string()]) {
            return Err(format!("could not start {exe}"));
        }
        if verbose {
            say!("  Running now in the background. You can close this window.");
            say!("  Log: %APPDATA%\\DropCopy\\log.txt   Stop and remove: dropcopy unsetup");
        }
        Ok(())
    }

    /// Windows blocks inbound connections on networks marked Public, so the phone just times out.
    /// Checked in the background so startup is not delayed.
    pub fn warn_if_public_network() {
        std::thread::spawn(|| {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let out = Command::new("powershell")
                .args(["-NoProfile", "-Command", "(Get-NetConnectionProfile | Where-Object NetworkCategory -eq 'Public').Name"])
                .creation_flags(CREATE_NO_WINDOW)
                .stderr(Stdio::null())
                .output();
            if let Ok(o) = out {
                let names = String::from_utf8_lossy(&o.stdout).trim().replace("\r\n", ", ");
                if !names.is_empty() {
                    say!("  Note: Windows calls \"{names}\" a Public network. If your phone can't connect, either run");
                    say!("  `dropcopy setup`, or set Settings > Network & internet > Wi-Fi > {names} > Private.\n");
                }
            }
        });
    }

    /// Starts the freshly updated copy; it waits for this one to release the port.
    pub fn restart(exe: &std::path::Path) -> bool {
        spawn_hidden(&exe.display().to_string(), &["--background".to_string()])
    }

    /// After `dropcopy update`: bring the installed background copy to the same version.
    pub fn refresh_background(updated: &std::path::Path) {
        let Some(installed) = installed_exe() else { return };
        if !installed.exists() {
            return;
        }
        stop_background();
        if installed != updated {
            let tmp = installed.with_file_name("dropcopy.new.exe");
            if std::fs::copy(updated, &tmp).is_err() || dropcopy::update::swap(&installed, &tmp).is_err() {
                say!("  ! could not update the background copy at {}", installed.display());
            }
        }
        if spawn_hidden(&installed.display().to_string(), &["--background".to_string()]) {
            say!("  Background copy restarted.");
        }
    }

    pub fn uninstall() -> Result<(), String> {
        stop_background();
        quiet(Command::new("reg").args(["delete", RUN_KEY, "/v", "DropCopy", "/f"]));
        dropcopy::sendto::remove_shortcut();
        quiet(Command::new("netsh").raw_arg("advfirewall firewall delete rule name=DropCopy"));
        if let Some(exe) = installed_exe() {
            let _ = std::fs::remove_file(exe);
        }
        say!("  Stopped and removed from startup (and the firewall rule, if this terminal is an administrator).");
        Ok(())
    }
}

#[cfg(not(windows))]
mod setup {
    use dropcopy::say;

    pub fn detach() -> bool {
        false
    }

    pub fn own_console() -> bool {
        false
    }

    pub fn attach_terminal() {}

    pub fn hide_console() {}

    #[allow(dead_code)] // `launch` only runs on Windows
    pub enum Bg {
        Running,
        Started,
    }

    pub fn ensure_background(_: &dropcopy::config::Config, _: bool) -> Result<Bg, String> {
        Err("only on Windows".into())
    }

    pub fn warn_if_public_network() {}

    pub fn restart(exe: &std::path::Path) -> bool {
        std::process::Command::new(exe).arg("--background").spawn().is_ok()
    }

    pub fn refresh_background(_: &std::path::Path) {}

    pub fn install() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        if cfg!(target_os = "macos") {
            say!("Add {} to System Settings > General > Login Items.", exe.display());
        } else {
            say!("Save as ~/.config/systemd/user/dropcopy.service, then: systemctl --user enable --now dropcopy\n");
            say!("[Unit]\nDescription=DropCopy receiver\n\n[Service]\nExecStart={}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target", exe.display());
        }
        Ok(())
    }

    pub fn uninstall() -> Result<(), String> {
        say!("Linux: systemctl --user disable --now dropcopy");
        Ok(())
    }
}

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
  dropcopy                      receive (shows the pairing QR code on first run)
  dropcopy pair [--new] [--png] [--invert]
                                show the pairing QR code
                                  --new     new key: every paired phone must scan again
                                  --png     also save it as an image and open it
                                  --invert  for terminals with a light background
  dropcopy send [--to HOST[:PORT]] [--text TEXT] [FILE...]
                                send to a DropCopy receiver (another PC)
  dropcopy dir [PATH]           show or change where received files are saved
  dropcopy name [NAME]          show or change the name phones see
  dropcopy setup                Windows: run in the background, start with Windows
  dropcopy unsetup              stop it and undo setup

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
            "--background" => a.background = true,
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ if a.cmd.is_none() => a.cmd = Some(arg),
            _ => a.rest.push(arg),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
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
    let (mut cfg, created) = Config::load_or_create(home).map_err(|e| format!("settings: {e}"))?;
    match a.cmd.as_deref() {
        None | Some("serve") | Some("receive") => serve(cfg, created, &a),
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
        Some("unsetup") => setup::uninstall(),
        Some(other) => Err(format!("unknown command {other} (try --help)")),
    }
}

fn serve(cfg: Config, first_run: bool, a: &Args) -> Result<(), String> {
    if a.hidden && setup::detach() {
        return Ok(());
    }
    if a.background {
        dropcopy::log_to(&cfg.home);
    }
    let cfg = Arc::new(cfg);
    let server = Server::bind(cfg.clone(), ServeOpts { port: a.port, clip: a.clip, discovery: true }).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            format!("port {} is in use - DropCopy is probably already running", a.port)
        } else {
            format!("cannot listen on port {}: {e}", a.port)
        }
    })?;
    say!("DropCopy - receiving as \"{}\" on port {}", cfg.name, server.port);
    say!("  Saving to {}", cfg.save_dir.display());
    if first_run {
        pair::print_pairing(&cfg, server.port, a.invert);
    } else {
        say!("  Pair another phone: dropcopy pair");
    }
    say!("  Waiting for your phone... (Ctrl+C to stop)\n");
    setup::warn_if_public_network();
    server.run().map_err(|e| e.to_string())
}

fn send(cfg: &mut Config, a: &Args) -> Result<(), String> {
    let mut items: Vec<Item> = a.texts.iter().cloned().map(Item::Text).collect();
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

    fn stop_background() {
        // The background copy is the installed one; an interactive window elsewhere keeps running.
        if let Some(exe) = installed_exe() {
            let filter = format!("$p = '{}'; Get-Process dropcopy -ErrorAction SilentlyContinue | Where-Object Path -eq $p | Stop-Process -Force", exe.display());
            quiet(Command::new("powershell").args(["-NoProfile", "-Command", &filter]));
        }
    }

    pub fn install() -> Result<(), String> {
        let current = std::env::current_exe().map_err(|e| e.to_string())?;
        let target = installed_exe().ok_or("LOCALAPPDATA is not set")?;
        stop_background();
        std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        if current != target {
            std::fs::copy(&current, &target).map_err(|e| format!("copying to {}: {e}", target.display()))?;
        }
        let exe = target.display().to_string();
        let value = format!("\"{exe}\" --hidden");
        if !quiet(Command::new("reg").args(["add", RUN_KEY, "/v", "DropCopy", "/t", "REG_SZ", "/d", &value, "/f"])) {
            return Err("could not add DropCopy to startup".into());
        }
        say!("  Installed to {exe}");
        say!("  It starts hidden every time you sign in, and waits quietly when there is no network.");
        // Transfers are authenticated with the pairing key, so allowing every profile is safe and
        // keeps it working when Windows labels a home Wi-Fi "Public".
        let rule = format!(
            "advfirewall firewall add rule name=DropCopy dir=in action=allow enable=yes profile=any program=\"{exe}\""
        );
        let _ = quiet(Command::new("netsh").raw_arg("advfirewall firewall delete rule name=DropCopy"));
        if quiet(Command::new("netsh").raw_arg(&rule)) {
            say!("  Firewall: allowed.");
        } else {
            say!("  Firewall: Windows will ask once when it starts - click Allow (tick both boxes).");
            say!("  (Or run `dropcopy setup` from an administrator terminal to skip the prompt.)");
        }
        if spawn_hidden(&exe, &["--background".to_string()]) {
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

    pub fn uninstall() -> Result<(), String> {
        stop_background();
        quiet(Command::new("reg").args(["delete", RUN_KEY, "/v", "DropCopy", "/f"]));
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

    pub fn warn_if_public_network() {}

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

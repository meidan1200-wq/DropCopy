//! Persistent settings: this PC's id and pairing key, its name, and the save folder.
//! Stored as `key=value` lines in `config.txt` inside the config directory.

use crate::proto::random;
use crate::util::{b64url, hex, unb64url, unhex};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Config {
    pub home: PathBuf,
    pub id: [u8; 8],
    pub key: [u8; 32],
    pub name: String,
    pub save_dir: PathBuf,
    /// Where `dropcopy send` last found a receiver, tried first next time.
    pub last_peer: Option<String>,
    /// Show the little phone above the taskbar when photos arrive.
    pub popup: bool,
    /// Watch Bluetooth for the phone being brought close.
    pub near: bool,
    /// Smoothed signal strength (dBm) that counts as "brought close". Higher = closer.
    pub near_rssi: i16,
    /// Install new releases automatically (background copy only).
    pub auto_update: bool,
}

/// Every setting `dropcopy set` accepts, with its meaning, for the help text.
pub const SETTINGS: &[(&str, &str)] = &[
    ("popup", "on|off   the phone above the taskbar: the photo when the phone comes close, and the pairing code"),
    ("near", "on|off   show the latest photo when the phone is brought close (Bluetooth)"),
    ("near_rssi", "-30..-90 how close counts as close (default -55; see `dropcopy near`)"),
    ("auto_update", "on|off   install new versions automatically"),
    ("name", "TEXT     the name phones see"),
];

fn on(v: Option<String>, default: bool) -> bool {
    match v.as_deref().map(str::trim) {
        Some("off" | "false" | "0" | "no") => false,
        Some("on" | "true" | "1" | "yes") => true,
        _ => default,
    }
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

pub fn default_home() -> PathBuf {
    if let Some(p) = env_path("DROPCOPY_HOME") {
        return p;
    }
    if cfg!(windows) {
        env_path("APPDATA").unwrap_or_else(|| PathBuf::from(".")).join("DropCopy")
    } else if cfg!(target_os = "macos") {
        env_path("HOME").unwrap_or_default().join("Library/Application Support/DropCopy")
    } else {
        env_path("XDG_CONFIG_HOME")
            .unwrap_or_else(|| env_path("HOME").unwrap_or_default().join(".config"))
            .join("dropcopy")
    }
}

fn default_save_dir() -> PathBuf {
    let home = if cfg!(windows) { env_path("USERPROFILE") } else { env_path("HOME") };
    home.unwrap_or_else(|| PathBuf::from(".")).join("Pictures").join("DropCopy")
}

pub fn hostname() -> String {
    let from_env = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok();
    let from_file = || fs::read_to_string("/etc/hostname").ok();
    from_env
        .or_else(from_file)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "PC".to_string())
}

impl Config {
    pub fn path(home: &Path) -> PathBuf {
        home.join("config.txt")
    }

    /// Loads the config, creating a fresh id and key on first run.
    pub fn load_or_create(home: PathBuf) -> io::Result<(Config, bool)> {
        let text = fs::read_to_string(Self::path(&home)).unwrap_or_default();
        let get = |k: &str| {
            text.lines()
                .filter_map(|l| l.split_once('='))
                .find(|(key, _)| key.trim() == k)
                .map(|(_, v)| v.trim().to_string())
        };
        let id = get("id").and_then(|v| unhex::<8>(&v));
        let key = get("key").and_then(|v| unb64url(&v)).and_then(|v| <[u8; 32]>::try_from(v).ok());
        let created = id.is_none() || key.is_none();
        let cfg = Config {
            id: id.unwrap_or_else(random),
            key: key.unwrap_or_else(random),
            name: get("name").filter(|s| !s.is_empty()).unwrap_or_else(hostname),
            save_dir: get("save_dir").map(PathBuf::from).unwrap_or_else(default_save_dir),
            last_peer: get("last_peer").filter(|s| !s.is_empty()),
            popup: on(get("popup"), true),
            near: on(get("near"), true),
            near_rssi: get("near_rssi").and_then(|v| v.parse().ok()).filter(|v| (-100..=-20).contains(v)).unwrap_or(-55),
            auto_update: on(get("auto_update"), true),
            home,
        };
        if created {
            cfg.save()?;
        }
        Ok((cfg, created))
    }

    pub fn save(&self) -> io::Result<()> {
        fs::create_dir_all(&self.home)?;
        let text = format!(
            "# DropCopy settings. Delete this file to unpair every phone.\n\
             id={}\nkey={}\nname={}\nsave_dir={}\nlast_peer={}\n\
             popup={}\nnear={}\nnear_rssi={}\nauto_update={}\n",
            hex(&self.id),
            b64url(&self.key),
            self.name,
            self.save_dir.display(),
            self.last_peer.as_deref().unwrap_or(""),
            if self.popup { "on" } else { "off" },
            if self.near { "on" } else { "off" },
            self.near_rssi,
            if self.auto_update { "on" } else { "off" },
        );
        let path = Self::path(&self.home);
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, text)?;
        restrict_permissions(&tmp);
        fs::rename(tmp, path)
    }

    /// Changes one setting by name (see `SETTINGS`).
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let flag = |v: &str| match v {
            "on" | "true" | "1" | "yes" => Ok(true),
            "off" | "false" | "0" | "no" => Ok(false),
            _ => Err(format!("{key} is on or off")),
        };
        match key {
            "popup" => self.popup = flag(value)?,
            "near" => self.near = flag(value)?,
            "auto_update" => self.auto_update = flag(value)?,
            "near_rssi" => {
                self.near_rssi = value
                    .parse()
                    .ok()
                    .filter(|v: &i16| (-100..=-20).contains(v))
                    .ok_or("near_rssi is a number from -100 to -20")?
            }
            "name" if !value.trim().is_empty() => self.name = value.trim().to_string(),
            _ => return Err(format!("unknown setting {key}")),
        }
        self.save().map_err(|e| e.to_string())
    }

    /// Forgets every paired phone by issuing a new id and key.
    pub fn rekey(&mut self) -> io::Result<()> {
        self.id = random();
        self.key = random();
        let _ = fs::remove_file(self.home.join(PAIRED));
        self.save()
    }

    /// True once a phone has connected with the current key, so it holds the pairing.
    pub fn is_paired(&self) -> bool {
        self.home.join(PAIRED).exists()
    }

    /// Called after every authenticated connection; writes only the first time.
    pub fn mark_paired(&self) {
        let p = self.home.join(PAIRED);
        if !p.exists() {
            let _ = fs::write(p, b"a phone has connected with this key\n");
        }
    }
}

const PAIRED: &str = "paired";
const LAST_PHONE: &str = "last-phone.txt";

/// The name of the phone that connected last ("Galaxy S20+"), for the setup window and the tray.
pub fn remember_phone(home: &Path, name: &str) {
    if last_phone(home).as_deref() != Some(name) {
        let _ = fs::write(home.join(LAST_PHONE), name);
    }
}

pub fn last_phone(home: &Path) -> Option<String> {
    fs::read_to_string(home.join(LAST_PHONE)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(unix)]
fn restrict_permissions(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_: &Path) {}

//! Persistent settings: this PC's id and pairing key, its name, and the save folder.
//! Stored as `key=value` lines in `config.txt` inside the config directory.

use crate::proto::random;
use crate::util::{b64url, hex, unb64url, unhex};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct Config {
    pub home: PathBuf,
    pub id: [u8; 8],
    pub key: [u8; 32],
    pub name: String,
    pub save_dir: PathBuf,
    /// Where `dropcopy send` last found a receiver, tried first next time.
    pub last_peer: Option<String>,
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
             id={}\nkey={}\nname={}\nsave_dir={}\nlast_peer={}\n",
            hex(&self.id),
            b64url(&self.key),
            self.name,
            self.save_dir.display(),
            self.last_peer.as_deref().unwrap_or(""),
        );
        let path = Self::path(&self.home);
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, text)?;
        restrict_permissions(&tmp);
        fs::rename(tmp, path)
    }

    /// Forgets every paired phone by issuing a new id and key.
    pub fn rekey(&mut self) -> io::Result<()> {
        self.id = random();
        self.key = random();
        self.save()
    }
}

#[cfg(unix)]
fn restrict_permissions(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_: &Path) {}

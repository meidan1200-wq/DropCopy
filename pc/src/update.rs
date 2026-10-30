//! Self-update from the project's GitHub releases.
//!
//! The background copy checks shortly after start and then every few hours. A newer release's
//! `dropcopy.exe` is downloaded next to the running one, its SHA-256 is checked against the digest
//! GitHub publishes for the asset, and the files are swapped (Windows lets a running exe be renamed,
//! not overwritten). The new version is started and this one exits. With no network nothing
//! happens and the next check tries again.
//!
//! Downloads use the `curl` that ships with Windows 10/11, so the binary carries no TLS stack.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const REPO: &str = "meidan1200-wq/DropCopy";
pub const ASSET: &str = if cfg!(windows) { "dropcopy.exe" } else { "dropcopy" };

pub fn current() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is x.y.z")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// Accepts "1.2.3" and "v1.2.3".
    pub fn parse(s: &str) -> Option<Version> {
        let mut it = s.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u32>().ok());
        let v = Version(it.next()??, it.next()??, it.next()??);
        it.next().is_none().then_some(v)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

pub struct Release {
    pub version: Version,
    pub url: String,
    pub sha256: Option<[u8; 32]>,
}

/// Picks the release asset out of GitHub's "latest release" JSON.
pub fn parse_release(json: &str) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let version = Version::parse(v.get("tag_name")?.as_str()?)?;
    let asset = v.get("assets")?.as_array()?.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(ASSET))?;
    let url = asset.get("browser_download_url")?.as_str()?.to_string();
    let sha256 = asset
        .get("digest")
        .and_then(|d| d.as_str())
        .and_then(|d| d.strip_prefix("sha256:"))
        .and_then(crate::util::unhex::<32>);
    Some(Release { version, url, sha256 })
}

fn curl() -> Command {
    let mut c = Command::new("curl");
    c.args(["-fsSL", "--max-time", "120", "-A", concat!("dropcopy/", env!("CARGO_PKG_VERSION"))]);
    c.stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: never flash a console from the background
    }
    c
}

/// The newest release, if it is newer than this build.
pub fn check() -> Result<Option<Release>, String> {
    let out = curl()
        .arg(format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err("no answer from GitHub (offline?)".into());
    }
    let rel = parse_release(&String::from_utf8_lossy(&out.stdout)).ok_or("release has no PC download")?;
    Ok((rel.version > current()).then_some(rel))
}

/// Downloads and verifies `rel` into `dest`.
pub fn download(rel: &Release, dest: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(dest);
    let ok = curl().arg("-o").arg(dest).arg(&rel.url).status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        return Err("download failed".into());
    }
    let data = std::fs::read(dest).map_err(|e| e.to_string())?;
    if data.len() < 100_000 {
        let _ = std::fs::remove_file(dest);
        return Err("download is too small to be the program".into());
    }
    if let Some(want) = rel.sha256 {
        let got: [u8; 32] = Sha256::digest(&data).into();
        if got != want {
            let _ = std::fs::remove_file(dest);
            return Err("download is corrupt (checksum mismatch)".into());
        }
    }
    Ok(())
}

fn sibling(exe: &Path, name: &str) -> PathBuf {
    exe.with_file_name(name)
}

/// Puts the downloaded `new` in place of `exe`; the old one is kept as `.old` until next start.
pub fn swap(exe: &Path, new: &Path) -> Result<(), String> {
    let old = sibling(exe, if cfg!(windows) { "dropcopy.old.exe" } else { "dropcopy.old" });
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| format!("cannot move the running program aside: {e}"))?;
    if let Err(e) = std::fs::rename(new, exe) {
        let _ = std::fs::rename(&old, exe); // put it back
        return Err(format!("cannot install the update: {e}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// Removes what a previous update left behind.
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        for n in ["dropcopy.old.exe", "dropcopy.new.exe", "dropcopy.old", "dropcopy.new"] {
            let _ = std::fs::remove_file(sibling(&exe, n));
        }
    }
}

/// Downloads and installs the update for `exe`. Returns the new version.
pub fn apply(exe: &Path, rel: &Release) -> Result<Version, String> {
    let new = sibling(exe, if cfg!(windows) { "dropcopy.new.exe" } else { "dropcopy.new" });
    download(rel, &new)?;
    swap(exe, &new)?;
    Ok(rel.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(Version::parse("v0.10.2"), Some(Version(0, 10, 2)));
        assert!(Version::parse("0.3.0").unwrap() > Version::parse("0.2.9").unwrap());
        assert!(Version::parse("1.0.0").unwrap() > Version::parse("0.99.99").unwrap());
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(Version::parse("v1.x.3"), None);
    }

    #[test]
    fn parses_github_release_json() {
        let json = format!(
            r#"{{"tag_name":"v9.1.0","assets":[
                {{"name":"DropCopy.apk","browser_download_url":"https://x/apk","digest":"sha256:{a}"}},
                {{"name":"{ASSET}","browser_download_url":"https://x/exe","digest":"sha256:{b}"}}]}}"#,
            a = "00".repeat(32),
            b = "ab".repeat(32)
        );
        let r = parse_release(&json).unwrap();
        assert_eq!(r.version, Version(9, 1, 0));
        assert_eq!(r.url, "https://x/exe");
        assert_eq!(r.sha256, Some([0xab; 32]));
        assert!(parse_release(r#"{"tag_name":"v1.0.0","assets":[]}"#).is_none());
        assert!(parse_release("not json").is_none());
    }

    #[test]
    fn swap_replaces_and_keeps_old_until_cleanup() {
        let d = std::env::temp_dir().join(format!("dc-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let exe = d.join(ASSET);
        let new = d.join("incoming");
        std::fs::write(&exe, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        swap(&exe, &new).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert!(!new.exists());
        let old = d.join(if cfg!(windows) { "dropcopy.old.exe" } else { "dropcopy.old" });
        assert_eq!(std::fs::read(old).unwrap(), b"old");
    }

    #[test]
    fn corrupt_download_is_rejected() {
        let d = std::env::temp_dir().join(format!("dc-dl-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let src = d.join("served.bin");
        std::fs::write(&src, vec![1u8; 200_000]).unwrap();
        let rel = Release { version: Version(9, 9, 9), url: format!("file://{}", src.display()), sha256: Some([0; 32]) };
        let dest = d.join("got.bin");
        let err = download(&rel, &dest).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert!(!dest.exists());
        let good: [u8; 32] = Sha256::digest(vec![1u8; 200_000]).into();
        let rel = Release { sha256: Some(good), ..rel };
        download(&rel, &dest).unwrap();
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), 200_000);
    }
}

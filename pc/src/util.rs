use std::path::{Path, PathBuf};

/// A DropCopy program file name, as Windows lists it: "dropcopy.exe", a renamed download such as
/// "dropcopy (1).exe" or "DropCopy-v0.3.0.exe", or the ".old"/".new" copies an update leaves.
pub fn is_dropcopy_exe(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("dropcopy") && n.ends_with(".exe")
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let s = s.trim();
    if s.len() != N * 2 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub fn b64url(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

pub fn unb64url(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s.trim()).ok()
}

/// Percent-encodes everything except RFC 3986 unreserved characters.
pub fn urlenc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn human_size(n: u64) -> String {
    const U: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", U[i]) }
}

/// Makes a name sent by the phone safe to use as a file name on any OS.
pub fn sanitize_filename(name: &str) -> String {
    // Keep only the last path component, whatever separator the sender used.
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut s: String = base
        .chars()
        .map(|c| if c.is_control() || "<>:\"|?*".contains(c) { '_' } else { c })
        .collect();
    let trimmed = s.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
    s = trimmed;
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
        "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if s.is_empty() || RESERVED.contains(&stem.as_str()) {
        s = format!("file_{s}");
    }
    // Stay well under the 255-byte limit, keeping the extension.
    if s.len() > 180 {
        let ext = Path::new(&s).extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
        let mut cut = 180 - ext.len().min(20);
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s = if ext.is_empty() || ext.len() > 20 { s[..cut].to_string() } else { format!("{}.{ext}", &s[..cut]) };
    }
    s
}

/// `dir/name`, or `dir/stem (2).ext` and so on if that already exists.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = path.extension().and_then(|e| e.to_str());
    for i in 2.. {
        let candidate = match ext {
            Some(e) => format!("{stem} ({i}).{e}"),
            None => format!("{stem} ({i})"),
        };
        let p = dir.join(candidate);
        if !p.exists() {
            return p;
        }
    }
    unreachable!()
}

/// The usual extension for a MIME type, for names that arrive without one.
pub fn ext_for_mime(mime: &str) -> Option<&'static str> {
    Some(match mime.split(';').next().unwrap_or("").trim() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/heic" | "image/heif" => "heic",
        "video/mp4" => "mp4",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        _ => return None,
    })
}

pub fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("heic") => "image/heic",
        Some("mp4") => "video/mp4",
        Some("pdf") => "application/pdf",
        Some("txt") => "text/plain",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_every_copy_of_itself_and_nothing_else() {
        for n in ["dropcopy.exe", "DropCopy.EXE", "dropcopy (1).exe", "dropcopy-v0.3.0.exe", "dropcopy.old.exe", "dropcopy.new.exe"] {
            assert!(is_dropcopy_exe(n), "{n}");
        }
        for n in ["explorer.exe", "dropbox.exe", "mydropcopy.exe", "dropcopy.txt", "dropcopy"] {
            assert!(!is_dropcopy_exe(n), "{n}");
        }
    }

    #[test]
    fn sanitize() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("C:\\Windows\\evil.exe"), "evil.exe");
        assert_eq!(sanitize_filename("a<b>:c?.png"), "a_b__c_.png");
        assert_eq!(sanitize_filename("CON.txt"), "file_CON.txt");
        assert_eq!(sanitize_filename(" .. "), "file_");
        assert_eq!(sanitize_filename("Screenshot_2026 10 30.png"), "Screenshot_2026 10 30.png");
        assert!(sanitize_filename(&"x".repeat(400)).len() <= 180);
        assert!(sanitize_filename(&format!("{}.jpg", "é".repeat(200))).ends_with(".jpg"));
    }

    #[test]
    fn encodings() {
        assert_eq!(unhex::<2>(&hex(&[0xab, 0x01])), Some([0xab, 0x01]));
        assert_eq!(unb64url(&b64url(b"\xff\x00k")).unwrap(), b"\xff\x00k");
        assert_eq!(urlenc("My PC #1"), "My%20PC%20%231");
        assert_eq!(ext_for_mime("image/jpeg"), Some("jpg"));
        assert_eq!(ext_for_mime("application/x-unknown"), None);
    }
}

/// Seconds since someone last touched this PC's keyboard or mouse (this signed-in session).
/// A phone with several PCs sends to the one in use. `None` where the OS doesn't say.
pub fn idle_seconds() -> Option<u64> {
    // Tests set this to stand in for a PC that is (or isn't) in use.
    if let Some(s) = std::env::var("DROPCOPY_IDLE").ok().and_then(|v| v.parse().ok()) {
        return Some(s);
    }
    os_idle_seconds()
}

#[cfg(windows)]
fn os_idle_seconds() -> Option<u64> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if !GetLastInputInfo(&mut li).as_bool() {
            return None;
        }
        Some(GetTickCount().wrapping_sub(li.dwTime) as u64 / 1000)
    }
}

#[cfg(not(windows))]
fn os_idle_seconds() -> Option<u64> {
    None
}

//! Files that are only meant to live a short while: proximity previews, and pictures sent with
//! "delete from PC after pasting". Each is written to a list in the settings folder before it is
//! handed out, so whatever a crash or a restart leaves behind is removed on the next start.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static HOME: OnceLock<PathBuf> = OnceLock::new();
static LIST: Mutex<()> = Mutex::new(());

const LIST_FILE: &str = "delete-pending.txt";

/// Where proximity previews are kept: never the user's own save folder.
pub fn nearby_dir(home: &Path) -> PathBuf {
    home.join("nearby")
}

/// Remembers the settings folder and removes what an earlier run left.
pub fn init(home: &Path) {
    let _ = HOME.set(home.to_path_buf());
    sweep(home);
}

fn sweep(home: &Path) {
    let _g = LIST.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(text) = std::fs::read_to_string(home.join(LIST_FILE)) {
        for line in text.lines().filter(|l| !l.is_empty()) {
            let _ = std::fs::remove_file(line);
        }
    }
    let _ = std::fs::remove_file(home.join(LIST_FILE));
    if let Ok(dir) = std::fs::read_dir(nearby_dir(home)) {
        for e in dir.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Records `files` as due for deletion (so a restart still removes them).
pub fn track(files: &[PathBuf]) {
    let Some(home) = HOME.get() else { return };
    let _g = LIST.lock().unwrap_or_else(|e| e.into_inner());
    let mut text = std::fs::read_to_string(home.join(LIST_FILE)).unwrap_or_default();
    for f in files {
        text.push_str(&f.display().to_string());
        text.push('\n');
    }
    let _ = std::fs::write(home.join(LIST_FILE), text);
}

fn untrack(files: &[PathBuf]) {
    let Some(home) = HOME.get() else { return };
    let _g = LIST.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(text) = std::fs::read_to_string(home.join(LIST_FILE)) else { return };
    let gone: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
    let kept: String = text.lines().filter(|l| !l.is_empty() && !gone.iter().any(|g| g == l)).map(|l| format!("{l}\n")).collect();
    let _ = std::fs::write(home.join(LIST_FILE), kept);
}

/// Deletes `files` after `after` (0 = now), off the calling thread.
pub fn delete_later(files: Vec<PathBuf>, after: Duration) {
    let run = move || {
        std::thread::sleep(after);
        for f in &files {
            match std::fs::remove_file(f) {
                Ok(()) => crate::say!("  deleted {}", f.display()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => crate::say!("  ! could not delete {}: {e}", f.display()),
            }
        }
        untrack(&files);
    };
    let _ = std::thread::Builder::new().name("cleanup".into()).spawn(run);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_files_survive_a_restart_only_until_the_next_start() {
        let home = std::env::temp_dir().join(format!("dropcopy-cleanup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(nearby_dir(&home)).unwrap();
        let kept = home.join("keep.jpg");
        let doomed = home.join("doomed.jpg");
        let preview = nearby_dir(&home).join("preview.jpg");
        for f in [&kept, &doomed, &preview] {
            std::fs::write(f, b"x").unwrap();
        }
        std::fs::write(home.join(LIST_FILE), format!("{}\n", doomed.display())).unwrap();
        sweep(&home);
        assert!(kept.exists());
        assert!(!doomed.exists());
        assert!(!preview.exists());
        assert!(!home.join(LIST_FILE).exists());
    }
}

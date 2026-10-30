//! Puts what arrived on the system clipboard so Ctrl+V pastes it anywhere.
//!
//! A single image goes on as a picture *and* as a file in one clipboard write: chat apps,
//! browsers, Paint and Office paste the picture, Explorer pastes the file. Several files go on
//! as a file list. Text goes on as text.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipMode {
    Off,
    /// Picture and file (default).
    Both,
    /// Picture only; non-images still go on as files.
    Image,
    /// Always as files.
    File,
}

impl ClipMode {
    pub fn parse(s: &str) -> Option<ClipMode> {
        Some(match s {
            "off" | "none" => ClipMode::Off,
            "both" => ClipMode::Both,
            "image" => ClipMode::Image,
            "file" => ClipMode::File,
            _ => return None,
        })
    }
}

/// Images larger than this are pasted as files only: a 200 MP photo would need 800 MB as a bitmap.
const MAX_PIXELS: u64 = 60_000_000;

pub fn is_image(path: &Path) -> bool {
    crate::util::mime_for(path).starts_with("image/")
}

/// Decodes and applies the EXIF orientation. Returns the pixels and whether they were rotated.
pub fn decode(raw: &[u8]) -> Option<(image::RgbaImage, bool)> {
    use image::metadata::Orientation;
    use image::{DynamicImage, ImageDecoder, ImageReader};
    let mut dec = ImageReader::new(std::io::Cursor::new(raw)).with_guessed_format().ok()?.into_decoder().ok()?;
    let (w, h) = dec.dimensions();
    if w as u64 * h as u64 > MAX_PIXELS {
        return None;
    }
    let orientation = dec.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(dec).ok()?;
    img.apply_orientation(orientation);
    Some((img.into_rgba8(), orientation != Orientation::NoTransforms))
}

/// Reads only the header: whether the image can be pasted as a bitmap, and whether its
/// EXIF orientation rotates it. Cheap, unlike `decode`.
pub fn probe(raw: &[u8]) -> Option<bool> {
    use image::metadata::Orientation;
    use image::{ImageDecoder, ImageReader};
    let mut dec = ImageReader::new(std::io::Cursor::new(raw)).with_guessed_format().ok()?.into_decoder().ok()?;
    let (w, h) = dec.dimensions();
    if w as u64 * h as u64 > MAX_PIXELS {
        return None;
    }
    Some(dec.orientation().unwrap_or(Orientation::NoTransforms) != Orientation::NoTransforms)
}

/// A Windows CF_DIB: BITMAPINFOHEADER followed by bottom-up 32-bit BGRA rows.
pub fn dib(img: &image::RgbaImage) -> Vec<u8> {
    let (w, h) = img.dimensions();
    let size = w as usize * h as usize * 4;
    let mut out = Vec::with_capacity(40 + size);
    out.extend_from_slice(&40u32.to_le_bytes()); // biSize
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes()); // positive: bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bit count
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes()); // 72 dpi
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    let raw = img.as_raw();
    let stride = w as usize * 4;
    for y in (0..h as usize).rev() {
        for px in raw[y * stride..(y + 1) * stride].chunks_exact(4) {
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    out
}

pub fn set_text(text: &str) -> Result<(), String> {
    imp::set_text(text)
}

pub fn set_files(files: &[PathBuf], mode: ClipMode) -> Result<(), String> {
    imp::set_files(files, mode)
}

#[cfg(windows)]
mod imp {
    //! Windows: a hidden message-only window owns the clipboard so the bitmap can use
    //! *delayed rendering*. The file list and the original PNG/JPEG bytes go on at once (about
    //! 1 ms); the bitmap is decoded in the background and handed over only when an app actually
    //! asks for it, so a 12 MP photo is pasteable the moment it lands.
    use super::{decode, dib, is_image, probe, ClipMode};
    use std::cell::RefCell;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Arc, Condvar, Mutex, OnceLock};
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{GlobalFree, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardOwner, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW, RegisterClassW, HWND_MESSAGE,
        MSG, WNDCLASSW,
    };

    const CF_DIB: u32 = 8;
    const CF_UNICODETEXT: u32 = 13;
    const CF_HDROP: u32 = 15;
    const WM_RENDERFORMAT: u32 = 0x0305;
    const WM_RENDERALLFORMATS: u32 = 0x0306;
    const WM_DESTROYCLIPBOARD: u32 = 0x0307;
    const WM_APP_JOB: u32 = 0x8000 + 1;

    fn register(name: &str) -> u32 {
        let w: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        unsafe { RegisterClipboardFormatW(w.as_ptr()) }
    }

    fn open(owner: HWND) -> Result<(), String> {
        // Another app may hold the clipboard for a moment; retry for up to half a second.
        for _ in 0..50 {
            if unsafe { OpenClipboard(owner) } != 0 {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err("the clipboard is held by another app".into())
    }

    /// Copies `bytes` into a movable global block and hands it to the clipboard.
    /// `None` registers the format for delayed rendering.
    fn put(format: u32, bytes: Option<&[u8]>) -> Result<(), String> {
        unsafe {
            let Some(bytes) = bytes else {
                SetClipboardData(format, std::ptr::null_mut());
                return Ok(());
            };
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
            if h.is_null() {
                return Err("out of memory".into());
            }
            let p = GlobalLock(h) as *mut u8;
            if p.is_null() {
                GlobalFree(h);
                return Err("GlobalLock failed".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            GlobalUnlock(h);
            // On success the system owns the memory.
            if SetClipboardData(format, h).is_null() {
                GlobalFree(h);
                return Err(format!("SetClipboardData({format}) failed"));
            }
        }
        Ok(())
    }

    /// A bitmap being prepared in the background.
    #[derive(Default)]
    struct Pending {
        done: Mutex<Option<Option<Vec<u8>>>>,
        cv: Condvar,
    }

    impl Pending {
        fn start(path: PathBuf) -> Arc<Pending> {
            let p = Arc::new(Pending::default());
            let q = p.clone();
            std::thread::spawn(move || {
                let out = std::fs::read(&path).ok().and_then(|raw| decode(&raw)).map(|(img, _)| dib(&img));
                *q.done.lock().unwrap() = Some(out);
                q.cv.notify_all();
            });
            p
        }

        fn wait(&self) -> Option<Vec<u8>> {
            let guard = self.done.lock().unwrap();
            let (mut guard, _) =
                self.cv.wait_timeout_while(guard, Duration::from_secs(10), |d| d.is_none()).unwrap();
            guard.take().flatten()
        }
    }

    struct Job {
        formats: Vec<(u32, Vec<u8>)>,
        delayed_dib: Option<Arc<Pending>>,
        reply: Sender<Result<(), String>>,
    }

    struct Owner {
        hwnd: usize,
        jobs: Mutex<Sender<Job>>,
    }

    thread_local! {
        static JOBS: RefCell<Option<std::sync::mpsc::Receiver<Job>>> = const { RefCell::new(None) };
        static PENDING: RefCell<Option<Arc<Pending>>> = const { RefCell::new(None) };
    }

    fn render_dib() {
        if let Some(p) = PENDING.with(|s| s.borrow_mut().take()) {
            if let Some(bytes) = p.wait() {
                let _ = put(CF_DIB, Some(&bytes));
            }
        }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_APP_JOB => {
                while let Some(job) = JOBS.with(|j| j.borrow().as_ref().and_then(|rx| rx.try_recv().ok())) {
                    let result = open(hwnd).and_then(|()| {
                        let r = (|| {
                            if EmptyClipboard() == 0 {
                                return Err("EmptyClipboard failed".to_string());
                            }
                            for (f, b) in &job.formats {
                                put(*f, Some(b))?;
                            }
                            if job.delayed_dib.is_some() {
                                put(CF_DIB, None)?;
                            }
                            Ok(())
                        })();
                        CloseClipboard();
                        r
                    });
                    PENDING.with(|s| *s.borrow_mut() = job.delayed_dib);
                    let _ = job.reply.send(result);
                }
                0
            }
            // An app is pasting the bitmap: the clipboard is already open for us.
            WM_RENDERFORMAT if wp as u32 == CF_DIB => {
                render_dib();
                0
            }
            // We are going away while still owning the clipboard: render now or it is lost.
            WM_RENDERALLFORMATS => {
                if OpenClipboard(hwnd) != 0 {
                    if GetClipboardOwner() == hwnd {
                        render_dib();
                    }
                    CloseClipboard();
                }
                0
            }
            // Something else was copied: drop the bitmap we were holding.
            WM_DESTROYCLIPBOARD => {
                PENDING.with(|s| s.borrow_mut().take());
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    fn owner() -> Result<&'static Owner, String> {
        static OWNER: OnceLock<Result<Owner, String>> = OnceLock::new();
        OWNER
            .get_or_init(|| {
                let (ready_tx, ready_rx) = channel();
                std::thread::Builder::new()
                    .name("clipboard".into())
                    .spawn(move || unsafe {
                        let (tx, rx) = channel::<Job>();
                        JOBS.with(|j| *j.borrow_mut() = Some(rx));
                        let class: Vec<u16> = "DropCopyClipboard\0".encode_utf16().collect();
                        let instance = GetModuleHandleW(std::ptr::null());
                        let mut wc: WNDCLASSW = std::mem::zeroed();
                        wc.lpfnWndProc = Some(wndproc);
                        wc.hInstance = instance;
                        wc.lpszClassName = class.as_ptr();
                        RegisterClassW(&wc);
                        let hwnd = CreateWindowExW(
                            0,
                            class.as_ptr(),
                            class.as_ptr(),
                            0,
                            0,
                            0,
                            0,
                            0,
                            HWND_MESSAGE,
                            std::ptr::null_mut(),
                            instance,
                            std::ptr::null(),
                        );
                        if hwnd.is_null() {
                            let _ = ready_tx.send(Err("cannot create the clipboard window".to_string()));
                            return;
                        }
                        let _ = ready_tx.send(Ok(Owner { hwnd: hwnd as usize, jobs: Mutex::new(tx) }));
                        let mut msg: MSG = std::mem::zeroed();
                        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                            DispatchMessageW(&msg);
                        }
                    })
                    .map_err(|e| e.to_string())?;
                ready_rx.recv().map_err(|e| e.to_string())?
            })
            .as_ref()
            .map_err(|e| e.clone())
    }

    fn submit(formats: Vec<(u32, Vec<u8>)>, delayed_dib: Option<Arc<Pending>>) -> Result<(), String> {
        let o = owner()?;
        let (reply, result) = channel();
        o.jobs.lock().unwrap().send(Job { formats, delayed_dib, reply }).map_err(|e| e.to_string())?;
        if unsafe { PostMessageW(o.hwnd as HWND, WM_APP_JOB, 0, 0) } == 0 {
            return Err("cannot reach the clipboard thread".into());
        }
        result.recv_timeout(Duration::from_secs(5)).map_err(|_| "clipboard thread did not answer".to_string())?
    }

    fn wide_bytes(s: &[u16]) -> Vec<u8> {
        s.iter().flat_map(|c| c.to_le_bytes()).collect()
    }

    /// DROPFILES header followed by double-NUL-terminated wide paths.
    fn hdrop(files: &[PathBuf]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&20u32.to_le_bytes()); // pFiles: paths start right after the header
        v.extend_from_slice(&0i32.to_le_bytes()); // pt.x
        v.extend_from_slice(&0i32.to_le_bytes()); // pt.y
        v.extend_from_slice(&0i32.to_le_bytes()); // fNC
        v.extend_from_slice(&1i32.to_le_bytes()); // fWide
        for f in files {
            let w: Vec<u16> = f.as_os_str().encode_wide().chain(Some(0)).collect();
            v.extend_from_slice(&wide_bytes(&w));
        }
        v.extend_from_slice(&[0, 0]);
        v
    }

    pub fn set_files(files: &[PathBuf], mode: ClipMode) -> Result<(), String> {
        let mut formats = vec![];
        let mut delayed = None;
        if mode != ClipMode::File && files.len() == 1 && is_image(&files[0]) {
            if let Ok(raw) = std::fs::read(&files[0]) {
                if let Some(rotated) = probe(&raw) {
                    // The original bytes are only correct as-is when no EXIF rotation applies.
                    let name = if raw.starts_with(b"\x89PNG") {
                        Some("PNG")
                    } else if raw.starts_with(&[0xFF, 0xD8, 0xFF]) && !rotated {
                        Some("JFIF")
                    } else if raw.starts_with(b"GIF8") {
                        Some("GIF")
                    } else {
                        None
                    };
                    if let Some(n) = name {
                        formats.push((register(n), raw));
                    }
                    delayed = Some(Pending::start(files[0].clone()));
                }
            }
        }
        if mode != ClipMode::Image || delayed.is_none() {
            formats.push((CF_HDROP, hdrop(files)));
            formats.push((register("Preferred DropEffect"), 1u32.to_le_bytes().to_vec())); // DROPEFFECT_COPY
        }
        submit(formats, delayed)
    }

    pub fn set_text(text: &str) -> Result<(), String> {
        let w: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        submit(vec![(CF_UNICODETEXT, wide_bytes(&w))], None)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{is_image, ClipMode};
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    fn pipe(cmd: &str, args: &[&str], input: &[u8]) -> Result<(), String> {
        let mut child = Command::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{cmd}: {e} (install it for clipboard support)"))?;
        child.stdin.take().unwrap().write_all(input).map_err(|e| e.to_string())?;
        // xclip and wl-copy fork a helper that keeps serving the clipboard; the parent exits.
        let status = child.wait().map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err(format!("{cmd} exited with {status}")) }
    }

    fn put(mime: &str, data: &[u8]) -> Result<(), String> {
        if cfg!(target_os = "macos") {
            return Err("unsupported type".into());
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            pipe("wl-copy", &["--type", mime], data)
        } else {
            pipe("xclip", &["-selection", "clipboard", "-t", mime, "-i"], data)
        }
    }

    fn osascript(script: &str) -> Result<(), String> {
        let ok = Command::new("osascript").args(["-e", script]).status().map_err(|e| e.to_string())?.success();
        if ok { Ok(()) } else { Err("osascript failed".into()) }
    }

    pub fn set_text(text: &str) -> Result<(), String> {
        if cfg!(target_os = "macos") {
            return pipe("pbcopy", &[], text.as_bytes());
        }
        put("text/plain;charset=utf-8", text.as_bytes()).or_else(|_| put("UTF8_STRING", text.as_bytes()))
    }

    pub fn set_files(files: &[PathBuf], mode: ClipMode) -> Result<(), String> {
        let single_image = files.len() == 1 && is_image(&files[0]) && mode != ClipMode::File;
        if cfg!(target_os = "macos") {
            let p = files[0].display().to_string().replace('"', "\\\"");
            return if single_image {
                osascript(&format!("set the clipboard to (read (POSIX file \"{p}\") as picture)"))
            } else {
                osascript(&format!("set the clipboard to (POSIX file \"{p}\")"))
            };
        }
        if single_image {
            let data = std::fs::read(&files[0]).map_err(|e| e.to_string())?;
            return put(crate::util::mime_for(&files[0]), &data);
        }
        let uris: String = files.iter().map(|f| format!("file://{}\r\n", f.display())).collect();
        put("text/uri-list", uris.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dib_layout() {
        let mut img = image::RgbaImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgba([1, 2, 3, 255])); // top-left
        img.put_pixel(1, 1, image::Rgba([9, 8, 7, 255])); // bottom-right
        let d = dib(&img);
        assert_eq!(d.len(), 40 + 16);
        assert_eq!(&d[4..8], &2i32.to_le_bytes());
        // First stored row is the bottom one, BGRA.
        assert_eq!(&d[40 + 4..40 + 8], &[7, 8, 9, 255]);
        assert_eq!(&d[40 + 8..40 + 12], &[3, 2, 1, 255]);
    }

    #[test]
    fn decodes_png() {
        let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let (out, rotated) = decode(&png).unwrap();
        assert_eq!(out.dimensions(), (3, 2));
        assert!(!rotated);
        assert!(decode(b"not an image").is_none());
    }
}

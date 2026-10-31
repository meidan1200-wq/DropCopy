//! Pastes like an app would (Windows): asks the clipboard for a format and prints what came back.
//! `clippaste [hdrop|png|text|watch]`. Used to check delayed rendering and "delete after paste".
#[cfg(windows)]
fn main() {
    use windows_sys::Win32::System::DataExchange::*;
    let what = std::env::args().nth(1).unwrap_or_else(|| "hdrop".into());
    unsafe {
        if what == "text" {
            // Copying something else, like Ctrl+C on some text.
            let w: Vec<u16> = "something else\0".encode_utf16().collect();
            let h = windows_sys::Win32::System::Memory::GlobalAlloc(2, w.len() * 2);
            let p = windows_sys::Win32::System::Memory::GlobalLock(h) as *mut u16;
            std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
            windows_sys::Win32::System::Memory::GlobalUnlock(h);
            while OpenClipboard(std::ptr::null_mut()) == 0 {}
            EmptyClipboard();
            SetClipboardData(13, h);
            CloseClipboard();
            println!("copied text");
            return;
        }
        let fmt = if what == "png" {
            let n: Vec<u16> = "PNG\0".encode_utf16().collect();
            RegisterClipboardFormatW(n.as_ptr())
        } else {
            15
        };
        while OpenClipboard(std::ptr::null_mut()) == 0 {}
        let h = GetClipboardData(fmt);
        let size = if h.is_null() { 0 } else { windows_sys::Win32::System::Memory::GlobalSize(h) };
        CloseClipboard();
        println!("pasted {what}: {size} bytes");
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows only");
}

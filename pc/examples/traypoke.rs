//! Windows test helper: right-clicks the DropCopy tray icon by sending its callback message.
#[cfg(windows)]
fn main() {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let class: Vec<u16> = "DropCopyTray\0".encode_utf16().collect();
    unsafe {
        let hwnd = FindWindowW(class.as_ptr(), std::ptr::null());
        if hwnd.is_null() {
            println!("no tray window");
            return;
        }
        // NOTIFYICON_VERSION_4: lParam low word = event, wParam = anchor x | y << 16.
        let (x, y) = (1100usize, 760usize);
        PostMessageW(hwnd, WM_APP + 10, x | (y << 16), 0x007B /* WM_CONTEXTMENU */ | (1 << 16));
        println!("poked");
    }
}
#[cfg(not(windows))]
fn main() {}

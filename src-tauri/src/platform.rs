use tauri::WebviewWindow;

pub fn apply(win: &WebviewWindow, enabled: bool) -> Result<&'static str, String> {
    if !enabled { detach(win)?; return Ok("normal"); }
    #[cfg(target_os = "windows")]
    { detach_windows(win)?; }
    #[cfg(target_os = "linux")]
    { if std::env::var_os("WAYLAND_DISPLAY").is_none() && attach_x11(win, true).is_ok() { return Ok("desktop"); } }
    // Wayland cannot honor Tauri's bottom stacking request. Keep an ordinary widget there.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() { return Ok("normal"); }
    win.set_always_on_bottom(true).map_err(|e| e.to_string())?;
    Ok("bottom")
}

pub fn raise(win: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    if let Ok(handle) = win.hwnd() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, SetWindowPos, SWP_NOMOVE, SWP_NOSIZE};
        unsafe {
            SetWindowPos(handle.0 as _, std::ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            SetForegroundWindow(handle.0 as _);
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = win;
}

fn detach(win: &WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    detach_windows(win)?;
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_none() { let _ = attach_x11(win, false); }
    win.set_always_on_bottom(false).map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
mod windows_desktop {
    use super::*;
    use std::ffi::c_void;
    type Hwnd = *mut c_void;
    #[link(name = "user32")]
    extern "system" {
        fn SetParent(child: Hwnd, parent: Hwnd) -> Hwnd;
        fn GetParent(hwnd: Hwnd) -> Hwnd;
        fn GetClassNameW(hwnd: Hwnd, class_name: *mut u16, max_count: i32) -> i32;
        fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
        fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    }
    fn hwnd(win: &WebviewWindow) -> Result<Hwnd, String> { use raw_window_handle::{HasWindowHandle, RawWindowHandle}; let h = win.window_handle().map_err(|e| e.to_string())?; match h.as_raw() { RawWindowHandle::Win32(h) => Ok(h.hwnd.get() as Hwnd), _ => Err("Win32 창을 찾을 수 없습니다".into()) } }
    pub(super) fn detach_windows(win: &WebviewWindow) -> Result<(), String> {
        let child = hwnd(win)?;
        unsafe {
            let parent = GetParent(child);
            if parent.is_null() { return Ok(()); }
            let mut class = [0u16; 64];
            let count = GetClassNameW(parent, class.as_mut_ptr(), class.len() as i32);
            if count <= 0 || String::from_utf16_lossy(&class[..count as usize]) != "WorkerW" { return Ok(()); }
            SetParent(child, std::ptr::null_mut());
            let style = GetWindowLongPtrW(child, -16);
            SetWindowLongPtrW(child, -16, (style | 0x80000000u32 as isize) & !0x40000000);
            SetWindowPos(child, std::ptr::null_mut(), 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0004 | 0x0020);
        }
        Ok(())
    }
}
#[cfg(target_os = "windows")]
use windows_desktop::detach_windows;

#[cfg(target_os = "linux")]
fn attach_x11(win: &WebviewWindow, desktop: bool) -> Result<(), String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::{connection::Connection, protocol::xproto::{AtomEnum, ConnectionExt, PropMode}, wrapper::ConnectionExt as _};
    let handle = win.window_handle().map_err(|e| e.to_string())?;
    let xid = match handle.as_raw() { RawWindowHandle::Xlib(h) => h.window as u32, RawWindowHandle::Xcb(h) => h.window.get(), _ => return Err("X11 창을 찾을 수 없습니다".into()) };
    let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
    let intern = |name: &[u8]| -> Result<u32, String> { Ok(conn.intern_atom(false, name).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom) };
    let property = intern(b"_NET_WM_WINDOW_TYPE")?;
    let kind: &[u8] = if desktop { b"_NET_WM_WINDOW_TYPE_DESKTOP" } else { b"_NET_WM_WINDOW_TYPE_NORMAL" };
    let value = intern(kind)?;
    conn.change_property32(PropMode::REPLACE, xid, property, AtomEnum::ATOM, &[value]).map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())?;
    Ok(())
}

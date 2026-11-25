//! Where is the mouse, relative to our window, in physical pixels?
//!
//! While an OS-level drag-and-drop is in progress the pointer is grabbed by the drag source.
//! On Windows winit reports no cursor movement during a drag, so to support dropping onto a
//! specific folder (like the Drive web UI) we ask the OS directly. (On Linux/X11 our patched
//! winit turns XDND position messages into normal cursor events instead.)

#[cfg_attr(not(windows), allow(unused_imports))]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub enum PointerProbe {
    #[cfg(windows)]
    Win32 { hwnd: isize },
    Unsupported,
}

impl PointerProbe {
    pub fn new(frame: &eframe::Frame) -> PointerProbe {
        let Ok(handle) = frame.window_handle() else { return PointerProbe::Unsupported };
        match handle.as_raw() {
            #[cfg(windows)]
            RawWindowHandle::Win32(h) => PointerProbe::Win32 { hwnd: h.hwnd.get() },
            _ => PointerProbe::Unsupported,
        }
    }

    /// Pointer position relative to the window's client area, in physical pixels.
    pub fn position(&self) -> Option<(f32, f32)> {
        match self {
            #[cfg(windows)]
            PointerProbe::Win32 { hwnd } => {
                use windows_sys::Win32::Foundation::POINT;
                use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
                use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
                let mut p = POINT { x: 0, y: 0 };
                // SAFETY: plain Win32 calls with a valid out-pointer and our own window handle.
                unsafe {
                    if GetCursorPos(&mut p) == 0 || ScreenToClient(*hwnd as _, &mut p) == 0 {
                        return None;
                    }
                }
                Some((p.x as f32, p.y as f32))
            }
            PointerProbe::Unsupported => None,
        }
    }
}

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

#[cfg(windows)]
use windows::Win32::Foundation::HWND;

#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pub window: RawWindowHandle,
    pub display: RawDisplayHandle,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    window_rwh06: rwh06::RawWindowHandle,
    display_rwh06: rwh06::RawDisplayHandle,
}

pub mod rwh06 {
    pub use winit::raw_window_handle::{RawDisplayHandle, RawWindowHandle};
}

impl Surface {
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn window_handle_06(&self) -> rwh06::RawWindowHandle {
        self.window_rwh06
    }

    pub fn display_handle_06(&self) -> rwh06::RawDisplayHandle {
        self.display_rwh06
    }

    #[cfg(windows)]
    pub fn hwnd(&self) -> Result<HWND, String> {
        match self.window {
            RawWindowHandle::Win32(win32) => Ok(HWND(win32.hwnd as *mut core::ffi::c_void)),
            _ => Err("window is not win32".to_string()),
        }
    }

    pub fn from_winit(window: &winit::window::Window) -> Result<Self, String> {
        use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
        use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};

        let window_05 = window.raw_window_handle();
        let display_05 = window.raw_display_handle();
        let window_06 = window
            .window_handle()
            .map_err(|err| err.to_string())?
            .as_raw();
        let display_06 = window
            .display_handle()
            .map_err(|err| err.to_string())?
            .as_raw();
        let size = window.inner_size();

        Ok(Self {
            window: window_05,
            display: display_05,
            width: size.width.max(1),
            height: size.height.max(1),
            scale_factor: window.scale_factor(),
            window_rwh06: window_06,
            display_rwh06: display_06,
        })
    }
}

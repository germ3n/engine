use crate::platform::Surface;
use windows::Win32::Foundation::HWND;

pub struct Desktop {
    pub hwnd: HWND,
    pub width: u32,
    pub height: u32,
}

pub fn attach_desktop(surface: &Surface) -> Result<Desktop, String> {
    let (width, height) = surface.size();

    Ok(Desktop {
        hwnd: surface.hwnd()?,
        width,
        height,
    })
}

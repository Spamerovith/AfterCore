//! The overlay window: WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE.
//!
//! * per-pixel alpha via `UpdateLayeredWindow` from a 32-bit top-down DIB section;
//! * click-through (WS_EX_TRANSPARENT) and never activated, so the game keeps focus;
//! * optional `WDA_EXCLUDEFROMCAPTURE` to keep the overlay out of recordings / streams.

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ac_core::config::{Anchor, OverlayLayout, OverlayStyle};
use ac_core::process::Rect;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::render::{Cell, Renderer, Scene};

pub struct OverlayFrame {
    pub cells: Vec<Cell>,
    pub style: OverlayStyle,
    pub layout: OverlayLayout,
    pub scale: f32,
    pub graph_height: f32,
    pub compact: bool,
    pub anchor: Anchor,
    pub offset: [i32; 2],
    /// Screen rectangle to place the overlay in (game client area or monitor).
    pub area: Rect,
    pub hide_from_capture: bool,
}

/// Implemented by the app; called by the overlay thread every tick.
pub trait OverlaySource: Send + Sync + 'static {
    /// `None` hides the overlay.
    fn frame(&self) -> Option<OverlayFrame>;
    /// Desired tick interval.
    fn interval(&self) -> Duration;
    fn should_quit(&self) -> bool {
        false
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

struct Surface {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    w: i32,
    h: i32,
}

impl Surface {
    fn new(w: i32, h: i32) -> Option<Self> {
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            let dc = CreateCompatibleDC(screen);
            ReleaseDC(std::ptr::null_mut(), screen);
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bi.bmiHeader.biWidth = w;
            bi.bmiHeader.biHeight = -h;
            bi.bmiHeader.biPlanes = 1;
            bi.bmiHeader.biBitCount = 32;
            bi.bmiHeader.biCompression = BI_RGB;
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            if bmp.is_null() || bits.is_null() {
                DeleteDC(dc);
                return None;
            }
            let old = SelectObject(dc, bmp);
            Some(Self { dc, bmp, old, bits: bits as *mut u8, w, h })
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bmp);
            DeleteDC(self.dc);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn spawn(source: Arc<dyn OverlaySource>) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("overlay".into())
        .spawn(move || unsafe { run(source) })
        .expect("spawn overlay thread")
}

unsafe fn run(source: Arc<dyn OverlaySource>) {
    let hinst = GetModuleHandleW(std::ptr::null());
    let class = wide("AfterCoreOverlay");
    let wc = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class.as_ptr(),
    };
    RegisterClassW(&wc);
    let title = wide("AfterCore Overlay");
    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        class.as_ptr(),
        title.as_ptr(),
        WS_POPUP,
        0,
        0,
        1,
        1,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        hinst,
        std::ptr::null(),
    );
    if hwnd.is_null() {
        return;
    }

    let mut renderer = Renderer::new();
    let mut surface: Option<Surface> = None;
    let mut visible = false;
    let mut capture_excluded = false;
    let mut last_topmost = Instant::now();

    loop {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            if msg.message == WM_QUIT {
                DestroyWindow(hwnd);
                return;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if source.should_quit() {
            DestroyWindow(hwnd);
            return;
        }

        match source.frame() {
            None => {
                if visible {
                    ShowWindow(hwnd, SW_HIDE);
                    visible = false;
                }
            }
            Some(f) => {
                let pm = renderer.render(&Scene {
                    cells: &f.cells,
                    style: &f.style,
                    layout: f.layout,
                    scale: f.scale,
                    graph_height: f.graph_height,
                    compact: f.compact,
                });
                let (w, h) = (pm.width() as i32, pm.height() as i32);
                if surface.as_ref().map(|s| s.w != w || s.h != h).unwrap_or(true) {
                    surface = Surface::new(w, h);
                }
                let Some(s) = surface.as_ref() else { continue };
                // RGBA (premultiplied) → BGRA (premultiplied)
                let dst = std::slice::from_raw_parts_mut(s.bits, (w * h * 4) as usize);
                for (d, p) in dst.chunks_exact_mut(4).zip(pm.data().chunks_exact(4)) {
                    d[0] = p[2];
                    d[1] = p[1];
                    d[2] = p[0];
                    d[3] = p[3];
                }

                let (fx, fy) = f.anchor.factors();
                let a = f.area;
                let ox = if fx < 0.25 { f.offset[0] } else if fx > 0.75 { -f.offset[0] } else { f.offset[0] };
                let oy = if fy < 0.25 { f.offset[1] } else if fy > 0.75 { -f.offset[1] } else { f.offset[1] };
                let x = a.left + ((a.width() - w) as f32 * fx) as i32 + ox;
                let y = a.top + ((a.height() - h) as f32 * fy) as i32 + oy;

                let pt_dst = POINT { x, y };
                let size = SIZE { cx: w, cy: h };
                let pt_src = POINT { x: 0, y: 0 };
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: (f.style.opacity.clamp(0.1, 1.0) * 255.0) as u8,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                let screen = GetDC(std::ptr::null_mut());
                UpdateLayeredWindow(hwnd, screen, &pt_dst, &size, s.dc, &pt_src, 0, &blend, ULW_ALPHA);
                ReleaseDC(std::ptr::null_mut(), screen);

                if f.hide_from_capture != capture_excluded {
                    SetWindowDisplayAffinity(hwnd, if f.hide_from_capture { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE });
                    capture_excluded = f.hide_from_capture;
                }
                if !visible {
                    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    visible = true;
                }
                // Games sometimes grab topmost; reassert regularly.
                if last_topmost.elapsed() > Duration::from_millis(750) {
                    SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
                    last_topmost = Instant::now();
                }
            }
        }

        let ms = source.interval().as_millis().clamp(16, 1000) as u32;
        MsgWaitForMultipleObjects(0, std::ptr::null(), 0, ms, QS_ALLINPUT);
    }
}

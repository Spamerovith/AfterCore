//! Tiny shared Win32 utilities.

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};

/// UTF-16, NUL-terminated.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// UTF-16 buffer (possibly NUL-terminated) to String.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// RAII wrapper for a kernel HANDLE.
pub struct OwnedHandle(pub HANDLE);

impl OwnedHandle {
    pub fn new(h: HANDLE) -> Option<Self> {
        if h.is_null() || h as isize == -1 {
            None
        } else {
            Some(Self(h))
        }
    }
    pub fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

unsafe impl Send for OwnedHandle {}

/// Local time as `YYYY-MM-DD_HH-MM-SS` (file friendly) and `DD.MM.YYYY HH:MM:SS` (display).
pub fn local_time_strings() -> (String, String) {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut st = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut st) };
    let file = format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    );
    let disp = format!(
        "{:02}.{:02}.{:04} {:02}:{:02}:{:02}",
        st.wDay, st.wMonth, st.wYear, st.wHour, st.wMinute, st.wSecond
    );
    (file, disp)
}

/// `HH:MM:SS` for the overlay clock.
pub fn local_clock() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut st = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut st) };
    format!("{:02}:{:02}:{:02}", st.wHour, st.wMinute, st.wSecond)
}

/// Monotonic high resolution clock in seconds.
pub fn qpc_seconds() -> f64 {
    use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    let mut f = 0i64;
    let mut c = 0i64;
    unsafe {
        QueryPerformanceFrequency(&mut f);
        QueryPerformanceCounter(&mut c);
    }
    c as f64 / f.max(1) as f64
}

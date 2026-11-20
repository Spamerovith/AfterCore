//! Process / window helpers.

use std::collections::HashMap;

use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
};

use crate::win::{from_wide, wide, OwnedHandle};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub threads: u32,
}

pub fn list_processes() -> Vec<ProcInfo> {
    let mut out = Vec::new();
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let Some(snap) = OwnedHandle::new(snap) else { return out };
    let mut e: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut ok = unsafe { Process32FirstW(snap.raw(), &mut e) };
    while ok != 0 {
        if e.th32ProcessID != 0 {
            out.push(ProcInfo { pid: e.th32ProcessID, name: from_wide(&e.szExeFile), threads: e.cntThreads });
        }
        ok = unsafe { Process32NextW(snap.raw(), &mut e) };
    }
    out
}

/// pid → lowercase exe name for all processes.
pub fn process_name_map() -> HashMap<u32, String> {
    list_processes().into_iter().map(|p| (p.pid, p.name.to_lowercase())).collect()
}

/// `\Device\HarddiskVolume3\Games\x.exe` → `D:\Games\x.exe`.
fn dos_path(nt: &str) -> String {
    use windows_sys::Win32::Storage::FileSystem::QueryDosDeviceW;
    for letter in b'A'..=b'Z' {
        let drive = format!("{}:", letter as char);
        let mut buf = [0u16; 512];
        let n = unsafe { QueryDosDeviceW(wide(&drive).as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        if n == 0 {
            continue;
        }
        let dev = from_wide(&buf);
        if !dev.is_empty() {
            if let Some(rest) = nt.strip_prefix(&dev) {
                if rest.starts_with('\\') {
                    return format!("{drive}{rest}");
                }
            }
        }
    }
    nt.to_string()
}

/// Full exe path **without opening the process**: the kernel's process-id → image-name
/// query (`SystemProcessIdInformation`), which needs no handle to the target. Anti-cheats
/// that watch handle creation on the game see nothing.
pub fn process_path(pid: u32) -> Option<String> {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    type NtQsi = unsafe extern "system" fn(u32, *mut u8, u32, *mut u32) -> i32;
    const SYSTEM_PROCESS_ID_INFORMATION: u32 = 88;
    #[repr(C)]
    struct Info {
        pid: usize,
        len: u16,
        max: u16,
        buf: *mut u16,
    }
    if pid == 0 {
        return None;
    }
    unsafe {
        let ntdll = GetModuleHandleW(wide("ntdll.dll").as_ptr());
        if ntdll.is_null() {
            return None;
        }
        let f: NtQsi = std::mem::transmute(GetProcAddress(ntdll, c"NtQuerySystemInformation".as_ptr().cast())?);
        let mut name = vec![0u16; 2048];
        let mut info = Info { pid: pid as usize, len: 0, max: (name.len() * 2) as u16, buf: name.as_mut_ptr() };
        let st = f(
            SYSTEM_PROCESS_ID_INFORMATION,
            &mut info as *mut Info as *mut u8,
            std::mem::size_of::<Info>() as u32,
            std::ptr::null_mut(),
        );
        if st < 0 || info.len == 0 {
            return None;
        }
        let nt = String::from_utf16_lossy(&name[..info.len as usize / 2]);
        Some(dos_path(&nt))
    }
}

/// Exe file name (e.g. `Cyberpunk2077.exe`).
pub fn process_name(pid: u32) -> Option<String> {
    let path = process_path(pid)?;
    path.rsplit(['\\', '/']).next().map(str::to_string)
}

#[cfg(test)]
mod path_tests {
    #[test]
    fn own_path_without_handle() {
        let me = std::env::current_exe().unwrap().to_string_lossy().to_lowercase();
        let got = super::process_path(std::process::id()).unwrap().to_lowercase();
        assert_eq!(got, me);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
    fn from(r: RECT) -> Self {
        Self { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
    }
}

#[derive(Clone, Debug)]
pub struct ForegroundInfo {
    pub hwnd: isize,
    pub pid: u32,
    /// Client area in screen coordinates.
    pub client: Rect,
    pub monitor: Rect,
    /// Window covers its whole monitor (exclusive / borderless fullscreen).
    pub fullscreen: bool,
}

pub fn foreground() -> Option<ForegroundInfo> {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.is_null() || IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return None;
        }
        let mut cloaked = 0u32;
        DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, (&mut cloaked as *mut u32).cast(), 4);
        if cloaked != 0 {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rc);
        let mut tl = POINT { x: rc.left, y: rc.top };
        let mut br = POINT { x: rc.right, y: rc.bottom };
        ClientToScreen(hwnd, &mut tl);
        ClientToScreen(hwnd, &mut br);
        let client = Rect { left: tl.x, top: tl.y, right: br.x, bottom: br.y };

        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(mon, &mut mi);
        let monitor = Rect::from(mi.rcMonitor);

        let mut wr: RECT = std::mem::zeroed();
        GetWindowRect(hwnd, &mut wr);
        let fullscreen = wr.left <= monitor.left
            && wr.top <= monitor.top
            && wr.right >= monitor.right
            && wr.bottom >= monitor.bottom;

        Some(ForegroundInfo { hwnd: hwnd as isize, pid, client, monitor, fullscreen })
    }
}

pub fn is_elevated() -> bool {
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let token = OwnedHandle(token);
        let mut elev = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            token.raw(),
            TokenElevation,
            (&mut elev as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        );
        ok != 0 && elev.TokenIsElevated != 0
    }
}

/// Start this exe again with "runas". Returns false if the user declined UAC.
pub fn relaunch_elevated(args: &str) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let Ok(exe) = std::env::current_exe() else { return false };
    let exe = wide(&exe.to_string_lossy());
    let verb = wide("runas");
    let params = wide(args);
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            exe.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    r as isize > 32
}

/// Open a folder / file / URL with the shell.
pub fn shell_open(target: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let t = wide(target);
    let verb = wide("open");
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), t.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

// ─────────────────────────── Autostart ───────────────────────────

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const TASK_NAME: &str = "AfterCore";

fn hidden_command(program: &str, args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Enable / disable start with Windows.
///
/// When elevated a scheduled task with highest privileges is used, so the app starts at
/// logon with admin rights and without a UAC prompt. Otherwise a HKCU Run entry is written.
pub fn set_autostart(enable: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_string_lossy().to_string();
    // Always clean both mechanisms first.
    let _ = hidden_command("schtasks", &["/Delete", "/TN", TASK_NAME, "/F"]);
    reg_run_set(None);
    if !enable {
        return Ok(());
    }
    if is_elevated() {
        let tr = format!("\"{exe}\" --tray");
        if hidden_command(
            "schtasks",
            &["/Create", "/TN", TASK_NAME, "/TR", &tr, "/SC", "ONLOGON", "/RL", "HIGHEST", "/F"],
        ) {
            return Ok(());
        }
    }
    if reg_run_set(Some(&format!("\"{exe}\" --tray"))) {
        Ok(())
    } else {
        Err("не удалось записать автозапуск".into())
    }
}

fn reg_run_set(value: Option<&str>) -> bool {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
        REG_SZ,
    };
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        let sub = wide(RUN_KEY);
        if RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_SET_VALUE, &mut key) != 0 {
            return false;
        }
        let name = wide(TASK_NAME);
        let ok = match value {
            Some(v) => {
                let data = wide(v);
                RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32) == 0
            }
            None => {
                RegDeleteValueW(key, name.as_ptr());
                true
            }
        };
        RegCloseKey(key);
        ok
    }
}

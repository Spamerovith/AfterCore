//! System font discovery (registry) + user fonts folder.

use std::path::{Path, PathBuf};

use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontEntry {
    pub name: String,
    pub path: String,
}

const FONTS_KEY: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn windows_fonts_dir() -> PathBuf {
    std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows")).join("Fonts")
}

fn read_key(root: HKEY, out: &mut Vec<FontEntry>) {
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        let sub = wide(FONTS_KEY);
        if RegOpenKeyExW(root, sub.as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return;
        }
        let dir = windows_fonts_dir();
        let mut i = 0u32;
        loop {
            let mut name = [0u16; 512];
            let mut name_len = name.len() as u32;
            let mut data = [0u16; 1024];
            let mut data_len = (data.len() * 2) as u32;
            let mut ty = 0u32;
            let rc = RegEnumValueW(
                key,
                i,
                name.as_mut_ptr(),
                &mut name_len,
                std::ptr::null(),
                &mut ty,
                data.as_mut_ptr().cast(),
                &mut data_len,
            );
            if rc != 0 {
                break;
            }
            i += 1;
            let n = String::from_utf16_lossy(&name[..name_len as usize]);
            let file = String::from_utf16_lossy(&data[..(data_len as usize / 2)]).trim_end_matches('\0').to_string();
            let lower = file.to_lowercase();
            if !(lower.ends_with(".ttf") || lower.ends_with(".otf") || lower.ends_with(".ttc")) {
                continue;
            }
            let path = if Path::new(&file).is_absolute() { PathBuf::from(&file) } else { dir.join(&file) };
            let clean = n.replace(" (TrueType)", "").replace(" (OpenType)", "").trim().to_string();
            out.push(FontEntry { name: clean, path: path.to_string_lossy().to_string() });
        }
        RegCloseKey(key);
    }
}

/// All installed fonts plus `.ttf/.otf` files from `extra_dir`, sorted by name.
pub fn list_fonts(extra_dir: Option<&Path>) -> Vec<FontEntry> {
    let mut out = Vec::new();
    read_key(HKEY_LOCAL_MACHINE, &mut out);
    read_key(HKEY_CURRENT_USER, &mut out);
    if let Some(dir) = extra_dir {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                if matches!(ext.as_str(), "ttf" | "otf" | "ttc") {
                    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    out.push(FontEntry { name: format!("{stem} (пользовательский)"), path: p.to_string_lossy().to_string() });
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out.dedup_by(|a, b| a.name == b.name);
    out
}

/// Preferred UI / overlay font: Segoe UI Variable (Windows 11) → Segoe UI.
pub fn default_font_path() -> PathBuf {
    let dir = windows_fonts_dir();
    for f in ["SegUIVar.ttf", "segoeui.ttf", "arial.ttf"] {
        let p = dir.join(f);
        if p.exists() {
            return p;
        }
    }
    dir.join("segoeui.ttf")
}

/// Icon font: Segoe Fluent Icons (Windows 11) → Segoe MDL2 Assets (Windows 10).
pub fn icon_font_path() -> Option<PathBuf> {
    let dir = windows_fonts_dir();
    ["SegoeIcons.ttf", "segmdl2.ttf"].into_iter().map(|f| dir.join(f)).find(|p| p.exists())
}

/// Monospace font for numbers in the UI.
pub fn mono_font_path() -> Option<PathBuf> {
    let dir = windows_fonts_dir();
    ["CascadiaMono.ttf", "consola.ttf"].into_iter().map(|f| dir.join(f)).find(|p| p.exists())
}

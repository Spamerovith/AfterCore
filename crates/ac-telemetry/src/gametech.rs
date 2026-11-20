//! Facts about a game that are read **without touching the game process**:
//! frame generation technology (from the files shipped next to the game) and anti-cheat.
//!
//! AfterCore deliberately never opens games with VM_READ, never enumerates their modules and
//! never walks their address space - that is what cheats do and what anti-cheats look for.
//! No handle to a game is opened at all: exe path, CPU time, working set and threads come
//! from the kernel process list (NtQuerySystemInformation), like Task Manager.

use ac_core::anticheat::{self, AntiCheat};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameTech {
    /// "DLSS", "FSR", "XeSS" - `None` when it cannot be told unambiguously.
    pub fg: Option<&'static str>,
    pub anticheat: Option<AntiCheat>,
}

/// Only an unambiguous single frame-generation library in the game folder counts.
pub fn fg_from_files(files: &[String]) -> Option<&'static str> {
    let dlss = files.iter().any(|f| f == "nvngx_dlssg.dll");
    let xess = files.iter().any(|f| f.starts_with("libxess_fg"));
    let fsr = files
        .iter()
        .any(|f| f.contains("framegeneration") || f.contains("frameinterpolation") || f.starts_with("amd_fidelityfx_"));
    match (dlss, fsr, xess) {
        (true, false, false) => Some("DLSS"),
        (false, true, false) => Some("FSR"),
        (false, false, true) => Some("XeSS"),
        _ => None,
    }
}

fn folder_files(exe_path: &str) -> Vec<String> {
    std::path::Path::new(exe_path)
        .parent()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect())
        .unwrap_or_default()
}

/// `running` = lowercase names of running processes (for platform anti-cheats like FACEIT).
pub fn detect(pid: u32, running: &[String]) -> GameTech {
    let path = ac_core::process::process_path(pid);
    let exe = path
        .as_deref()
        .and_then(|p| p.rsplit(['\\', '/']).next())
        .map(str::to_lowercase)
        .unwrap_or_default();
    GameTech {
        fg: path.as_deref().map(folder_files).and_then(|f| fg_from_files(&f)),
        anticheat: anticheat::detect(&exe, path.as_deref(), running),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn fg_only_when_unambiguous() {
        assert_eq!(fg_from_files(&v(&["game.exe", "nvngx_dlssg.dll"])), Some("DLSS"));
        assert_eq!(fg_from_files(&v(&["game.exe", "amd_fidelityfx_dx12.dll"])), Some("FSR"));
        assert_eq!(fg_from_files(&v(&["nvngx_dlssg.dll", "amd_fidelityfx_dx12.dll"])), None);
        assert_eq!(fg_from_files(&v(&["d3d11.dll"])), None);
    }

    #[test]
    fn own_process_is_not_an_anticheat_game() {
        assert!(detect(std::process::id(), &[]).anticheat.is_none());
    }
}

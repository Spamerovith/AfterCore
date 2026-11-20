//! Anti-cheat recognition.
//!
//! AfterCore never injects into games and never reads their memory. For games protected by
//! an anti-cheat it additionally switches to a *safe mode*: no process handles beyond the
//! limited query used by Task Manager, no capture exclusion for the overlay (anti-cheat
//! screenshots must see that it only shows FPS), and no affinity / priority changes unless
//! the user explicitly allows them.
//!
//! Recognition is read-only: the game's exe name, files next to it and the names of running
//! anti-cheat service processes.

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AntiCheat {
    pub name: &'static str,
    /// Has a kernel-mode driver.
    pub kernel: bool,
}

const BATTLEYE: AntiCheat = AntiCheat { name: "BattlEye", kernel: true };
const EAC: AntiCheat = AntiCheat { name: "Easy Anti-Cheat", kernel: true };
const VAC: AntiCheat = AntiCheat { name: "VAC", kernel: false };
const VANGUARD: AntiCheat = AntiCheat { name: "Riot Vanguard", kernel: true };
const FACEIT: AntiCheat = AntiCheat { name: "FACEIT AC", kernel: true };
const ESEA: AntiCheat = AntiCheat { name: "ESEA", kernel: true };
const RICOCHET: AntiCheat = AntiCheat { name: "Ricochet", kernel: true };
const JAVELIN: AntiCheat = AntiCheat { name: "EA Javelin", kernel: true };
const GAMEGUARD: AntiCheat = AntiCheat { name: "nProtect GameGuard", kernel: true };
const XIGNCODE: AntiCheat = AntiCheat { name: "XIGNCODE3", kernel: true };
const ACE: AntiCheat = AntiCheat { name: "ACE (Tencent)", kernel: true };
const NETEASE: AntiCheat = AntiCheat { name: "NetEase Anti-Cheat", kernel: true };
const HOYO: AntiCheat = AntiCheat { name: "HoYoverse (mhyprot)", kernel: true };
const DENUVO: AntiCheat = AntiCheat { name: "Denuvo Anti-Cheat", kernel: true };
const EQU8: AntiCheat = AntiCheat { name: "EQU8", kernel: true };
const HYPERION: AntiCheat = AntiCheat { name: "Hyperion (Byfron)", kernel: false };
const PUNKBUSTER: AntiCheat = AntiCheat { name: "PunkBuster", kernel: false };

/// Every anti-cheat AfterCore recognises (for the settings page).
pub const KNOWN: [AntiCheat; 17] = [
    BATTLEYE, EAC, VAC, VANGUARD, FACEIT, ESEA, RICOCHET, JAVELIN, GAMEGUARD, XIGNCODE, ACE, NETEASE, HOYO, DENUVO, EQU8,
    HYPERION, PUNKBUSTER,
];

/// Games whose protection is known by exe name (lowercase).
const BY_EXE: &[(&str, AntiCheat)] = &[
    // Valve Anti-Cheat
    ("cs2.exe", VAC),
    ("csgo.exe", VAC),
    ("dota2.exe", VAC),
    ("tf_win64.exe", VAC),
    ("project8.exe", VAC),
    ("left4dead2.exe", VAC),
    // BattlEye
    ("gta5_enhanced.exe", BATTLEYE),
    ("gta5.exe", BATTLEYE),
    ("rdr2.exe", BATTLEYE),
    ("rainbowsix.exe", BATTLEYE),
    ("tslgame.exe", BATTLEYE),
    ("destiny2.exe", BATTLEYE),
    ("escapefromtarkov.exe", BATTLEYE),
    ("dayz_x64.exe", BATTLEYE),
    ("arma3_x64.exe", BATTLEYE),
    ("arma reforger.exe", BATTLEYE),
    // Easy Anti-Cheat
    ("discovery.exe", EAC), // THE FINALS
    ("r5apex.exe", EAC),
    ("r5apex_dx12.exe", EAC),
    ("fortniteclient-win64-shipping.exe", EAC),
    ("deadbydaylight-win64-shipping.exe", EAC),
    ("rustclient.exe", EAC),
    ("eldenring.exe", EAC),
    ("hunt.exe", EAC),
    ("enlisted.exe", EAC),
    // Riot Vanguard
    ("valorant-win64-shipping.exe", VANGUARD),
    ("league of legends.exe", VANGUARD),
    // Call of Duty
    ("cod.exe", RICOCHET),
    ("cod24-cod.exe", RICOCHET),
    ("blackops6.exe", RICOCHET),
    ("blackopscoldwar.exe", RICOCHET),
    ("modernwarfare.exe", RICOCHET),
    // EA
    ("bf2042.exe", JAVELIN),
    ("bf6.exe", JAVELIN),
    // HoYoverse
    ("genshinimpact.exe", HOYO),
    ("starrail.exe", HOYO),
    ("zenlesszonezero.exe", HOYO),
    // Roblox
    ("robloxplayerbeta.exe", HYPERION),
];

/// Files / folders next to the game (lowercase names).
const BY_FILE: &[(&str, AntiCheat)] = &[
    ("battleye", BATTLEYE),
    ("beclient_x64.dll", BATTLEYE),
    ("easyanticheat", EAC),
    ("easyanticheat_eos_setup.exe", EAC),
    ("start_protected_game.exe", EAC),
    ("eaanticheat", JAVELIN),
    ("eaanticheat.installer.exe", JAVELIN),
    ("gameguard", GAMEGUARD),
    ("xigncode", XIGNCODE),
    ("anticheatexpert", ACE),
    ("neacsafe", NETEASE),
    ("neacclient.exe", NETEASE),
    ("mhyprot2.sys", HOYO),
    ("mhyprot3.sys", HOYO),
    ("denuvo-anti-cheat", DENUVO),
    ("equ8", EQU8),
    ("pbsvc.exe", PUNKBUSTER),
];

/// Running service processes (lowercase) that reveal a platform anti-cheat for a game.
const BY_SERVICE: &[(&str, &[&str], AntiCheat)] = &[
    ("faceitservice.exe", &["cs2.exe", "csgo.exe"], FACEIT),
    ("faceit.exe", &["cs2.exe", "csgo.exe"], FACEIT),
    ("eseaclient.exe", &["cs2.exe", "csgo.exe"], ESEA),
    ("vgc.exe", &["valorant-win64-shipping.exe", "league of legends.exe"], VANGUARD),
];

fn files_near(exe_path: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut dir = exe_path.parent();
    // The game folder and at most two levels above (e.g. Game\Binaries\Win64\game.exe),
    // never climbing into shared folders that hold other games or user data.
    for _ in 0..3 {
        let Some(d) = dir else { break };
        let name = d.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if d.parent().is_none()
            || matches!(
                name.as_str(),
                "common" | "games" | "steamapps" | "appdata" | "local" | "temp" | "program files" | "program files (x86)"
            )
        {
            break;
        }
        if let Ok(rd) = std::fs::read_dir(d) {
            out.extend(rd.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()));
        }
        dir = d.parent();
    }
    out
}

/// `exe_lower` is the game's exe name; `running` are lowercase names of running processes.
pub fn detect(exe_lower: &str, exe_path: Option<&str>, running: &[String]) -> Option<AntiCheat> {
    // Platform anti-cheats layered over a game (FACEIT on top of VAC, …) win.
    for (svc, games, ac) in BY_SERVICE {
        if games.contains(&exe_lower) && running.iter().any(|r| r == svc) {
            return Some(*ac);
        }
    }
    if let Some((_, ac)) = BY_EXE.iter().find(|(e, _)| *e == exe_lower) {
        return Some(*ac);
    }
    if let Some(p) = exe_path {
        let path = Path::new(p);
        let files = files_near(path);
        // "<game>_BE.exe" launcher next to the game.
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !stem.is_empty() && files.iter().any(|f| *f == format!("{stem}_be.exe")) {
            return Some(BATTLEYE);
        }
        for (marker, ac) in BY_FILE {
            if files.iter().any(|f| f == marker) {
                return Some(*ac);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_games() {
        assert_eq!(detect("gta5_enhanced.exe", None, &[]).unwrap().name, "BattlEye");
        assert_eq!(detect("cs2.exe", None, &[]).unwrap().name, "VAC");
        assert_eq!(detect("cs2.exe", None, &["faceitservice.exe".into()]).unwrap().name, "FACEIT AC");
        assert_eq!(detect("discovery.exe", None, &[]).unwrap().name, "Easy Anti-Cheat");
        assert!(detect("notepad.exe", None, &[]).is_none());
    }

    #[test]
    fn folder_markers() {
        let dir = std::env::temp_dir().join("ac_test_root").join("common").join("Some Game");
        let _ = std::fs::create_dir_all(dir.join("EasyAntiCheat"));
        let exe = dir.join("SomeGame.exe");
        let _ = std::fs::write(&exe, b"");
        let ac = detect("somegame.exe", Some(&exe.to_string_lossy()), &[]);
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("ac_test_root"));
        assert_eq!(ac.unwrap().name, "Easy Anti-Cheat");
    }

    #[test]
    fn real_gta_folder_if_present() {
        let p = r"D:\SteamLibrary\steamapps\common\Grand Theft Auto V Enhanced\GTA5_Enhanced.exe";
        if Path::new(p).parent().is_some_and(|d| d.exists()) {
            assert_eq!(detect("unknown.exe", Some(p), &[]).unwrap().name, "BattlEye");
        }
    }
}

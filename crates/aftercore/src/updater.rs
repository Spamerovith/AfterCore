//! Updates from GitHub Releases (https://github.com/Spamerovith/AfterCore/releases).
//!
//! Uses the curl.exe that ships with Windows 10/11, so no HTTP library is bundled.
//! The new exe is downloaded next to the current one, the running exe is renamed to
//! AfterCore.old.exe (Windows allows renaming a running exe) and the new one takes its
//! place. It starts on the next launch or via "Перезапустить"; it then deletes the old file

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

const API: &str = "https://api.github.com/repos/Spamerovith/AfterCore/releases/latest";
const ASSET: &str = "AfterCore.exe";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    Downloading(String),
    /// Installed, waiting for a restart
    Ready(String),
    Error(String),
}

static STATE: Mutex<State> = Mutex::new(State::Idle);

pub fn state() -> State {
    STATE.lock().map(|s| s.clone()).unwrap_or(State::Idle)
}

fn set(s: State) {
    if let Ok(mut g) = STATE.lock() {
        *g = s;
    }
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn parse(v: &str) -> Vec<u32> {
    v.trim().trim_start_matches(['v', 'V']).split(['.', '-']).map_while(|p| p.parse().ok()).collect()
}

/// `tag` is newer than `cur` ("v1.2.0" > "1.1.9")
pub fn newer(tag: &str, cur: &str) -> bool {
    let (a, b) = (parse(tag), parse(cur));
    !a.is_empty() && a > b
}

fn curl(args: &[&str]) -> Result<Vec<u8>, String> {
    let exe = std::env::var("SystemRoot").map(|r| format!(r"{r}\System32\curl.exe")).unwrap_or_else(|_| "curl.exe".into());
    let out = std::process::Command::new(exe)
        .args(["-sSfL", "--max-time", "120", "-H", "User-Agent: AfterCore-updater"])
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())
}

/// Check GitHub; with `install` download and swap the exe. Blocking, call from a thread
pub fn check(install: bool) {
    if matches!(state(), State::Checking | State::Downloading(_) | State::Ready(_)) {
        return;
    }
    set(State::Checking);
    let r = (|| -> Result<State, String> {
        let body = curl(&[API])?;
        let v: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        let tag = v["tag_name"].as_str().ok_or("нет tag_name")?.to_string();
        if !newer(&tag, current()) {
            return Ok(State::UpToDate);
        }
        if !install {
            return Ok(State::Error(format!("доступна версия {tag}")));
        }
        let asset = v["assets"]
            .as_array()
            .and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(ASSET)))
            .ok_or("в релизе нет AfterCore.exe")?;
        let url = asset["browser_download_url"].as_str().ok_or("нет ссылки")?;
        let size = asset["size"].as_u64().unwrap_or(0);
        set(State::Downloading(tag.clone()));

        let exe = exe_path()?;
        let new = exe.with_file_name("AfterCore.new.exe");
        let old = exe.with_file_name("AfterCore.old.exe");
        let _ = std::fs::remove_file(&new);
        curl(&["-o", &new.to_string_lossy(), url])?;
        let data = std::fs::read(&new).map_err(|e| e.to_string())?;
        if !data.starts_with(b"MZ") || (size > 0 && data.len() as u64 != size) {
            let _ = std::fs::remove_file(&new);
            return Err("файл обновления повреждён".into());
        }
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old).map_err(|e| format!("не удалось заменить exe: {e}"))?;
        if let Err(e) = std::fs::rename(&new, &exe) {
            let _ = std::fs::rename(&old, &exe);
            return Err(format!("не удалось заменить exe: {e}"));
        }
        Ok(State::Ready(tag))
    })();
    set(r.unwrap_or_else(State::Error));
}

/// Background checks: shortly after start, then every 6 hours
pub fn spawn(enabled: impl Fn() -> bool + Send + 'static) {
    std::thread::Builder::new()
        .name("updater".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(20));
            loop {
                if enabled() {
                    check(true);
                }
                std::thread::sleep(Duration::from_secs(6 * 3600));
            }
        })
        .ok();
}

/// Start the new exe (it waits until this instance exits) and quit
pub fn restart(core: &crate::core::Core) {
    if let Ok(exe) = exe_path() {
        let _ = std::process::Command::new(exe).arg("--wait-instance").spawn();
    }
    core.shutdown();
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    #[test]
    fn versions() {
        assert!(super::newer("v1.1.0", "1.0.0"));
        assert!(super::newer("v1.0.10", "1.0.9"));
        assert!(!super::newer("v1.0.0", "1.0.0"));
        assert!(!super::newer("v0.9.0", "1.0.0"));
    }
}

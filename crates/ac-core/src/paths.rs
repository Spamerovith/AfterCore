//! Data directory resolution.
//!
//! AfterCore is portable: settings live in `<exe dir>\data`. If that directory is not
//! writable (e.g. the app was copied to Program Files) we fall back to `%APPDATA%\AfterCore`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write_probe");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

pub fn data_dir() -> &'static Path {
    DATA_DIR.get_or_init(|| {
        let portable = exe_dir().join("data");
        if writable(&portable) {
            return portable;
        }
        let appdata = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("AfterCore");
        let _ = std::fs::create_dir_all(&appdata);
        appdata
    })
}

pub fn config_file() -> PathBuf {
    data_dir().join("config.json")
}

pub fn bench_dir() -> PathBuf {
    let d = data_dir().join("benchmarks");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Folder where users can drop their own .ttf/.otf fonts.
pub fn user_fonts_dir() -> PathBuf {
    let d = data_dir().join("fonts");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn exports_dir() -> PathBuf {
    let d = data_dir().join("exports");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Atomic write: temp file + rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

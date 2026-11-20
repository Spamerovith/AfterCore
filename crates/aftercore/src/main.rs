#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! AfterCore - modern FPS counter, hardware monitor and in-game overlay.

mod core;
mod hotkeys;
mod tray;
mod ui;

use std::sync::Arc;

use ac_core::config::Config;
use ac_core::process;
use eframe::egui;

pub struct Args {
    pub tray: bool,
    pub no_elevate: bool,
    pub elevated_attempt: bool,
    pub selftest: Option<u64>,
    pub screenshot: Option<String>,
    pub page: Option<String>,
    pub desktop_preview: bool,
    pub keys_diag: bool,
}

fn parse_args() -> Args {
    let v: Vec<String> = std::env::args().skip(1).collect();
    let val = |k: &str| v.iter().position(|a| a == k).and_then(|i| v.get(i + 1)).cloned();
    Args {
        tray: v.iter().any(|a| a == "--tray" || a == "--minimized"),
        no_elevate: v.iter().any(|a| a == "--no-elevate"),
        elevated_attempt: v.iter().any(|a| a == "--elevated-attempt"),
        selftest: v.iter().any(|a| a == "--selftest").then(|| val("--selftest").and_then(|s| s.parse().ok()).unwrap_or(10)),
        screenshot: val("--screenshot"),
        page: val("--page"),
        desktop_preview: v.iter().any(|a| a == "--desktop-preview"),
        keys_diag: v.iter().any(|a| a == "--keys-diag"),
    }
}

fn attach_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

/// `AfterCore.exe --selftest [seconds]` - print what the frame counter sees (needs admin).
fn selftest(secs: u64) {
    attach_console();
    let tracker = Arc::new(ac_telemetry::FrameTracker::new());
    let mut report = String::new();
    report.push_str(&format!("AfterCore {} self-test, admin = {}\n", ac_core::APP_VERSION, process::is_elevated()));
    match ac_telemetry::etw::EtwSession::start(tracker.clone()) {
        Ok(s) => {
            report.push_str(&format!("ETW session OK, providers: {}\n", s.providers.join(", ")));
            std::thread::sleep(std::time::Duration::from_secs(secs));
            let list = tracker.presenting();
            report.push_str(&format!(
                "events: {}, presenting processes: {}\n",
                tracker.events.load(std::sync::atomic::Ordering::Relaxed),
                list.len()
            ));
            for (pid, fps, src) in list {
                let name = process::process_name(pid).unwrap_or_default();
                let st = tracker.stats(pid, 10.0, 10);
                report.push_str(&format!(
                    "  {pid:>6}  {name:<32} {fps:>7.1} FPS  [{}]  1% {:.1}\n",
                    src.label(),
                    st.map(|s| s.low1).unwrap_or(0.0)
                ));
            }
            s.stop();
        }
        Err(e) => report.push_str(&format!("ETW failed: {e:?}\n")),
    }
    println!("{report}");
    let _ = std::fs::write(ac_core::paths::data_dir().join("selftest.txt"), report);
}

fn single_instance() -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name = ac_core::win::wide("Local\\AfterCore.SingleInstance");
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
        if h.is_null() {
            return true;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Bring the running instance to front.
            use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW};
            let title = ac_core::win::wide("AfterCore");
            let w = FindWindowW(std::ptr::null(), title.as_ptr());
            if !w.is_null() {
                ShowWindow(w, SW_SHOW);
                ShowWindow(w, SW_RESTORE);
                SetForegroundWindow(w);
            }
            return false;
        }
        // Intentionally leak the handle: the mutex lives as long as the process.
        true
    }
}

/// build.ps1 moves a running exe aside as AfterCore.old.exe; the new version cleans it up.
fn remove_old_exe() {
    if let Ok(me) = std::env::current_exe() {
        let old = me.with_file_name("AfterCore.old.exe");
        if !me.file_name().is_some_and(|n| n.eq_ignore_ascii_case("AfterCore.old.exe")) {
            let _ = std::fs::remove_file(old);
        }
    }
}

fn main() {
    let args = parse_args();
    remove_old_exe();
    if let Some(secs) = args.selftest {
        selftest(secs);
        return;
    }

    let cfg = Config::load(&ac_core::paths::config_file());
    let elevated = process::is_elevated();

    if !elevated && cfg.general.auto_elevate && !args.no_elevate && !args.elevated_attempt && args.screenshot.is_none() {
        let mut fwd: Vec<String> = std::env::args().skip(1).collect();
        fwd.push("--elevated-attempt".into());
        if process::relaunch_elevated(&fwd.join(" ")) {
            return;
        }
        // UAC declined: continue with limited functionality.
    }

    if args.screenshot.is_none() && !single_instance() {
        return;
    }

    let start_hidden = args.tray || cfg.general.start_minimized;
    let core = core::Core::new(cfg, elevated);
    if args.desktop_preview {
        core.preview_on_desktop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if args.keys_diag {
        hotkeys::set_diag(true);
    }
    core.spawn_watcher();
    hotkeys::spawn(core.clone());
    ac_overlay::spawn(Arc::new(core::CoreOverlay(core.clone())));

    let icon = egui::IconData { rgba: ac_overlay::logo::logo_rgba(64), width: 64, height: 64 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AfterCore")
            .with_app_id("AfterCore")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([1040.0, 660.0])
            .with_decorations(false)
            .with_resizable(true)
            .with_visible(!(start_hidden && args.screenshot.is_none()))
            .with_icon(Arc::new(icon)),
        renderer: eframe::Renderer::Glow,
        vsync: true,
        centered: true,
        ..Default::default()
    };

    let core_for_app = core.clone();
    let res = eframe::run_native(
        "AfterCore",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, core_for_app, start_hidden, args)))),
    );
    core.shutdown();
    if let Err(e) = res {
        let msg = ac_core::win::wide(&format!("AfterCore не смог запустить интерфейс:\n{e}"));
        let title = ac_core::win::wide("AfterCore");
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                std::ptr::null_mut(),
                msg.as_ptr(),
                title.as_ptr(),
                windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
            );
        }
    }
}

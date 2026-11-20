//! Global hotkeys that work inside games.
//!
//! Two independent input paths, both running in AfterCore's own thread (nothing is injected
//! into the game and the key is never swallowed):
//! 1. a low-level keyboard hook (WH_KEYBOARD_LL) - reinstalled every few seconds because
//!    Windows silently removes LL hooks that answer too slowly while a game loads every core;
//! 2. polling the async key state of the bound keys (~100 Hz) - a fallback that does not
//!    depend on hooks at all (Raw Input is not usable here: winit owns the process-wide
//!    keyboard registration).
//!
//! Modifiers are tracked from the key events themselves; `GetAsyncKeyState` is only a second
//! opinion (it can read 0 while a fullscreen game has focus). The same action is debounced so
//! both paths never fire it twice.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use ac_core::hotkey::{Hotkey, HotkeyAction, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};
use parking_lot::{Mutex, RwLock};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetTimer, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER,
};

use crate::core::{Core, LogLevel};

static BINDS: RwLock<Vec<(Hotkey, HotkeyAction)>> = RwLock::new(Vec::new());
static PAUSED: AtomicBool = AtomicBool::new(false);
static TX: OnceLock<Mutex<Sender<(HotkeyAction, Hotkey)>>> = OnceLock::new();

#[derive(Default)]
struct KeyState {
    hook_down: HashSet<u32>,
    poll_down: HashSet<u32>,
    mods: u32,
    last_fire: Vec<(HotkeyAction, Option<Instant>)>,
}

static STATE: Mutex<Option<KeyState>> = Mutex::new(None);

#[derive(Clone, Copy, PartialEq)]
enum Path {
    Hook,
    Poll,
}

fn mod_bit(vk: u32) -> Option<u32> {
    match vk {
        0x10 | 0xA0 | 0xA1 => Some(MOD_SHIFT),
        0x11 | 0xA2 | 0xA3 => Some(MOD_CONTROL),
        0x12 | 0xA4 | 0xA5 => Some(MOD_ALT),
        0x5B | 0x5C => Some(MOD_WIN),
        _ => None,
    }
}

fn async_mods() -> u32 {
    let p = |vk: i32| unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 };
    let mut m = 0;
    if p(0x11) {
        m |= MOD_CONTROL;
    }
    if p(0x10) {
        m |= MOD_SHIFT;
    }
    if p(0x12) {
        m |= MOD_ALT;
    }
    if p(0x5B) || p(0x5C) {
        m |= MOD_WIN;
    }
    m
}

// ───────────────────────── Diagnostics ─────────────────────────
//
// Off by default. When enabled from the Binds page, every first key press (key code only,
// no text) is recorded with the path that saw it and what happened, so a user can check
// inside a game whether keys reach AfterCore at all. Written to data\keys-diagnostic.log,
// deleted when diagnostics are switched off.

#[derive(Clone, Debug)]
pub struct DiagEvt {
    pub time: String,
    pub path: &'static str,
    pub vk: u32,
    pub mods: u32,
    pub fg_pid: u32,
    pub result: String,
}

static DIAG_ON: AtomicBool = AtomicBool::new(false);
static DIAG_DIRTY: AtomicBool = AtomicBool::new(false);
static DIAG: Mutex<VecDeque<DiagEvt>> = Mutex::new(VecDeque::new());
pub static SEEN_HOOK: AtomicU64 = AtomicU64::new(0);
pub static SEEN_POLL: AtomicU64 = AtomicU64::new(0);

pub fn diag_enabled() -> bool {
    DIAG_ON.load(Ordering::Relaxed)
}

pub fn set_diag(on: bool) {
    DIAG_ON.store(on, Ordering::SeqCst);
    if on {
        let fg = foreground_pid();
        LAST_FG.store(fg, Ordering::Relaxed);
        diag_note(fg, "диагностика включена".into());
    } else {
        diag_note(foreground_pid(), "диагностика выключена".into());
        // Final flush; the file stays until the user clears it.
        DIAG_DIRTY.store(true, Ordering::SeqCst);
        flush_diag_file();
    }
    DIAG_DIRTY.store(true, Ordering::SeqCst);
}

/// Forget all records and delete the log file.
pub fn clear_diag() {
    DIAG.lock().clear();
    let _ = std::fs::remove_file(diag_file());
}

static LAST_FG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn foreground_pid() -> u32 {
    unsafe {
        let mut pid = 0u32;
        windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
            windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow(),
            &mut pid,
        );
        pid
    }
}

/// A non-key line (window switch, heartbeat…).
fn diag_note(fg_pid: u32, text: String) {
    let mut d = DIAG.lock();
    d.push_back(DiagEvt { time: now_ms(), path: "", vk: 0, mods: 0, fg_pid, result: text });
    while d.len() > 400 {
        d.pop_front();
    }
    DIAG_DIRTY.store(true, Ordering::Relaxed);
}

/// Called on the hotkey thread timer: notes window switches with the event counters,
/// so a game that blocks keyboard access shows up as "window active, counters frozen".
fn diag_tick() {
    if !DIAG_ON.load(Ordering::Relaxed) {
        return;
    }
    let fg = foreground_pid();
    if LAST_FG.swap(fg, Ordering::Relaxed) != fg {
        diag_note(
            fg,
            format!(
                "активное окно сменилось · событий: хук {} / опрос {}",
                SEEN_HOOK.load(Ordering::Relaxed),
                SEEN_POLL.load(Ordering::Relaxed)
            ),
        );
    }
    flush_diag_file();
}

pub fn diag_events() -> Vec<DiagEvt> {
    DIAG.lock().iter().cloned().collect()
}

fn diag_file() -> std::path::PathBuf {
    ac_core::paths::data_dir().join("keys-diagnostic.log")
}

fn now_ms() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut t = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    format!("{:02}:{:02}:{:02}.{:03}", t.wHour, t.wMinute, t.wSecond, t.wMilliseconds)
}

fn diag(path: Path, vk: u32, mods: u32, result: impl Into<String>) {
    if !DIAG_ON.load(Ordering::Relaxed) {
        return;
    }
    let fg_pid = foreground_pid();
    let mut d = DIAG.lock();
    d.push_back(DiagEvt {
        time: now_ms(),
        path: if path == Path::Hook { "хук" } else { "опрос" },
        vk,
        mods,
        fg_pid,
        result: result.into(),
    });
    while d.len() > 400 {
        d.pop_front();
    }
    DIAG_DIRTY.store(true, Ordering::Relaxed);
}

fn flush_diag_file() {
    if !DIAG_DIRTY.swap(false, Ordering::Relaxed) {
        return;
    }
    if DIAG.lock().is_empty() {
        return;
    }
    let mut out = format!(
        "AfterCore keys diagnostic · hook events {} · poll events {}
",
        SEEN_HOOK.load(Ordering::Relaxed),
        SEEN_POLL.load(Ordering::Relaxed)
    );
    for e in diag_events() {
        let app = ac_core::process::process_name(e.fg_pid).unwrap_or_else(|| format!("pid {}", e.fg_pid));
        let key = if e.vk == 0 { "-".to_string() } else { Hotkey::new(e.mods, e.vk).label() };
        out.push_str(&format!("{}  {:<5}  {:<22}  окно: {:<28}  {}
", e.time, e.path, key, app, e.result));
    }
    let _ = std::fs::write(diag_file(), out);
}

/// Common key handler for both input paths.
fn on_key(path: Path, vk: u32, down: bool) {
    match path {
        Path::Hook => SEEN_HOOK.fetch_add(1, Ordering::Relaxed),
        Path::Poll => SEEN_POLL.fetch_add(1, Ordering::Relaxed),
    };
    let mut guard = STATE.lock();
    let st = guard.get_or_insert_with(KeyState::default);
    if let Some(bit) = mod_bit(vk) {
        if down {
            st.mods |= bit;
        } else {
            st.mods &= !bit;
        }
        return;
    }
    let set = match path {
        Path::Hook => &mut st.hook_down,
        Path::Poll => &mut st.poll_down,
    };
    if !down {
        set.remove(&vk);
        return;
    }
    if !set.insert(vk) {
        return; // auto-repeat
    }
    let tracked = st.mods;
    let polled = async_mods();
    if PAUSED.load(Ordering::Relaxed) {
        diag(path, vk, tracked, "пауза: идёт назначение бинда");
        return;
    }
    let found = BINDS.read().iter().find(|(h, _)| h.vk == vk && (h.mods == tracked || h.mods == polled)).copied();
    let Some((hk, action)) = found else {
        diag(path, vk, tracked, "нет бинда на это сочетание");
        return;
    };
    // Debounce: the hook and the poller both report the same press.
    let now = Instant::now();
    let slot = match st.last_fire.iter_mut().find(|(a, _)| *a == action) {
        Some(s) => s,
        None => {
            st.last_fire.push((action, None));
            st.last_fire.last_mut().unwrap()
        }
    };
    if slot.1.is_some_and(|t| now.duration_since(t) < Duration::from_millis(250)) {
        drop(guard);
        diag(path, vk, tracked, "уже сработал по другому каналу");
        return;
    }
    slot.1 = Some(now);
    drop(guard);
    diag(path, vk, tracked, format!("СРАБОТАЛ: {}", action.title()));
    if let Some(tx) = TX.get() {
        let _ = tx.lock().send((action, hk));
    }
}

unsafe extern "system" fn ll_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
        match wparam as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => on_key(Path::Hook, kb.vkCode, true),
            WM_KEYUP | WM_SYSKEYUP => on_key(Path::Hook, kb.vkCode, false),
            _ => {}
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

fn reload(core: &Core) {
    let map = core.cfg.read().hotkeys.clone();
    *BINDS.write() = map.into_iter().map(|(a, h)| (h, a)).collect();
    PAUSED.store(core.hotkeys_paused.load(Ordering::SeqCst), Ordering::SeqCst);
    core.hotkey_errors.write().clear();
}

unsafe fn install_hook() -> HHOOK {
    SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_proc), GetModuleHandleW(std::ptr::null()), 0)
}

pub fn spawn(core: Arc<Core>) {
    let (tx, rx) = channel::<(HotkeyAction, Hotkey)>();
    let _ = TX.set(Mutex::new(tx));

    // Actions run off the input thread so the hook always returns instantly.
    {
        let core = core.clone();
        std::thread::Builder::new()
            .name("hotkey-actions".into())
            .spawn(move || {
                while let Ok((a, hk)) = rx.recv() {
                    if core.quit.load(Ordering::Relaxed) {
                        break;
                    }
                    core.log(LogLevel::Info, format!("Бинд {}: {}", hk.label(), a.title()));
                    core.dispatch(a);
                }
            })
            .expect("spawn hotkey actions");
    }

    // Fallback path: poll the bound keys.
    {
        let core = core.clone();
        std::thread::Builder::new()
            .name("hotkey-poll".into())
            .spawn(move || {
                let pressed = |vk: u32| unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 };
                let mut was: HashSet<u32> = HashSet::new();
                while !core.quit.load(Ordering::Relaxed) {
                    let keys: Vec<u32> = BINDS.read().iter().map(|(h, _)| h.vk).collect();
                    for vk in keys {
                        let now = pressed(vk);
                        if now && was.insert(vk) {
                            on_key(Path::Poll, vk, true);
                        } else if !now && was.remove(&vk) {
                            on_key(Path::Poll, vk, false);
                        }
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
            .expect("spawn hotkey poll");
    }

    std::thread::Builder::new()
        .name("hotkeys".into())
        .spawn(move || unsafe {
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
            reload(&core);
            core.hotkey_reload.store(false, Ordering::SeqCst);
            let mut hook = install_hook();
            if hook.is_null() {
                core.log(LogLevel::Warn, "Перехват клавиатуры не установлен - бинды работают через опрос клавиш".into());
            }
            SetTimer(std::ptr::null_mut(), 0, 150, None);
            let mut last_rehook = Instant::now();
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                if msg.message == WM_TIMER && msg.hwnd.is_null() {
                    if core.quit.load(Ordering::Relaxed) {
                        break;
                    }
                    if core.hotkey_reload.swap(false, Ordering::SeqCst) {
                        reload(&core);
                    }
                    diag_tick();
                    // Windows may have dropped a slow hook without telling us: reinstall.
                    if last_rehook.elapsed() > Duration::from_secs(5) {
                        let fresh = install_hook();
                        if !fresh.is_null() {
                            if !hook.is_null() {
                                UnhookWindowsHookEx(hook);
                            }
                            hook = fresh;
                        }
                        last_rehook = Instant::now();
                    }
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if !hook.is_null() {
                UnhookWindowsHookEx(hook);
            }
        })
        .expect("spawn hotkeys");
}

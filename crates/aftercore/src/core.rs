//! Application core shared between the UI, the overlay thread, hotkeys, the tray and
//! the background watcher.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use ac_core::config::{Config, GameRule, Metric, OverlayConfig, OverlayLayout, Rgba, ShowMode, Tri};
use ac_core::cpu::{self, Topology};
use ac_core::hotkey::HotkeyAction;
use ac_core::process::{self, Rect};
use ac_overlay::{Cell, OverlayFrame, OverlaySource, Part};
use ac_telemetry::bottleneck::{Analyzer, Inputs, Level, Limiter, Verdict};
use ac_telemetry::etw::{EtwSession, EtwState};
use ac_telemetry::frames::summarize;
use ac_telemetry::gametech::{self, GameTech};
use ac_telemetry::{FrameStats, FrameTracker, SensorHub, SensorSnapshot};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Ok,
    Warn,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub level: LogLevel,
    pub time: String,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Target {
    pub pid: u32,
    pub exe: String,
    pub fullscreen: bool,
    pub window: Option<Rect>,
    pub monitor: Option<Rect>,
    pub dpi_scale: f32,
    pub profile: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BenchRecord {
    pub exe: String,
    pub api: String,
    pub when: String,
    pub frames: usize,
    pub duration_s: f32,
    pub avg_fps: f32,
    pub low1: f32,
    pub low01: f32,
    pub min_fps: f32,
    pub max_fps: f32,
    pub median_ms: f32,
    pub stutters: usize,
    pub csv: String,
}

pub struct Core {
    pub cfg: RwLock<Config>,
    /// Bumped when something other than the UI changes the config (hotkeys, tray).
    pub cfg_ext_gen: AtomicU64,
    pub topo: Topology,
    pub frames: Arc<FrameTracker>,
    pub sensors: Arc<SensorHub>,
    pub etw: Mutex<Option<EtwSession>>,
    pub etw_state: RwLock<EtwState>,
    pub elevated: bool,
    pub overlay_hidden: AtomicBool,
    pub preview_on_desktop: AtomicBool,
    pub target: RwLock<Target>,
    /// Last foreground app that rendered frames (what the dashboard shows).
    pub last_game: RwLock<Target>,
    pub log: Mutex<VecDeque<LogEntry>>,
    pub bench: RwLock<Vec<BenchRecord>>,
    pub hotkey_reload: AtomicBool,
    pub hotkeys_paused: AtomicBool,
    pub hotkey_errors: RwLock<Vec<HotkeyAction>>,
    pub quit: AtomicBool,
    pub hwnd: AtomicIsize,
    pub egui: OnceLock<eframe::egui::Context>,
    notice: Mutex<Option<(String, Rgba, Instant)>>,
    names: Mutex<HashMap<u32, String>>,
    applied: Mutex<HashMap<u32, String>>,
    tech: Mutex<HashMap<u32, (GameTech, Instant)>>,
    bottleneck: Mutex<BnState>,
    self_pid: u32,
}

impl Core {
    pub fn new(cfg: Config, elevated: bool) -> Arc<Self> {
        let sensors = SensorHub::start(cfg.general.sensor_interval_ms);
        let core = Arc::new(Self {
            cfg: RwLock::new(cfg),
            cfg_ext_gen: AtomicU64::new(0),
            topo: Topology::detect(),
            frames: Arc::new(FrameTracker::new()),
            sensors,
            etw: Mutex::new(None),
            etw_state: RwLock::new(EtwState::Stopped),
            elevated,
            overlay_hidden: AtomicBool::new(false),
            preview_on_desktop: AtomicBool::new(false),
            target: RwLock::new(Target::default()),
            last_game: RwLock::new(Target::default()),
            log: Mutex::new(VecDeque::new()),
            bench: RwLock::new(load_bench_index()),
            hotkey_reload: AtomicBool::new(true),
            hotkeys_paused: AtomicBool::new(false),
            hotkey_errors: RwLock::new(Vec::new()),
            quit: AtomicBool::new(false),
            hwnd: AtomicIsize::new(0),
            egui: OnceLock::new(),
            notice: Mutex::new(None),
            names: Mutex::new(HashMap::new()),
            applied: Mutex::new(HashMap::new()),
            tech: Mutex::new(HashMap::new()),
            bottleneck: Mutex::new(BnState::default()),
            self_pid: std::process::id(),
        });
        core.start_etw();
        core
    }

    pub fn start_etw(&self) {
        let mut slot = self.etw.lock();
        if slot.is_some() {
            return;
        }
        match EtwSession::start(self.frames.clone()) {
            Ok(s) => {
                let providers = s.providers.clone();
                self.log(LogLevel::Ok, format!("Счётчик кадров запущен (ETW: {})", providers.join(", ")));
                *self.etw_state.write() = EtwState::Running { providers };
                *slot = Some(s);
            }
            Err(state) => {
                match &state {
                    EtwState::NeedsAdmin => self.log(
                        LogLevel::Warn,
                        "Для счётчика FPS нужны права администратора - сенсоры работают и без них".into(),
                    ),
                    EtwState::Failed(e) => self.log(LogLevel::Warn, format!("ETW не запустился: {e}")),
                    _ => {}
                }
                *self.etw_state.write() = state;
            }
        }
    }

    pub fn shutdown(&self) {
        self.quit.store(true, Ordering::SeqCst);
        self.sensors.stop();
        if let Some(s) = self.etw.lock().take() {
            s.stop();
        }
        let _ = self.cfg.read().save(&ac_core::paths::config_file());
    }

    pub fn log(&self, level: LogLevel, text: String) {
        let (_, disp) = ac_core::win::local_time_strings();
        let time = disp.split(' ').nth(1).unwrap_or("").to_string();
        let mut l = self.log.lock();
        l.push_front(LogEntry { level, time, text });
        l.truncate(80);
        self.repaint();
    }

    pub fn repaint(&self) {
        if let Some(ctx) = self.egui.get() {
            ctx.request_repaint();
        }
    }

    pub fn notice(&self, text: impl Into<String>, color: Rgba) {
        if self.cfg.read().general.notifications {
            *self.notice.lock() = Some((text.into(), color, Instant::now()));
        }
    }

    pub fn save(&self) {
        if let Err(e) = self.cfg.read().save(&ac_core::paths::config_file()) {
            self.log(LogLevel::Warn, format!("Не удалось сохранить настройки: {e}"));
        }
    }

    /// Mutate config from outside the UI.
    pub fn edit_cfg(&self, f: impl FnOnce(&mut Config)) {
        f(&mut self.cfg.write());
        self.cfg_ext_gen.fetch_add(1, Ordering::SeqCst);
        self.save();
        self.repaint();
    }

    pub fn process_name(&self, pid: u32) -> String {
        if let Some(n) = self.names.lock().get(&pid) {
            return n.clone();
        }
        let n = process::process_name(pid)
            .or_else(|| process::list_processes().into_iter().find(|p| p.pid == pid).map(|p| p.name));
        match n {
            Some(n) => {
                self.names.lock().insert(pid, n.clone());
                n
            }
            None => format!("pid {pid}"),
        }
    }

    // ───────────────────────── Window ─────────────────────────

    pub fn show_launcher(&self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW};
        let h = self.hwnd.load(Ordering::Relaxed);
        if h != 0 {
            unsafe {
                ShowWindow(h as _, SW_SHOW);
                ShowWindow(h as _, SW_RESTORE);
                SetForegroundWindow(h as _);
            }
        }
        self.repaint();
    }

    pub fn hide_launcher(&self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
        let h = self.hwnd.load(Ordering::Relaxed);
        if h != 0 {
            unsafe { ShowWindow(h as _, SW_HIDE) };
        }
    }

    // ───────────────────────── Actions ─────────────────────────

    pub fn dispatch(&self, action: HotkeyAction) {
        match action {
            HotkeyAction::ToggleOverlay => {
                // Same switch as "Оверлей" in the launcher: the effective profile's `enabled`
                // (or the game rule's override, if the focused game forces it on/off).
                let profile = self.effective_profile();
                let exe = self.target.read().exe.to_lowercase();
                let mut now_on = false;
                self.edit_cfg(|c| now_on = toggle_overlay(c, &profile, &exe));
                self.overlay_hidden.store(false, Ordering::Relaxed);
                if now_on {
                    self.notice("Оверлей включён", ACCENT_NOTICE);
                }
            }
            HotkeyAction::CyclePosition => {
                let name = self.effective_profile();
                self.edit_cfg(|c| {
                    if let Some(p) = c.profiles.iter_mut().find(|p| p.name == name) {
                        p.overlay.anchor = p.overlay.anchor.next_corner();
                    }
                });
            }
            HotkeyAction::CycleLayout => {
                let name = self.effective_profile();
                let mut title = "";
                self.edit_cfg(|c| {
                    if let Some(p) = c.profiles.iter_mut().find(|p| p.name == name) {
                        p.overlay.layout = p.overlay.layout.next();
                        title = p.overlay.layout.title();
                    }
                });
                self.notice(format!("Вид: {title}"), ACCENT_NOTICE);
            }
            HotkeyAction::BenchToggle => self.bench_toggle(),
            HotkeyAction::NextProfile => {
                let mut name = String::new();
                self.edit_cfg(|c| name = c.next_profile());
                self.notice(format!("Профиль: {name}"), ACCENT_NOTICE);
                self.log(LogLevel::Info, format!("Активный профиль: {name}"));
            }
            HotkeyAction::ShowLauncher => self.show_launcher(),
        }
    }

    pub fn effective_profile(&self) -> String {
        let t = self.target.read();
        if !t.profile.is_empty() {
            return t.profile.clone();
        }
        self.cfg.read().active_profile.clone()
    }

    // ───────────────────────── Benchmark ─────────────────────────

    pub fn bench_toggle(&self) {
        if self.frames.is_recording().is_some() {
            self.bench_stop();
        } else {
            self.bench_start();
        }
    }

    pub fn bench_start(&self) {
        let g = self.last_game.read().clone();
        if g.pid == 0 {
            self.log(LogLevel::Warn, "Бенчмарк: нет активной игры".into());
            self.notice("Нет активной игры", BAD_NOTICE);
            return;
        }
        match self.frames.start_recording(g.pid) {
            Ok(src) => {
                self.log(LogLevel::Info, format!("Бенчмарк начат: {} ({})", g.exe, src.label()));
            }
            Err(e) => {
                self.log(LogLevel::Warn, format!("Бенчмарк: {e}"));
                self.notice("Игра не выводит кадры", BAD_NOTICE);
            }
        }
        self.repaint();
    }

    pub fn bench_stop(&self) {
        let Some((pid, src, fts)) = self.frames.stop_recording() else { return };
        if fts.len() < 10 {
            self.log(LogLevel::Warn, "Бенчмарк отменён: слишком мало кадров".into());
            return;
        }
        let s = summarize(&fts);
        let exe = self.process_name(pid);
        let (file_ts, disp) = ac_core::win::local_time_strings();
        let mut csv_path = String::new();
        if self.cfg.read().bench.save_csv {
            let stem = exe.trim_end_matches(".exe").trim_end_matches(".EXE");
            let path = ac_core::paths::bench_dir().join(format!("{stem}_{file_ts}.csv"));
            let mut out = String::with_capacity(fts.len() * 24);
            out.push_str("frame,time_ms,frametime_ms,fps\n");
            let mut t = 0.0f64;
            for (i, f) in fts.iter().enumerate() {
                t += *f as f64;
                out.push_str(&format!("{},{:.3},{:.3},{:.1}\n", i + 1, t, f, if *f > 0.0 { 1000.0 / f } else { 0.0 }));
            }
            if std::fs::write(&path, out).is_ok() {
                csv_path = path.to_string_lossy().to_string();
            }
        }
        let rec = BenchRecord {
            exe: exe.clone(),
            api: src.label().into(),
            when: disp,
            frames: s.frames,
            duration_s: s.duration_s,
            avg_fps: s.avg_fps,
            low1: s.low1,
            low01: s.low01,
            min_fps: s.min_fps,
            max_fps: s.max_fps,
            median_ms: s.median_ms,
            stutters: s.stutters,
            csv: csv_path,
        };
        self.bench.write().insert(0, rec);
        save_bench_index(&self.bench.read());
        self.log(
            LogLevel::Ok,
            format!("Бенчмарк {exe}: {:.0} FPS, 1% {:.0}, 0.1% {:.0} ({:.0} c)", s.avg_fps, s.low1, s.low01, s.duration_s),
        );
        self.notice(format!("AVG {:.0} · 1% {:.0} · 0.1% {:.0}", s.avg_fps, s.low1, s.low01), GOOD_NOTICE);
        self.repaint();
    }

    pub fn delete_bench(&self, idx: usize) {
        let mut b = self.bench.write();
        if idx < b.len() {
            let r = b.remove(idx);
            if !r.csv.is_empty() {
                let _ = std::fs::remove_file(&r.csv);
            }
        }
        save_bench_index(&b);
    }

    // ───────────────────────── Game rules ─────────────────────────

    pub fn apply_rule_to(&self, pid: u32, rule: &GameRule) -> Vec<String> {
        // Protected games are never opened unless the user explicitly allowed it.
        if !self.cfg.read().general.tweak_anticheat_games {
            if let Some(ac) = self.tech_for(pid, &[]).anticheat {
                return vec![format!("не применено: игра под защитой {} (Настройки → Античиты)", ac.name)];
            }
        }
        let cfg = self.cfg.read();
        let mut done = Vec::new();
        if let Some(cpus) = self.topo.resolve_set(&rule.core_set, &cfg.core_sets) {
            match cpu::apply_cores(pid, &cpus, rule.cpu_mode, &self.topo) {
                Ok(()) => done.push(format!("ядра: {}", self.topo.set_label(&rule.core_set))),
                Err(e) => done.push(format!("ядра не применены ({e})")),
            }
        }
        if let Some(p) = rule.priority {
            match cpu::set_priority(pid, p) {
                Ok(()) => done.push(format!("приоритет: {}", p.title())),
                Err(e) => done.push(format!("приоритет не применён ({e})")),
            }
        }
        done
    }

    /// Runs every ~2 s on the watcher thread.
    fn watch_tick(&self) {
        self.frames.prune();
        let procs = process::list_processes();
        let running: Vec<String> = procs.iter().map(|p| p.name.to_lowercase()).collect();
        let game = self.last_game.read().pid;
        if game != 0 {
            self.tech_for(game, &running);
        }

        // Auto-stop benchmark.
        let dur = self.cfg.read().bench.duration_s;
        if dur > 0 {
            if let Some((_, secs)) = self.frames.is_recording() {
                if secs >= dur as f64 {
                    self.bench_stop();
                }
            }
        }

        // Apply core sets / priorities from game rules to newly started processes.
        let rules: Vec<GameRule> = self
            .cfg
            .read()
            .game_rules
            .iter()
            .filter(|r| r.enabled && (!r.core_set.is_empty() || r.priority.is_some()))
            .cloned()
            .collect();
        {
            let alive: Vec<u32> = procs.iter().map(|p| p.pid).collect();
            self.applied.lock().retain(|pid, _| alive.contains(pid));
            self.names.lock().retain(|pid, _| alive.contains(pid));
            self.tech.lock().retain(|pid, _| alive.contains(pid));
        }
        if rules.is_empty() {
            return;
        }
        let allow_ac = self.cfg.read().general.tweak_anticheat_games;
        for p in procs {
            let exe = p.name.to_lowercase();
            let Some(rule) = rules.iter().find(|r| r.exe == exe) else { continue };
            let sig = format!("{}|{:?}|{:?}", rule.core_set, rule.cpu_mode, rule.priority);
            if self.applied.lock().get(&p.pid) == Some(&sig) {
                continue;
            }
            if let Some(ac) = self.tech_for(p.pid, &running).anticheat.filter(|_| !allow_ac) {
                self.applied.lock().insert(p.pid, sig);
                self.log(
                    LogLevel::Warn,
                    format!("{}: защищён {}, ядра и приоритет не меняю (безопасный режим)", p.name, ac.name),
                );
                continue;
            }
            let done = self.apply_rule_to(p.pid, rule);
            self.applied.lock().insert(p.pid, sig);
            if !done.is_empty() {
                self.log(LogLevel::Ok, format!("{} (pid {}): {}", p.name, p.pid, done.join(", ")));
            }
        }
    }

    /// Frame generation / anti-cheat of a process (cached, refreshed by the watcher).
    pub fn tech(&self, pid: u32) -> Option<GameTech> {
        (pid != 0).then(|| self.tech.lock().get(&pid).map(|t| t.0.clone())).flatten()
    }

    /// Anti-cheat protecting this process, if any.
    /// Bottleneck verdict for `pid`, advanced once per sensor tick.
    pub fn verdict(&self, pid: u32, sensors: &SensorSnapshot, stats: Option<&FrameStats>) -> Verdict {
        let mut b = self.bottleneck.lock();
        if b.pid != pid {
            *b = BnState { pid, ..Default::default() };
        }
        if sensors.updated.is_some() && sensors.updated != b.tick {
            b.tick = sensors.updated;
            b.verdict = b.analyzer.update(&bn_inputs(sensors, stats));
        }
        b.verdict.clone()
    }

    pub fn anticheat(&self, pid: u32) -> Option<ac_core::anticheat::AntiCheat> {
        self.tech(pid).and_then(|t| t.anticheat)
    }

    /// Detect (read-only: exe path + files next to it) and cache for 10 s.
    pub fn tech_for(&self, pid: u32, running: &[String]) -> GameTech {
        if let Some((t, at)) = self.tech.lock().get(&pid) {
            if at.elapsed() < Duration::from_secs(10) {
                return t.clone();
            }
        }
        let t = gametech::detect(pid, running);
        if let Some(ac) = t.anticheat {
            let known = self.tech.lock().get(&pid).is_some_and(|(o, _)| o.anticheat.is_some());
            if !known {
                self.log(LogLevel::Info, format!("{}: {} - безопасный режим", self.process_name(pid), ac.name));
            }
        }
        self.tech.lock().insert(pid, (t.clone(), Instant::now()));
        t
    }

    pub fn forget_applied(&self) {
        self.applied.lock().clear();
    }

    pub fn spawn_watcher(self: &Arc<Self>) {
        let core = self.clone();
        std::thread::Builder::new()
            .name("watcher".into())
            .spawn(move || {
                while !core.quit.load(Ordering::Relaxed) {
                    core.watch_tick();
                    for _ in 0..20 {
                        if core.quit.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            })
            .expect("spawn watcher");
    }

    // ───────────────────────── Overlay model ─────────────────────────

    fn resolve_target(&self, cfg: &Config) -> Option<Target> {
        let fg = process::foreground()?;
        if fg.pid == self.self_pid {
            return None;
        }
        let exe = self.process_name(fg.pid);
        let lower = exe.to_lowercase();
        let rule = cfg.rule_for(&lower);
        let profile = rule
            .filter(|r| !r.profile.is_empty() && cfg.profile(&r.profile).is_some())
            .map(|r| r.profile.clone())
            .unwrap_or_default();
        let dpi = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(fg.hwnd as _) };
        Some(Target {
            pid: fg.pid,
            exe,
            fullscreen: fg.fullscreen,
            window: Some(fg.client),
            monitor: Some(fg.monitor),
            dpi_scale: if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 },
            profile,
        })
    }

    pub fn current_notice(&self) -> Option<(String, Rgba)> {
        let n = self.notice.lock();
        n.as_ref().filter(|(_, _, at)| at.elapsed() < Duration::from_millis(3500)).map(|(t, c, _)| (t.clone(), *c))
    }
}

/// Flip the overlay switch the user sees: the game rule override if the focused game forces
/// one, otherwise the profile's "Оверлей включён". Returns the new state.
pub fn toggle_overlay(c: &mut Config, profile: &str, exe_lower: &str) -> bool {
    if let Some(r) = c.game_rules.iter_mut().find(|r| r.enabled && r.exe == exe_lower && r.overlay != Tri::Inherit) {
        r.overlay = if r.overlay == Tri::On { Tri::Off } else { Tri::On };
        return r.overlay == Tri::On;
    }
    let i = c.profiles.iter().position(|p| p.name == profile).unwrap_or_else(|| c.active_index());
    let p = &mut c.profiles[i];
    p.overlay.enabled = !p.overlay.enabled;
    p.overlay.enabled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_toggles_the_visible_switch() {
        let mut c = Config::default();
        c.active_mut().overlay.enabled = false; // switched off in the launcher
        let name = c.active_profile.clone();
        assert!(toggle_overlay(&mut c, &name, "gta5_enhanced.exe"), "bind must turn a disabled overlay on");
        assert!(c.active().overlay.enabled);
        assert!(!toggle_overlay(&mut c, &name, "gta5_enhanced.exe"));
        assert!(!c.active().overlay.enabled);
    }

    #[test]
    fn hotkey_respects_game_rule_override() {
        let mut c = Config::default();
        c.game_rules.push(GameRule { exe: "game.exe".into(), overlay: Tri::On, ..Default::default() });
        let name = c.active_profile.clone();
        assert!(!toggle_overlay(&mut c, &name, "game.exe"));
        assert_eq!(c.game_rules[0].overlay, Tri::Off);
        assert!(c.active().overlay.enabled, "profile untouched");
    }
}

pub const ACCENT_NOTICE: Rgba = [167, 139, 250, 255];
pub const GOOD_NOTICE: Rgba = [74, 222, 128, 255];
pub const BAD_NOTICE: Rgba = [248, 113, 113, 255];

pub fn fps_color(ov: &OverlayConfig, fps: f32) -> Rgba {
    let st = &ov.style;
    if !st.fps_colors {
        return st.accent;
    }
    if fps <= 0.0 {
        st.text
    } else if fps < st.fps_bad {
        st.bad
    } else if fps < st.fps_warn {
        st.warn
    } else {
        st.good
    }
}

/// Section label colours for the Classic layout.
pub const C_FPS: Rgba = [255, 120, 190, 255];
pub const C_GPU: Rgba = [118, 214, 106, 255];
pub const C_CPU: Rgba = [64, 160, 255, 255];
pub const C_APP: Rgba = [190, 140, 255, 255];
pub const C_TIME: Rgba = [210, 210, 215, 255];

/// Live inputs for building overlay cells.
pub struct LiveData<'a> {
    pub exe: &'a str,
    pub stats: Option<&'a FrameStats>,
    pub sensors: &'a SensorSnapshot,
    pub tech: Option<&'a GameTech>,
    pub recording: Option<f64>,
    pub notice: Option<(String, Rgba)>,
    /// Smoothed bottleneck verdict; `None` = evaluate this frame alone (launcher preview).
    pub verdict: Option<&'a Verdict>,
}

fn opt(v: Option<f32>, prec: usize) -> String {
    v.map(|v| format!("{v:.prec$}")).unwrap_or_else(|| "-".into())
}

/// Build overlay cells from configuration + live data (shared with the UI preview).
pub fn build_cells(ov: &OverlayConfig, d: &LiveData) -> Vec<Cell> {
    let st = &ov.style;
    let s = d.sensors;
    let stats = d.stats;
    let classic = ov.layout == OverlayLayout::Classic;
    let on = |m: Metric| ov.is_on(m);
    let col = |c: Rgba| if classic { c } else { st.label };
    let enabled: Vec<Metric> = ov.items.iter().filter(|i| i.enabled).map(|i| i.metric).collect();
    let mut title: Option<Cell> = None;
    let mut fps_cell: Option<Cell> = None;
    let mut frames: Option<Cell> = None;
    let mut body: Vec<Cell> = Vec::new();
    let mut graphs: Vec<Cell> = Vec::new();
    let mut done: Vec<&'static str> = Vec::new();
    let mut strip: Option<Cell> = None;

    for m in enabled.iter().copied() {
        let group = match m {
            Metric::FrameTime | Metric::Low1 | Metric::Low01 | Metric::FpsAvg | Metric::FpsMax => Some("frames"),
            Metric::GpuLoad | Metric::GpuTemp | Metric::GpuClock | Metric::GpuPower | Metric::GpuFan => Some("gpu"),
            Metric::CpuLoad
            | Metric::CpuClock
            | Metric::CpuMaxCore
            | Metric::CpuTemp
            | Metric::CpuPower
            | Metric::CpuVoltage
            | Metric::CpuFan => Some("cpu"),
            Metric::ProcCpu | Metric::ProcRam => Some("app"),
            _ => None,
        };
        if let Some(g) = group {
            if done.contains(&g) {
                continue;
            }
            done.push(g);
        }
        match m {
            Metric::Process => {
                let name = d.exe.trim_end_matches(".exe").trim_end_matches(".EXE").to_string();
                let tag = stats.and_then(|f| f.source).map(|s| s.label().to_string()).unwrap_or_default();
                title = Some(Cell::Title { name: if name.is_empty() { "AfterCore".into() } else { name }, tag });
            }
            Metric::Fps => {
                let fps = stats.map(|f| f.fps).unwrap_or(0.0);
                let fg = stats.filter(|f| ov.show_real_fps && f.frame_generation());
                fps_cell = Some(Cell::Fps {
                    value: if fps > 0.0 { format!("{fps:.0}") } else { "-".into() },
                    color: fps_color(ov, fps),
                    real: fg.and_then(|f| f.real_fps).map(|r| format!("{r:.0}")),
                    unit: if fg.is_some() { d.tech.and_then(|t| t.fg).unwrap_or("FG").to_string() } else { "FPS".into() },
                });
            }
            Metric::FrameTime | Metric::Low1 | Metric::Low01 | Metric::FpsAvg | Metric::FpsMax => {
                let v = |f: fn(&FrameStats) -> f32| opt(stats.map(f).filter(|v| *v > 0.0), 0);
                let mut parts = Vec::new();
                for m in enabled.iter() {
                    match m {
                        Metric::FrameTime => {
                            parts.push(Part::new(opt(stats.map(|f| f.frametime_ms).filter(|v| *v > 0.0), 1), "ms"))
                        }
                        Metric::FpsAvg => parts.push(Part::new(v(|f| f.avg_fps), "avg")),
                        Metric::Low1 => parts.push(Part::new(v(|f| f.low1), "1% low")),
                        Metric::Low01 => parts.push(Part::new(v(|f| f.low01), "0.1% low")),
                        Metric::FpsMax => parts.push(Part::new(v(|f| f.max_fps), "max")),
                        _ => {}
                    }
                }
                frames = Some(Cell::Section { title: String::new(), color: C_FPS, parts });
            }
            Metric::GpuLoad | Metric::GpuTemp | Metric::GpuClock | Metric::GpuPower | Metric::GpuFan => {
                let g = &s.gpu;
                let mut parts = Vec::new();
                for m in enabled.iter() {
                    match m {
                        Metric::GpuLoad => parts.push(Part::new(opt(g.load, 0), "%")),
                        Metric::GpuTemp => {
                            let t = g.temp.unwrap_or(0.0);
                            let c = if t >= 85.0 { Some(st.bad) } else if t >= 76.0 { Some(st.warn) } else { None };
                            parts.push(Part { value: opt(g.temp, 0), unit: "°C".into(), color: c });
                        }
                        Metric::GpuClock => parts.push(Part::new(opt(g.clock_mhz, 0), "MHz")),
                        Metric::GpuPower => parts.push(Part::new(opt(g.power_w, 0), "W")),
                        Metric::GpuFan => parts.push(Part::new(opt(g.fan_pct, 0), "% fan")),
                        _ => {}
                    }
                }
                body.push(Cell::Section { title: "GPU".into(), color: col(C_GPU), parts });
            }
            Metric::Vram => {
                let unit = s.gpu.vram_total_gb.map(|t| format!("/ {t:.0} GB")).unwrap_or_else(|| "GB".into());
                body.push(Cell::Section { title: "VRAM".into(), color: col(C_GPU), parts: vec![Part::new(opt(s.gpu.vram_used_gb, 1), unit)] });
            }
            Metric::CpuLoad
            | Metric::CpuClock
            | Metric::CpuMaxCore
            | Metric::CpuTemp
            | Metric::CpuPower
            | Metric::CpuVoltage
            | Metric::CpuFan => {
                let mut parts = Vec::new();
                for m in enabled.iter() {
                    match m {
                        Metric::CpuLoad => parts.push(Part::new(opt(s.cpu_load, 0), "%")),
                        Metric::CpuClock => parts.push(Part::new(opt(s.cpu_clock_mhz.map(|v| v / 1000.0), 2), "GHz")),
                        Metric::CpuMaxCore => {
                            let v = s.cpu_max_core.unwrap_or(0.0);
                            let c = if v >= 95.0 { Some(st.bad) } else if v >= 85.0 { Some(st.warn) } else { None };
                            parts.push(Part { value: opt(s.cpu_max_core, 0), unit: "% max core".into(), color: c });
                        }
                        // HWiNFO-only values: hidden instead of a dead "-" when it is not running.
                        Metric::CpuTemp if s.cpu_temp.is_some() => {
                            let t = s.cpu_temp.unwrap_or(0.0);
                            let c = if t >= 90.0 { Some(st.bad) } else if t >= 80.0 { Some(st.warn) } else { None };
                            parts.push(Part { value: opt(s.cpu_temp, 0), unit: "°C".into(), color: c });
                        }
                        Metric::CpuPower => parts.push(Part::new(opt(s.cpu_power_w, 0), "W")),
                        Metric::CpuVoltage if s.cpu_voltage.is_some() => parts.push(Part::new(opt(s.cpu_voltage, 3), "V")),
                        Metric::CpuFan if s.cpu_fan_rpm.is_some() => parts.push(Part::new(opt(s.cpu_fan_rpm, 0), "RPM")),
                        _ => {}
                    }
                }
                body.push(Cell::Section { title: "CPU".into(), color: col(C_CPU), parts });
            }
            Metric::Bottleneck => {
                let fresh;
                let v = match d.verdict {
                    Some(v) => v,
                    None => {
                        fresh = Analyzer::new().update(&bn_inputs(s, stats));
                        &fresh
                    }
                };
                strip = Some(Cell::Bottleneck { pills: bottleneck_pills(v, st) });
            }
            Metric::CpuCores => {
                if classic || ov.compact {
                    for (i, load) in s.per_cpu.iter().enumerate() {
                        let mut parts = vec![Part::new(format!("{load:.0}"), "%")];
                        if let Some(mhz) = s.per_cpu_mhz.get(i) {
                            parts.push(Part::new(format!("{mhz:.0}"), "MHz"));
                        }
                        body.push(Cell::Section { title: format!("CPU{}", i + 1), color: col(C_CPU), parts });
                    }
                } else {
                    let parts = s
                        .per_cpu
                        .iter()
                        .enumerate()
                        .map(|(i, l)| Part::new(format!("{l:.0}"), format!("% · {}", i + 1)))
                        .collect();
                    body.push(Cell::Section { title: "CPU CORES".into(), color: col(C_CPU), parts });
                }
            }
            Metric::Ram => body.push(Cell::Section {
                title: "RAM".into(),
                color: col(C_CPU),
                parts: vec![Part::new(format!("{:.1}", s.ram_used_gb), format!("/ {:.0} GB", s.ram_total_gb))],
            }),
            Metric::ProcCpu | Metric::ProcRam => {
                let t = s.target.as_ref();
                let mut parts = Vec::new();
                if on(Metric::ProcCpu) {
                    parts.push(Part::new(opt(t.map(|t| t.cpu_pct), 0), "% CPU"));
                }
                if on(Metric::ProcRam) {
                    parts.push(Part::new(opt(t.map(|t| t.ram_gb), 1), "GB RAM"));
                }
                body.push(Cell::Section { title: "GAME".into(), color: col(C_APP), parts });
            }
            Metric::Clock => body.push(Cell::Section {
                title: "TIME".into(),
                color: col(C_TIME),
                parts: vec![Part::new(ac_core::win::local_clock(), "")],
            }),
            Metric::Graph => {
                let ft = stats.map(|f| f.frametime_ms).unwrap_or(0.0);
                graphs.push(Cell::Graph {
                    title: "Frametime".into(),
                    value: if ft > 0.0 { format!("{ft:.1} ms") } else { "-".into() },
                    values: stats.map(|f| f.graph.clone()).unwrap_or_default(),
                    max: 0.0,
                    color: if classic { st.text } else { st.graph },
                });
            }
            Metric::GraphFps => {
                let h = stats.map(|f| f.fps_hist.clone()).unwrap_or_default();
                let peak = h.iter().cloned().fold(60.0, f32::max) * 1.15;
                graphs.push(Cell::Graph {
                    title: "Framerate".into(),
                    value: stats.map(|f| format!("{:.0} FPS", f.fps)).unwrap_or_else(|| "-".into()),
                    values: h,
                    max: peak,
                    color: if classic { C_FPS } else { st.accent },
                });
            }
            Metric::GraphGpu => graphs.push(Cell::Graph {
                title: "GPU usage".into(),
                value: format!("{} %", opt(s.gpu.load, 0)),
                values: s.gpu_hist.iter().copied().collect(),
                max: 100.0,
                color: C_GPU,
            }),
            Metric::GraphCpu => graphs.push(Cell::Graph {
                title: "CPU usage".into(),
                value: format!("{} %", opt(s.cpu_load, 0)),
                values: s.cpu_hist.iter().copied().collect(),
                max: 100.0,
                color: C_CPU,
            }),
            Metric::GraphRam => graphs.push(Cell::Graph {
                title: "RAM usage".into(),
                value: format!("{:.1} GB", s.ram_used_gb),
                values: s.ram_hist.iter().copied().collect(),
                max: 100.0,
                color: [96, 190, 255, 255],
            }),
        }
    }

    // Order: title, FPS, frame statistics, sections in the user's order, graphs at the bottom.
    let mut cells: Vec<Cell> = Vec::new();
    cells.extend(title);
    cells.extend(fps_cell);
    cells.extend(frames);
    cells.extend(body);
    cells.extend(graphs);
    cells.extend(strip);
    if let Some(secs) = d.recording {
        let s = secs as u64;
        cells.push(Cell::Badge { text: format!("REC {:02}:{:02}", s / 60, s % 60), color: st.bad });
    } else if let Some((text, color)) = d.notice.clone() {
        cells.push(Cell::Badge { text, color });
    }
    cells
}

fn bn_inputs<'a>(s: &SensorSnapshot, stats: Option<&'a FrameStats>) -> Inputs<'a> {
    let t = s.target.as_ref();
    Inputs {
        presenting: stats.is_some_and(|f| f.presenting && f.fps > 0.0),
        fps: stats.map_or(0.0, |f| f.fps),
        avg_fps: stats.map_or(0.0, |f| f.avg_fps),
        low1: stats.map_or(0.0, |f| f.low1),
        frametimes: stats.map_or(&[][..], |f| f.graph.as_slice()),
        refresh_hz: s.refresh_hz,
        gpu_util: s.gpu.load,
        gpu_throttle: s.gpu.throttle,
        vram_used_gb: s.gpu.vram_used_gb,
        vram_total_gb: s.gpu.vram_total_gb,
        game_shared_gb: t.and_then(|t| t.gpu_shared_gb),
        main_thread: t.and_then(|t| t.threads).map(|l| l.main_thread_pct),
        cores_busy: t.and_then(|t| t.threads).map(|l| l.cores_busy),
        ncpu: s.per_cpu.len(),
        cpu_total: s.cpu_load,
        max_core: s.cpu_max_core,
        hard_faults_s: t.and_then(|t| t.threads).map(|l| l.hard_faults_per_s),
        ram_used_gb: s.ram_used_gb,
        ram_total_gb: s.ram_total_gb,
    }
}

/// Bottleneck state per game; updated once per sensor tick.
#[derive(Default)]
pub struct BnState {
    pid: u32,
    tick: Option<Instant>,
    analyzer: Analyzer,
    verdict: Verdict,
}

fn bottleneck_pills(v: &Verdict, st: &ac_core::config::OverlayStyle) -> Vec<ac_overlay::Pill> {
    let mut out = Vec::new();
    if v.limiter == Some(Limiter::Cap) {
        out.push(ac_overlay::Pill {
            text: format!("CAP {:.0}", v.cap_fps.unwrap_or(0.0)),
            sub: v.detail.clone(),
            color: st.accent,
            bound: true,
        });
    }
    let items = [
        ("GPU", v.gpu, Limiter::Gpu),
        ("CPU", v.cpu, Limiter::Cpu),
        ("VRAM", v.vram, Limiter::Vram),
        ("RAM", v.ram, Limiter::Ram),
    ];
    for (name, r, lim) in items {
        let Some(pct) = r.pct else { continue };
        let bound = v.limiter == Some(lim);
        out.push(ac_overlay::Pill {
            text: format!("{name} {pct:.0}%"),
            sub: if v.detail.is_empty() { "bound".into() } else { format!("bound · {}", v.detail) },
            color: match r.level {
                Level::Ok => st.good,
                Level::Pressure => st.warn,
                Level::Limit => st.bad,
            },
            bound,
        });
    }
    out
}

pub fn effective_overlay<'a>(cfg: &'a Config, profile: &str) -> &'a OverlayConfig {
    cfg.profile(profile).map(|p| &p.overlay).unwrap_or(&cfg.active().overlay)
}

// ───────────────────────── Overlay source ─────────────────────────

pub struct CoreOverlay(pub Arc<Core>);

impl OverlaySource for CoreOverlay {
    fn interval(&self) -> Duration {
        let cfg = self.0.cfg.read();
        let p = self.0.target.read().profile.clone();
        Duration::from_millis(effective_overlay(&cfg, &p).refresh_ms as u64)
    }

    fn should_quit(&self) -> bool {
        self.0.quit.load(Ordering::Relaxed)
    }

    fn frame(&self) -> Option<OverlayFrame> {
        let core = &self.0;
        let cfg = core.cfg.read().clone();
        let target = core.resolve_target(&cfg);
        let preview = core.preview_on_desktop.load(Ordering::Relaxed);

        if let Some(t) = target.as_ref() {
            core.sensors.set_target(t.pid);
            *core.target.write() = t.clone();
        } else {
            core.target.write().pid = 0;
        }

        // Dashboard follows the last foreground app that renders frames.
        if let Some(t) = target.as_ref() {
            let lower = t.exe.to_lowercase();
            if !cfg.is_blacklisted(&lower) && core.frames.stats(t.pid, 1.0, 1).is_some_and(|s| s.presenting) {
                *core.last_game.write() = t.clone();
            }
        }

        let notice = core.current_notice();
        let recording = core.frames.is_recording().map(|(_, s)| s);

        if preview && !core.overlay_hidden.load(Ordering::Relaxed) {
            // Show on the monitor of the launcher with the last game's data.
            let g = core.last_game.read().clone();
            let ov = effective_overlay(&cfg, &core.effective_profile()).clone();
            let stats = core.frames.stats(g.pid, ov.low_window_s as f64, ov.graph_frames as usize);
            let sensors = core.sensors.get();
            let verdict = core.verdict(g.pid, &sensors, stats.as_ref());
            let area = target.as_ref().and_then(|t| t.monitor).or(g.monitor).unwrap_or_else(primary_monitor);
            let exe = if g.exe.is_empty() { "AfterCore".to_string() } else { g.exe.clone() };
            return Some(OverlayFrame {
                cells: build_cells(
                    &ov,
                    &LiveData {
                        exe: &exe,
                        stats: stats.as_ref(),
                        sensors: &sensors,
                        tech: core.tech(g.pid).as_ref(),
                        recording,
                        notice,
                        verdict: Some(&verdict),
                    },
                ),
                compact: ov.compact,
                scale: ov.scale * g.dpi_scale.max(1.0),
                style: ov.style.clone(),
                layout: ov.layout,
                graph_height: ov.graph_height,
                anchor: ov.anchor,
                offset: ov.offset,
                area,
                hide_from_capture: ov.hide_from_capture && core.anticheat(g.pid).is_none(),
            });
        }

        if core.overlay_hidden.load(Ordering::Relaxed) {
            return None;
        }
        let t = target?;
        let lower = t.exe.to_lowercase();
        if cfg.is_blacklisted(&lower) {
            return None;
        }
        let rule = cfg.rule_for(&lower);
        let ov = effective_overlay(&cfg, &t.profile);
        let enabled = match rule.map(|r| r.overlay).unwrap_or(Tri::Inherit) {
            Tri::On => true,
            Tri::Off => false,
            Tri::Inherit => ov.enabled,
        };
        if !enabled {
            return None;
        }
        let stats = core.frames.stats(t.pid, ov.low_window_s as f64, ov.graph_frames as usize);
        let presenting = stats.as_ref().is_some_and(|s| s.presenting);
        let show = match ov.show_mode {
            ShowMode::GamesOnly => presenting && (t.fullscreen || rule.is_some()),
            ShowMode::AnyPresenting => presenting,
            ShowMode::Always => true,
        };
        if !show {
            return None;
        }
        let sensors = core.sensors.get();
        let verdict = core.verdict(t.pid, &sensors, stats.as_ref());
        let area = match ov.anchor_target {
            ac_core::config::AnchorTarget::Window => t.window,
            ac_core::config::AnchorTarget::Monitor => t.monitor,
        }
        .or(t.monitor)?;
        Some(OverlayFrame {
            cells: build_cells(
                ov,
                &LiveData {
                    exe: &t.exe,
                    stats: stats.as_ref(),
                    sensors: &sensors,
                    tech: core.tech(t.pid).as_ref(),
                    recording,
                    notice,
                    verdict: Some(&verdict),
                },
            ),
            compact: ov.compact,
            scale: ov.scale * t.dpi_scale,
            style: ov.style.clone(),
            layout: ov.layout,
            graph_height: ov.graph_height,
            anchor: ov.anchor,
            offset: ov.offset,
            area,
            // Under an anti-cheat the overlay must stay visible to its screenshots.
            hide_from_capture: ov.hide_from_capture && core.anticheat(t.pid).is_none(),
        })
    }
}

pub fn primary_monitor() -> Rect {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    Rect { left: 0, top: 0, right: w, bottom: h }
}

// ───────────────────────── Bench index ─────────────────────────

fn bench_index_path() -> std::path::PathBuf {
    ac_core::paths::bench_dir().join("index.json")
}

fn load_bench_index() -> Vec<BenchRecord> {
    std::fs::read(bench_index_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_bench_index(list: &[BenchRecord]) {
    if let Ok(json) = serde_json::to_vec_pretty(list) {
        let _ = ac_core::paths::write_atomic(&bench_index_path(), &json);
    }
}

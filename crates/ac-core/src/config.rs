//! Persistent settings model.
//!
//! Two profile concepts:
//! * **Profile** - a named, switchable set of overlay settings (look + content + placement).
//! * **GameRule** - per-executable rule: which profile to use, overlay on/off, CPU core set,
//!   scheduling mode and priority. Applied automatically when the game runs / is focused.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::hotkey::{Hotkey, HotkeyAction, MOD_CONTROL, MOD_SHIFT};

pub type Rgba = [u8; 4];

pub const CONFIG_VERSION: u32 = 3;

// ───────────────────────────── UI ─────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiThemeKind {
    Midnight,
    Graphite,
    Amoled,
    Light,
}

impl UiThemeKind {
    pub const ALL: [UiThemeKind; 4] = [
        UiThemeKind::Midnight,
        UiThemeKind::Graphite,
        UiThemeKind::Amoled,
        UiThemeKind::Light,
    ];
    pub fn title(self) -> &'static str {
        match self {
            UiThemeKind::Midnight => "Midnight",
            UiThemeKind::Graphite => "Graphite",
            UiThemeKind::Amoled => "AMOLED",
            UiThemeKind::Light => "Daylight",
        }
    }
}

pub const ACCENTS: [(&str, Rgba); 8] = [
    ("Фиалка", [139, 92, 246, 255]),
    ("Лазурь", [56, 189, 248, 255]),
    ("Мята", [52, 211, 153, 255]),
    ("Лайм", [163, 230, 53, 255]),
    ("Янтарь", [251, 191, 36, 255]),
    ("Закат", [251, 113, 133, 255]),
    ("Пион", [236, 72, 153, 255]),
    ("Сапфир", [59, 130, 246, 255]),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    pub theme: UiThemeKind,
    pub accent: Rgba,
    /// Font display name; empty = Segoe UI Variable.
    pub font_name: String,
    /// Font file path; empty = default.
    pub font_path: String,
    pub scale: f32,
    pub animations: bool,
    pub glow: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            theme: UiThemeKind::Midnight,
            accent: ACCENTS[0].1,
            font_name: String::new(),
            font_path: String::new(),
            scale: 1.0,
            animations: true,
            glow: true,
        }
    }
}

// ─────────────────────────── Overlay ───────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverlayLayout {
    /// Vertical card with labels and a frametime graph.
    Card,
    /// One horizontal line - great for streams.
    Bar,
    /// Large FPS number only.
    Minimal,
    /// Classic OSD: coloured labels, aligned value columns, no panel.
    Classic,
}

impl OverlayLayout {
    pub const ALL: [OverlayLayout; 4] =
        [OverlayLayout::Card, OverlayLayout::Classic, OverlayLayout::Bar, OverlayLayout::Minimal];
    pub fn title(self) -> &'static str {
        match self {
            OverlayLayout::Card => "Карточка",
            OverlayLayout::Classic => "Классика",
            OverlayLayout::Bar => "Строка",
            OverlayLayout::Minimal => "Только FPS",
        }
    }
    pub fn next(self) -> Self {
        match self {
            OverlayLayout::Card => OverlayLayout::Classic,
            OverlayLayout::Classic => OverlayLayout::Bar,
            OverlayLayout::Bar => OverlayLayout::Minimal,
            OverlayLayout::Minimal => OverlayLayout::Card,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    Center,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Anchor {
    pub const GRID: [Anchor; 9] = [
        Anchor::TopLeft,
        Anchor::TopCenter,
        Anchor::TopRight,
        Anchor::MiddleLeft,
        Anchor::Center,
        Anchor::MiddleRight,
        Anchor::BottomLeft,
        Anchor::BottomCenter,
        Anchor::BottomRight,
    ];
    /// (horizontal, vertical) in 0.0 / 0.5 / 1.0
    pub fn factors(self) -> (f32, f32) {
        let i = Anchor::GRID.iter().position(|a| *a == self).unwrap_or(0);
        ((i % 3) as f32 * 0.5, (i / 3) as f32 * 0.5)
    }
    /// Clockwise through the four corners.
    pub fn next_corner(self) -> Self {
        match self {
            Anchor::TopLeft => Anchor::TopRight,
            Anchor::TopRight => Anchor::BottomRight,
            Anchor::BottomRight => Anchor::BottomLeft,
            _ => Anchor::TopLeft,
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Anchor::TopLeft => "Слева сверху",
            Anchor::TopCenter => "Сверху по центру",
            Anchor::TopRight => "Справа сверху",
            Anchor::MiddleLeft => "Слева по центру",
            Anchor::Center => "По центру",
            Anchor::MiddleRight => "Справа по центру",
            Anchor::BottomLeft => "Слева снизу",
            Anchor::BottomCenter => "Снизу по центру",
            Anchor::BottomRight => "Справа снизу",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnchorTarget {
    /// Inside the game window (works for windowed and borderless).
    Window,
    /// Relative to the monitor the game is on.
    Monitor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShowMode {
    /// Fullscreen / borderless 3D apps and apps with a game rule.
    GamesOnly,
    /// Any foreground app that presents frames (includes browsers, launchers…).
    AnyPresenting,
    /// Always visible (sensors only when nothing renders).
    Always,
}

impl ShowMode {
    pub const ALL: [ShowMode; 3] = [ShowMode::GamesOnly, ShowMode::AnyPresenting, ShowMode::Always];
    pub fn title(self) -> &'static str {
        match self {
            ShowMode::GamesOnly => "Только в играх",
            ShowMode::AnyPresenting => "В любом 3D-приложении",
            ShowMode::Always => "Всегда",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Metric {
    Process,
    Fps,
    FrameTime,
    Low1,
    Low01,
    Graph,
    GpuLoad,
    GpuTemp,
    GpuClock,
    GpuPower,
    GpuFan,
    Vram,
    CpuLoad,
    CpuClock,
    Ram,
    ProcCpu,
    ProcRam,
    Clock,
    FpsAvg,
    FpsMax,
    CpuCores,
    GraphFps,
    GraphGpu,
    GraphCpu,
    GraphRam,
    CpuTemp,
    CpuPower,
    CpuVoltage,
    CpuFan,
    CpuMaxCore,
    Bottleneck,
}

impl Metric {
    pub const ALL: [Metric; 31] = [
        Metric::Process,
        Metric::Fps,
        Metric::FrameTime,
        Metric::Low1,
        Metric::Low01,
        Metric::FpsAvg,
        Metric::FpsMax,
        Metric::GpuLoad,
        Metric::GpuTemp,
        Metric::GpuClock,
        Metric::GpuPower,
        Metric::GpuFan,
        Metric::Vram,
        Metric::CpuLoad,
        Metric::CpuClock,
        Metric::CpuMaxCore,
        Metric::CpuTemp,
        Metric::CpuPower,
        Metric::CpuVoltage,
        Metric::CpuFan,
        Metric::CpuCores,
        Metric::Ram,
        Metric::ProcCpu,
        Metric::ProcRam,
        Metric::Clock,
        Metric::Graph,
        Metric::GraphFps,
        Metric::GraphGpu,
        Metric::GraphCpu,
        Metric::GraphRam,
        Metric::Bottleneck,
    ];

    /// Added in config v3: CPU sensors next to the CPU clock, bottleneck at the bottom.
    pub const NEW_IN_V3: [Metric; 6] =
        [Metric::CpuMaxCore, Metric::CpuTemp, Metric::CpuPower, Metric::CpuVoltage, Metric::CpuFan, Metric::Bottleneck];

    pub fn is_graph(self) -> bool {
        matches!(self, Metric::Graph | Metric::GraphFps | Metric::GraphGpu | Metric::GraphCpu | Metric::GraphRam)
    }

    pub fn title(self) -> &'static str {
        match self {
            Metric::Process => "Имя игры и API",
            Metric::Fps => "FPS",
            Metric::FrameTime => "Frametime",
            Metric::Low1 => "1% low",
            Metric::Low01 => "0.1% low",
            Metric::FpsAvg => "Средний FPS",
            Metric::FpsMax => "Максимальный FPS",
            Metric::CpuCores => "Каждое ядро CPU",
            Metric::Graph => "График frametime",
            Metric::GraphFps => "График FPS",
            Metric::GraphGpu => "График загрузки GPU",
            Metric::GraphCpu => "График загрузки CPU",
            Metric::GraphRam => "График заполнения RAM",
            Metric::GpuLoad => "Загрузка GPU",
            Metric::GpuTemp => "Температура GPU",
            Metric::GpuClock => "Частота GPU",
            Metric::GpuPower => "Потребление GPU",
            Metric::GpuFan => "Вентилятор GPU",
            Metric::Vram => "Видеопамять",
            Metric::CpuLoad => "Загрузка CPU",
            Metric::CpuClock => "Частота CPU",
            Metric::Ram => "Оперативная память",
            Metric::ProcCpu => "CPU процесса игры",
            Metric::ProcRam => "RAM процесса игры",
            Metric::Clock => "Часы",
            Metric::CpuTemp => "Температура CPU",
            Metric::CpuPower => "Потребление CPU",
            Metric::CpuVoltage => "Напряжение CPU",
            Metric::CpuFan => "Вентилятор CPU",
            Metric::CpuMaxCore => "Max Core %",
            Metric::Bottleneck => "Узкое место",
        }
    }

    /// Short name for compact chips in the launcher.
    pub fn short(self) -> &'static str {
        match self {
            Metric::Process => "Игра и API",
            Metric::Fps => "FPS",
            Metric::FrameTime => "Frametime",
            Metric::Low1 => "1% low",
            Metric::Low01 => "0.1% low",
            Metric::FpsAvg => "Средний",
            Metric::FpsMax => "Максимум",
            Metric::GpuLoad | Metric::CpuLoad => "Загрузка",
            Metric::GpuTemp | Metric::CpuTemp => "Температура",
            Metric::GpuClock | Metric::CpuClock => "Частота",
            Metric::GpuPower | Metric::CpuPower => "Ватты",
            Metric::GpuFan | Metric::CpuFan => "Вентилятор",
            Metric::Vram => "VRAM",
            Metric::CpuVoltage => "Напряжение",
            Metric::CpuMaxCore => "Max Core %",
            Metric::CpuCores => "Каждое ядро",
            Metric::Ram => "RAM",
            Metric::ProcCpu => "CPU игры",
            Metric::ProcRam => "RAM игры",
            Metric::Clock => "Часы",
            Metric::Graph => "Frametime",
            Metric::GraphFps => "FPS",
            Metric::GraphGpu => "GPU",
            Metric::GraphCpu => "CPU",
            Metric::GraphRam => "RAM",
            Metric::Bottleneck => "Узкое место",
        }
    }

    /// Extra explanation shown on hover.
    pub fn hint(self) -> Option<&'static str> {
        match self {
            Metric::CpuTemp | Metric::CpuVoltage | Metric::CpuFan => Some(
                "Берётся из запущенного MSI Afterburner (датчик «CPU temperature» в мониторинге) или HWiNFO (Shared Memory Support). Свой драйвер AfterCore не ставит - античиты их не любят",
            ),
            Metric::CpuPower => Some("Счётчик Windows Energy Meter (Intel RAPL), без драйвера. Иначе - из HWiNFO"),
            Metric::CpuMaxCore => Some("Загрузка самого нагруженного потока. ~100% при низкой загрузке GPU - упор в CPU"),
            Metric::Bottleneck => Some(
                "Плашки GPU / CPU / VRAM / RAM. Красным помечается только то, что реально держит FPS: GPU занят ≥93% (с причиной - лимит питания/температуры); главный поток игры упёрся, пока GPU ждёт; видеопамять переполнена и игра вытесняется в ОЗУ со статтерами; игра читает память из файла подкачки со статтерами. Жёлтый - близко к пределу без влияния. CAP - FPS упёрся в V-Sync/лимит",
            ),
            Metric::FpsMax => Some("Лучшая секунда за окно статистики"),
            _ => None,
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            Metric::Process
            | Metric::Fps
            | Metric::FrameTime
            | Metric::Low1
            | Metric::Low01
            | Metric::FpsAvg
            | Metric::FpsMax => "Кадры",
            Metric::GpuLoad | Metric::GpuTemp | Metric::GpuClock | Metric::GpuPower | Metric::GpuFan | Metric::Vram => {
                "Видеокарта"
            }
            Metric::CpuLoad
            | Metric::CpuClock
            | Metric::CpuCores
            | Metric::CpuTemp
            | Metric::CpuPower
            | Metric::CpuVoltage
            | Metric::CpuFan
            | Metric::CpuMaxCore => "Процессор",
            Metric::Ram | Metric::ProcCpu | Metric::ProcRam | Metric::Clock => "Система и игра",
            Metric::Graph
            | Metric::GraphFps
            | Metric::GraphGpu
            | Metric::GraphCpu
            | Metric::GraphRam
            | Metric::Bottleneck => "Внизу оверлея",
        }
    }

    /// Group order for the launcher.
    pub const GROUPS: [&'static str; 5] = ["Кадры", "Видеокарта", "Процессор", "Система и игра", "Внизу оверлея"];

    pub fn is_bottom(self) -> bool {
        self.is_graph() || self == Metric::Bottleneck
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricItem {
    pub metric: Metric,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayStyle {
    pub font_name: String,
    pub font_path: String,
    pub font_size: f32,
    pub bg: Rgba,
    pub border: Rgba,
    pub text: Rgba,
    pub label: Rgba,
    pub accent: Rgba,
    pub good: Rgba,
    pub warn: Rgba,
    pub bad: Rgba,
    pub graph: Rgba,
    pub radius: f32,
    pub padding: f32,
    pub border_width: f32,
    pub shadow: bool,
    pub opacity: f32,
    pub fps_colors: bool,
    pub fps_warn: f32,
    pub fps_bad: f32,
}

impl Default for OverlayStyle {
    fn default() -> Self {
        builtin_styles().remove(0).style
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedStyle {
    pub name: String,
    pub style: OverlayStyle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    pub enabled: bool,
    pub layout: OverlayLayout,
    pub anchor: Anchor,
    pub anchor_target: AnchorTarget,
    pub offset: [i32; 2],
    pub scale: f32,
    pub items: Vec<MetricItem>,
    pub style: OverlayStyle,
    pub graph_frames: u32,
    pub graph_height: f32,
    pub refresh_ms: u32,
    pub low_window_s: u32,
    pub show_mode: ShowMode,
    pub hide_from_capture: bool,
    /// Tighter rows: label and values on one line, smaller gaps.
    pub compact: bool,
    /// With frame generation show the game's own FPS next to the presented FPS.
    pub show_real_fps: bool,
    /// Overlay labels in Russian (otherwise English, like RTSS)
    pub russian: bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        let on = [
            Metric::Process,
            Metric::Fps,
            Metric::FrameTime,
            Metric::Low1,
            Metric::Low01,
            Metric::FpsAvg,
            Metric::Graph,
            Metric::GpuLoad,
            Metric::GpuTemp,
            Metric::GpuClock,
            Metric::GpuPower,
            Metric::Vram,
            Metric::CpuLoad,
            Metric::CpuClock,
            Metric::CpuMaxCore,
            Metric::CpuTemp,
            Metric::CpuPower,
            Metric::Ram,
            Metric::Bottleneck,
        ];
        Self {
            enabled: true,
            layout: OverlayLayout::Card,
            anchor: Anchor::TopLeft,
            anchor_target: AnchorTarget::Window,
            offset: [16, 16],
            scale: 1.0,
            items: Metric::ALL
                .iter()
                .map(|m| MetricItem { metric: *m, enabled: on.contains(m) })
                .collect(),
            style: OverlayStyle::default(),
            graph_frames: 180,
            graph_height: 42.0,
            refresh_ms: 66,
            low_window_s: 10,
            show_mode: ShowMode::GamesOnly,
            hide_from_capture: false,
            compact: false,
            show_real_fps: true,
            russian: false,
        }
    }
}

impl OverlayConfig {
    /// Make sure every metric appears exactly once (configs from older versions).
    /// v2 → v3: put the new CPU metrics right after the CPU clock (enabled when the profile
    /// shows CPU load) and the bottleneck strip at the end.
    pub fn add_v3_metrics(&mut self) {
        let cpu_on = self.is_on(Metric::CpuLoad);
        let rich = cpu_on && self.is_on(Metric::GpuLoad);
        for m in Metric::NEW_IN_V3.iter().rev() {
            if self.items.iter().any(|i| i.metric == *m) {
                continue;
            }
            let enabled = match m {
                Metric::CpuMaxCore | Metric::CpuTemp | Metric::CpuPower => cpu_on,
                Metric::Bottleneck => rich,
                _ => false,
            };
            let item = MetricItem { metric: *m, enabled };
            if *m == Metric::Bottleneck {
                self.items.push(item);
            } else {
                let at = self.items.iter().position(|i| i.metric == Metric::CpuClock).map(|i| i + 1).unwrap_or(self.items.len());
                self.items.insert(at, item);
            }
        }
    }

    pub fn normalize(&mut self) {
        let mut seen = Vec::new();
        self.items.retain(|i| {
            if seen.contains(&i.metric) {
                false
            } else {
                seen.push(i.metric);
                true
            }
        });
        for m in Metric::ALL {
            if !seen.contains(&m) {
                self.items.push(MetricItem { metric: m, enabled: false });
            }
        }
        self.scale = self.scale.clamp(0.5, 3.0);
        self.refresh_ms = self.refresh_ms.clamp(16, 1000);
        self.graph_frames = self.graph_frames.clamp(30, 600);
        self.low_window_s = self.low_window_s.clamp(2, 120);
    }

    pub fn is_on(&self, m: Metric) -> bool {
        self.items.iter().any(|i| i.metric == m && i.enabled)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub overlay: OverlayConfig,
}

impl Default for Profile {
    fn default() -> Self {
        Self { name: "Стандартный".into(), overlay: OverlayConfig::default() }
    }
}

// ─────────────────────────── Game rules ───────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tri {
    Inherit,
    On,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CpuMode {
    /// Hard affinity mask - threads can run only on the selected cores.
    Affinity,
    /// Soft CPU Sets - the scheduler prefers selected cores (Windows 10+).
    CpuSets,
}

impl CpuMode {
    pub fn title(self) -> &'static str {
        match self {
            CpuMode::Affinity => "Жёсткая привязка (Affinity)",
            CpuMode::CpuSets => "Мягкая (CPU Sets)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
}

impl Priority {
    pub const ALL: [Priority; 5] =
        [Priority::Idle, Priority::BelowNormal, Priority::Normal, Priority::AboveNormal, Priority::High];
    pub fn title(self) -> &'static str {
        match self {
            Priority::Idle => "Низкий",
            Priority::BelowNormal => "Ниже среднего",
            Priority::Normal => "Обычный",
            Priority::AboveNormal => "Выше среднего",
            Priority::High => "Высокий",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameRule {
    pub exe: String,
    pub enabled: bool,
    /// Profile name to activate while this game is focused; empty = keep the active one.
    pub profile: String,
    pub overlay: Tri,
    /// `""` = do not touch, `"builtin:<id>"` = topology preset, otherwise a saved set name.
    pub core_set: String,
    pub cpu_mode: CpuMode,
    pub priority: Option<Priority>,
}

impl Default for GameRule {
    fn default() -> Self {
        Self {
            exe: String::new(),
            enabled: true,
            profile: String::new(),
            overlay: Tri::Inherit,
            core_set: String::new(),
            cpu_mode: CpuMode::Affinity,
            priority: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreSet {
    pub name: String,
    /// Logical processor indices.
    pub cpus: Vec<u32>,
}

// ─────────────────────────── General ───────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub autostart: bool,
    pub start_minimized: bool,
    pub close_to_tray: bool,
    pub minimize_to_tray: bool,
    pub auto_elevate: bool,
    pub sensor_interval_ms: u32,
    pub blacklist: Vec<String>,
    pub notifications: bool,
    /// Download new versions from GitHub Releases
    pub auto_update: bool,
    /// Allow core-set / priority changes for games protected by an anti-cheat.
    pub tweak_anticheat_games: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            autostart: false,
            start_minimized: false,
            close_to_tray: true,
            minimize_to_tray: false,
            auto_elevate: true,
            sensor_interval_ms: 500,
            blacklist: [
                "explorer.exe",
                "dwm.exe",
                "aftercore.exe",
                "searchhost.exe",
                "startmenuexperiencehost.exe",
                "shellexperiencehost.exe",
                "textinputhost.exe",
                "applicationframehost.exe",
                "steamwebhelper.exe",
                "msedgewebview2.exe",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            notifications: true,
            auto_update: true,
            tweak_anticheat_games: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BenchSettings {
    /// 0 = manual stop.
    pub duration_s: u32,
    pub save_csv: bool,
}

impl Default for BenchSettings {
    fn default() -> Self {
        Self { duration_s: 0, save_csv: true }
    }
}

// ─────────────────────────── Root ───────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub general: General,
    pub ui: UiSettings,
    pub hotkeys: BTreeMap<HotkeyAction, Hotkey>,
    pub bench: BenchSettings,
    pub core_sets: Vec<CoreSet>,
    pub profiles: Vec<Profile>,
    pub active_profile: String,
    pub game_rules: Vec<GameRule>,
    pub custom_styles: Vec<NamedStyle>,
}

pub fn default_hotkeys() -> BTreeMap<HotkeyAction, Hotkey> {
    let cs = MOD_CONTROL | MOD_SHIFT;
    let mut m = BTreeMap::new();
    m.insert(HotkeyAction::ToggleOverlay, Hotkey::new(cs, b'O' as u32));
    m.insert(HotkeyAction::CyclePosition, Hotkey::new(cs, b'P' as u32));
    m.insert(HotkeyAction::CycleLayout, Hotkey::new(cs, b'L' as u32));
    m.insert(HotkeyAction::BenchToggle, Hotkey::new(cs, b'B' as u32));
    m.insert(HotkeyAction::NextProfile, Hotkey::new(cs, b'N' as u32));
    m.insert(HotkeyAction::ShowLauncher, Hotkey::new(cs, b'A' as u32));
    m
}

impl Default for Config {
    fn default() -> Self {
        let standard = Profile::default();

        let mut stream = Profile { name: "Стрим".into(), overlay: OverlayConfig::default() };
        stream.overlay.layout = OverlayLayout::Bar;
        stream.overlay.anchor = Anchor::TopCenter;
        stream.overlay.style = builtin_styles()[2].style.clone();
        for it in stream.overlay.items.iter_mut() {
            it.enabled = matches!(
                it.metric,
                Metric::Fps | Metric::FrameTime | Metric::Low1 | Metric::GpuLoad | Metric::GpuTemp | Metric::CpuLoad
            );
        }

        let mut minimal = Profile { name: "Минимум".into(), overlay: OverlayConfig::default() };
        minimal.overlay.layout = OverlayLayout::Minimal;
        minimal.overlay.anchor = Anchor::TopRight;
        minimal.overlay.style = builtin_styles()[3].style.clone();
        for it in minimal.overlay.items.iter_mut() {
            it.enabled = matches!(it.metric, Metric::Fps | Metric::FrameTime);
        }

        Self {
            version: CONFIG_VERSION,
            general: General::default(),
            ui: UiSettings::default(),
            hotkeys: default_hotkeys(),
            bench: BenchSettings::default(),
            core_sets: Vec::new(),
            active_profile: standard.name.clone(),
            profiles: vec![standard, classic_profile(), stream, minimal],
            game_rules: Vec::new(),
            custom_styles: Vec::new(),
        }
    }
}

impl Config {
    pub fn load(path: &std::path::Path) -> Self {
        let mut cfg = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<Config>(&bytes).unwrap_or_else(|_| {
                // Keep a copy of the unreadable file instead of silently losing it.
                let _ = std::fs::copy(path, path.with_extension("broken.json"));
                Config::default()
            }),
            Err(_) => Config::default(),
        };
        cfg.normalize();
        cfg
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::paths::write_atomic(path, &json)
    }

    pub fn normalize(&mut self) {
        // v1 → v2: new "Классика" profile.
        if self.version < 2 && !self.profiles.iter().any(|p| p.name == "Классика") {
            self.profiles.push(classic_profile());
        }
        if self.version < 3 {
            for p in self.profiles.iter_mut() {
                p.overlay.add_v3_metrics();
            }
        }
        self.version = CONFIG_VERSION;
        if self.profiles.is_empty() {
            self.profiles.push(Profile::default());
        }
        for p in self.profiles.iter_mut() {
            p.overlay.normalize();
        }
        if !self.profiles.iter().any(|p| p.name == self.active_profile) {
            self.active_profile = self.profiles[0].name.clone();
        }
        self.ui.scale = self.ui.scale.clamp(0.75, 1.75);
        self.general.sensor_interval_ms = self.general.sensor_interval_ms.clamp(200, 5000);
        for r in self.game_rules.iter_mut() {
            r.exe = r.exe.trim().to_lowercase();
        }
    }

    pub fn active_index(&self) -> usize {
        self.profiles.iter().position(|p| p.name == self.active_profile).unwrap_or(0)
    }

    pub fn active(&self) -> &Profile {
        &self.profiles[self.active_index()]
    }

    pub fn active_mut(&mut self) -> &mut Profile {
        let i = self.active_index();
        &mut self.profiles[i]
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    pub fn rule_for(&self, exe_lower: &str) -> Option<&GameRule> {
        self.game_rules.iter().find(|r| r.enabled && r.exe == exe_lower)
    }

    pub fn is_blacklisted(&self, exe_lower: &str) -> bool {
        self.general.blacklist.iter().any(|b| b.eq_ignore_ascii_case(exe_lower))
    }

    /// Unique name derived from `base` (`base`, `base 2`, `base 3`, …).
    pub fn unique_profile_name(&self, base: &str) -> String {
        unique_name(base, |n| self.profiles.iter().any(|p| p.name == n))
    }

    pub fn unique_set_name(&self, base: &str) -> String {
        unique_name(base, |n| self.core_sets.iter().any(|p| p.name == n))
    }

    pub fn next_profile(&mut self) -> String {
        let i = (self.active_index() + 1) % self.profiles.len();
        self.active_profile = self.profiles[i].name.clone();
        self.active_profile.clone()
    }
}

/// Classic OSD look: coloured section labels, orange values, outlined text, graphs at the bottom.
pub fn classic_profile() -> Profile {
    let mut p = Profile { name: "Классика".into(), overlay: OverlayConfig::default() };
    p.overlay.layout = OverlayLayout::Classic;
    p.overlay.style = builtin_styles().into_iter().find(|s| s.name == "Classic").unwrap().style;
    p.overlay.offset = [6, 6];
    for it in p.overlay.items.iter_mut() {
        it.enabled = matches!(
            it.metric,
            Metric::Fps
                | Metric::FrameTime
                | Metric::GpuLoad
                | Metric::GpuTemp
                | Metric::GpuClock
                | Metric::GpuPower
                | Metric::Vram
                | Metric::CpuLoad
                | Metric::CpuClock
                | Metric::CpuCores
                | Metric::Ram
                | Metric::GraphGpu
                | Metric::GraphRam
                | Metric::Graph
        );
    }
    p
}

pub fn unique_name(base: &str, exists: impl Fn(&str) -> bool) -> String {
    let base = if base.trim().is_empty() { "Новый" } else { base.trim() };
    if !exists(base) {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base} {i}"))
        .find(|n| !exists(n))
        .unwrap()
}

// ─────────────────────── Built-in overlay styles ───────────────────────

pub fn builtin_styles() -> Vec<NamedStyle> {
    let base = OverlayStyle {
        font_name: String::new(),
        font_path: String::new(),
        font_size: 15.0,
        bg: [12, 12, 20, 200],
        border: [139, 92, 246, 110],
        text: [240, 240, 250, 255],
        label: [150, 150, 175, 255],
        accent: [167, 139, 250, 255],
        good: [74, 222, 128, 255],
        warn: [250, 204, 21, 255],
        bad: [248, 113, 113, 255],
        graph: [167, 139, 250, 255],
        radius: 12.0,
        padding: 12.0,
        border_width: 1.0,
        shadow: true,
        opacity: 1.0,
        fps_colors: true,
        fps_warn: 60.0,
        fps_bad: 30.0,
    };
    vec![
        NamedStyle { name: "Neon Violet".into(), style: base.clone() },
        NamedStyle {
            name: "Glass".into(),
            style: OverlayStyle {
                bg: [255, 255, 255, 38],
                border: [255, 255, 255, 70],
                text: [255, 255, 255, 255],
                label: [220, 225, 235, 255],
                accent: [255, 255, 255, 255],
                graph: [255, 255, 255, 230],
                radius: 16.0,
                ..base.clone()
            },
        },
        NamedStyle {
            name: "Cyber".into(),
            style: OverlayStyle {
                bg: [5, 10, 20, 215],
                border: [34, 211, 238, 140],
                text: [224, 250, 255, 255],
                label: [103, 232, 249, 255],
                accent: [34, 211, 238, 255],
                graph: [236, 72, 153, 255],
                radius: 4.0,
                ..base.clone()
            },
        },
        NamedStyle {
            name: "Minimal".into(),
            style: OverlayStyle {
                bg: [0, 0, 0, 0],
                border: [0, 0, 0, 0],
                border_width: 0.0,
                text: [255, 255, 255, 255],
                label: [210, 210, 210, 255],
                accent: [255, 255, 255, 255],
                graph: [255, 255, 255, 220],
                radius: 0.0,
                padding: 6.0,
                font_size: 16.0,
                ..base.clone()
            },
        },
        NamedStyle {
            name: "Terminal".into(),
            style: OverlayStyle {
                font_name: "Consolas".into(),
                font_path: "C:\\Windows\\Fonts\\consola.ttf".into(),
                bg: [0, 8, 0, 220],
                border: [34, 197, 94, 120],
                text: [134, 239, 172, 255],
                label: [34, 197, 94, 255],
                accent: [74, 222, 128, 255],
                graph: [74, 222, 128, 255],
                radius: 2.0,
                ..base.clone()
            },
        },
        NamedStyle {
            name: "Sunset".into(),
            style: OverlayStyle {
                bg: [30, 10, 20, 210],
                border: [251, 113, 133, 120],
                text: [255, 241, 242, 255],
                label: [253, 164, 175, 255],
                accent: [251, 146, 60, 255],
                graph: [251, 113, 133, 255],
                radius: 14.0,
                ..base.clone()
            },
        },
        NamedStyle {
            name: "Classic".into(),
            style: OverlayStyle {
                font_name: "Bahnschrift".into(),
                font_path: r"C:\Windows\Fonts\bahnschrift.ttf".into(),
                font_size: 17.0,
                bg: [0, 0, 0, 0],
                border: [0, 0, 0, 0],
                border_width: 0.0,
                text: [255, 160, 30, 255],
                label: [255, 196, 120, 255],
                accent: [255, 160, 30, 255],
                graph: [255, 160, 30, 255],
                radius: 0.0,
                padding: 4.0,
                fps_colors: false,
                ..base.clone()
            },
        },
        NamedStyle {
            name: "Без фона".into(),
            style: OverlayStyle {
                bg: [0, 0, 0, 0],
                border: [0, 0, 0, 0],
                border_width: 0.0,
                text: [255, 255, 255, 255],
                label: [205, 205, 220, 255],
                padding: 6.0,
                ..base
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v3_adds_cpu_metrics_after_clock() {
        let mut ov = OverlayConfig::default();
        ov.items.retain(|i| !Metric::NEW_IN_V3.contains(&i.metric));
        ov.add_v3_metrics();
        let pos = |m| ov.items.iter().position(|i| i.metric == m).unwrap();
        assert_eq!(pos(Metric::CpuMaxCore), pos(Metric::CpuClock) + 1);
        assert!(ov.is_on(Metric::CpuPower) && ov.is_on(Metric::Bottleneck));
        assert!(!ov.is_on(Metric::CpuVoltage));
        assert_eq!(ov.items.last().unwrap().metric, Metric::Bottleneck);
        ov.normalize();
        assert_eq!(ov.items.len(), Metric::ALL.len());
    }

    #[test]
    fn roundtrip_and_normalize() {
        let cfg = Config::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let mut back: Config = serde_json::from_str(&json).unwrap();
        back.normalize();
        assert_eq!(back.profiles.len(), 4);
        assert_eq!(back.active().name, "Стандартный");
        assert_eq!(back.hotkeys.len(), HotkeyAction::ALL.len());
    }

    #[test]
    fn missing_fields_use_defaults() {
        let back: Config = serde_json::from_str(r#"{"ui":{"scale":1.2}}"#).unwrap();
        assert_eq!(back.ui.scale, 1.2);
        assert!(back.general.auto_elevate);
    }

    #[test]
    fn unique_names() {
        let cfg = Config::default();
        assert_eq!(cfg.unique_profile_name("Стрим"), "Стрим 2");
        assert_eq!(cfg.unique_profile_name("Новый"), "Новый");
    }

    #[test]
    fn anchor_factors() {
        assert_eq!(Anchor::TopLeft.factors(), (0.0, 0.0));
        assert_eq!(Anchor::Center.factors(), (0.5, 0.5));
        assert_eq!(Anchor::BottomRight.factors(), (1.0, 1.0));
    }
}

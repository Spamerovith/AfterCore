//! Hotkey model. Registration itself lives in the app (RegisterHotKey thread).

use serde::{Deserialize, Serialize};

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hotkey {
    /// Combination of `MOD_*` flags.
    pub mods: u32,
    /// Win32 virtual-key code.
    pub vk: u32,
}

impl Hotkey {
    pub const fn new(mods: u32, vk: u32) -> Self {
        Self { mods, vk }
    }

    pub fn label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.mods & MOD_CONTROL != 0 {
            parts.push("Ctrl".into());
        }
        if self.mods & MOD_ALT != 0 {
            parts.push("Alt".into());
        }
        if self.mods & MOD_SHIFT != 0 {
            parts.push("Shift".into());
        }
        if self.mods & MOD_WIN != 0 {
            parts.push("Win".into());
        }
        parts.push(vk_name(self.vk));
        parts.join(" + ")
    }

    /// Function keys and a few others may be bound without modifiers;
    /// letters/digits need at least one modifier so typing is not hijacked.
    pub fn is_reasonable(&self) -> bool {
        let fkey = (0x70..=0x87).contains(&self.vk);
        let special = matches!(self.vk, 0x13 | 0x91 | 0x2D | 0x24 | 0x23 | 0x21 | 0x22 | 0x2C);
        self.mods != 0 || fkey || special
    }
}

pub fn vk_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).map(|c| c.to_string()).unwrap_or_default(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x60..=0x69 => format!("Num {}", vk - 0x60),
        0x6A => "Num *".into(),
        0x6B => "Num +".into(),
        0x6D => "Num -".into(),
        0x6E => "Num .".into(),
        0x6F => "Num /".into(),
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x13 => "Pause".into(),
        0x1B => "Esc".into(),
        0x20 => "Space".into(),
        0x21 => "PgUp".into(),
        0x22 => "PgDn".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x25 => "←".into(),
        0x26 => "↑".into(),
        0x27 => "→".into(),
        0x28 => "↓".into(),
        0x2C => "PrtSc".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x91 => "ScrollLock".into(),
        0xBA => ";".into(),
        0xBB => "=".into(),
        0xBC => ",".into(),
        0xBD => "-".into(),
        0xBE => ".".into(),
        0xBF => "/".into(),
        0xC0 => "`".into(),
        0xDB => "[".into(),
        0xDC => "\\".into(),
        0xDD => "]".into(),
        0xDE => "'".into(),
        other => format!("VK 0x{other:02X}"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HotkeyAction {
    ToggleOverlay,
    CyclePosition,
    CycleLayout,
    BenchToggle,
    NextProfile,
    ShowLauncher,
}

impl HotkeyAction {
    pub const ALL: [HotkeyAction; 6] = [
        HotkeyAction::ToggleOverlay,
        HotkeyAction::CyclePosition,
        HotkeyAction::CycleLayout,
        HotkeyAction::BenchToggle,
        HotkeyAction::NextProfile,
        HotkeyAction::ShowLauncher,
    ];

    pub fn title(self) -> &'static str {
        match self {
            HotkeyAction::ToggleOverlay => "Показать / скрыть оверлей",
            HotkeyAction::CyclePosition => "Сменить угол оверлея",
            HotkeyAction::CycleLayout => "Сменить вид оверлея",
            HotkeyAction::BenchToggle => "Старт / стоп бенчмарка",
            HotkeyAction::NextProfile => "Следующий профиль",
            HotkeyAction::ShowLauncher => "Открыть AfterCore",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            HotkeyAction::ToggleOverlay => "Мгновенно прячет оверлей поверх игры и возвращает его",
            HotkeyAction::CyclePosition => "Перемещает оверлей по углам экрана по часовой стрелке",
            HotkeyAction::CycleLayout => "Карточка → Строка → Только FPS",
            HotkeyAction::BenchToggle => "Запись frametime активной игры в CSV с итоговой статистикой",
            HotkeyAction::NextProfile => "Переключает профиль настроек по кругу",
            HotkeyAction::ShowLauncher => "Разворачивает окно AfterCore из трея",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            HotkeyAction::ToggleOverlay => "\u{E7B3}",
            HotkeyAction::CyclePosition => "\u{E740}",
            HotkeyAction::CycleLayout => "\u{E8A9}",
            HotkeyAction::BenchToggle => "\u{E7C1}",
            HotkeyAction::NextProfile => "\u{E8AB}",
            HotkeyAction::ShowLauncher => "\u{E737}",
        }
    }
}

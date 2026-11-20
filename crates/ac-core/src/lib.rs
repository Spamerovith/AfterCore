//! AfterCore core: everything that is not telemetry, rendering or UI.
//!
//! * [`config`]  - the persistent settings model (profiles, game rules, themes…)
//! * [`cpu`]     - CPU topology (P/E cores, SMT, L3 groups), presets, affinity / CPU sets
//! * [`hotkey`]  - hotkey model and key naming
//! * [`process`] - small Win32 helpers (processes, foreground window, elevation…)
//! * [`paths`]   - portable data directory resolution

pub mod anticheat;
pub mod config;
pub mod cpu;
pub mod hotkey;
pub mod paths;
pub mod process;
pub mod win;

pub const APP_NAME: &str = "AfterCore";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

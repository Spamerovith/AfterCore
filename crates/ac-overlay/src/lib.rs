//! AfterCore overlay.
//!
//! Unlike classic OSD tools that inject a DLL into the game and draw with the game's own
//! D3D/Vulkan device inside a hooked `Present`, AfterCore draws **outside** the game: a
//! top-most, click-through, per-pixel-alpha layered window is placed over the game window.
//! Content is rendered in software (tiny-skia + ab_glyph) - no GPU context, no injection,
//! nothing for anti-cheat to complain about.

pub mod fonts;
pub mod logo;
pub mod render;
pub mod window;

pub use render::{Cell, Part, Pill, Renderer, Scene, RUSSIAN};
pub use window::{spawn, OverlayFrame, OverlaySource};

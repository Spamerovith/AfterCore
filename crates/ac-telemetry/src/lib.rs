//! AfterCore telemetry.
//!
//! Frame timing is collected **without injecting anything into games**: the DirectX runtime
//! (DXGI, D3D9) and the kernel graphics driver (DxgKrnl) publish an ETW event every time an
//! application calls Present. We run a real-time ETW session, timestamp those events with the
//! QPC clock and derive FPS / frametime / percentile lows per process.
//!
//! Hardware sensors come from Windows performance counters (PDH) - CPU, per-core load,
//! GPU engines, VRAM - and NVML for NVIDIA temperature / clocks / power.

pub mod etw;
pub mod frames;
pub mod gametech;
pub mod afterburner;
pub mod hwinfo;
pub mod nvml;
pub mod sensors;
pub mod sysproc;
pub mod bottleneck;

pub use frames::{BenchSummary, FrameStats, FrameTracker, Source};
pub use sensors::{GpuInfo, SensorHub, SensorSnapshot};

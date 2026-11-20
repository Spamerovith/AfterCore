//! Hardware sensors: PDH performance counters + NVML.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use windows_sys::Win32::System::Performance::*;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use crate::nvml::Nvml;

const PDH_FMT_NOCAP100_: u32 = 0x8000;

#[derive(Clone, Debug, Default)]
pub struct GpuInfo {
    pub name: String,
    pub load: Option<f32>,
    pub temp: Option<f32>,
    pub clock_mhz: Option<f32>,
    pub mem_clock_mhz: Option<f32>,
    pub power_w: Option<f32>,
    pub fan_pct: Option<f32>,
    pub vram_used_gb: Option<f32>,
    pub vram_total_gb: Option<f32>,
    /// NVML clock throttle reasons (power / thermal limit while GPU-bound).
    pub throttle: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct ProcUsage {
    pub pid: u32,
    pub cpu_pct: f32,
    pub ram_gb: f32,
    pub gpu_pct: Option<f32>,
    /// Per-thread view from the kernel process list (no handles to the game).
    pub threads: Option<crate::sysproc::ThreadLoad>,
    /// Game's video memory in VRAM and spilled to system memory ("shared").
    pub gpu_dedicated_gb: Option<f32>,
    pub gpu_shared_gb: Option<f32>,
}

pub const HISTORY: usize = 120;

#[derive(Clone, Debug, Default)]
pub struct SensorSnapshot {
    pub cpu_load: Option<f32>,
    pub cpu_clock_mhz: Option<f32>,
    /// Indexed by logical processor.
    pub per_cpu: Vec<f32>,
    /// Effective clock per logical processor (MHz).
    pub per_cpu_mhz: Vec<f32>,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    /// Busiest logical processor (%), the usual sign of a main-thread CPU limit.
    pub cpu_max_core: Option<f32>,
    /// CPU package power: Windows Energy Meter (Intel RAPL, no driver), else HWiNFO.
    pub cpu_power_w: Option<f32>,
    /// From HWiNFO shared memory when available (needs no driver of ours).
    pub cpu_temp: Option<f32>,
    pub cpu_voltage: Option<f32>,
    pub cpu_fan_rpm: Option<f32>,
    /// HWiNFO or MSI Afterburner shared memory was readable on the last tick.
    pub hwinfo: bool,
    /// Refresh rate of the monitor with the foreground window.
    pub refresh_hz: Option<f32>,
    pub gpu: GpuInfo,
    pub target: Option<ProcUsage>,
    pub cpu_hist: VecDeque<f32>,
    pub gpu_hist: VecDeque<f32>,
    pub ram_hist: VecDeque<f32>,
    pub updated: Option<Instant>,
}

pub struct SensorHub {
    pub snapshot: RwLock<SensorSnapshot>,
    pub interval_ms: AtomicU32,
    target_pid: AtomicU32,
    stop: AtomicBool,
}

impl SensorHub {
    pub fn start(interval_ms: u32) -> Arc<Self> {
        let hub = Arc::new(Self {
            snapshot: RwLock::new(SensorSnapshot::default()),
            interval_ms: AtomicU32::new(interval_ms),
            target_pid: AtomicU32::new(0),
            stop: AtomicBool::new(false),
        });
        let h = hub.clone();
        std::thread::Builder::new()
            .name("sensors".into())
            .spawn(move || run(h))
            .expect("spawn sensors");
        hub
    }

    pub fn set_target(&self, pid: u32) {
        self.target_pid.store(pid, Ordering::Relaxed);
    }

    pub fn get(&self) -> SensorSnapshot {
        self.snapshot.read().clone()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// ─────────────────────────── PDH wrapper ───────────────────────────

struct Query {
    q: isize,
}

struct Counter(isize);

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Query {
    fn new() -> Option<Self> {
        let mut q: isize = unsafe { std::mem::zeroed() };
        (unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut q) } == 0).then_some(Self { q })
    }

    fn add(&self, path: &str) -> Option<Counter> {
        let mut c: isize = unsafe { std::mem::zeroed() };
        let p = w(path);
        (unsafe { PdhAddEnglishCounterW(self.q, p.as_ptr(), 0, &mut c) } == 0).then_some(Counter(c))
    }

    fn collect(&self) {
        unsafe { PdhCollectQueryData(self.q) };
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.q) };
    }
}

impl Counter {
    fn value(&self) -> Option<f64> {
        let mut v: PDH_FMT_COUNTERVALUE = unsafe { std::mem::zeroed() };
        let rc = unsafe {
            PdhGetFormattedCounterValue(self.0, PDH_FMT_DOUBLE | PDH_FMT_NOCAP100_, std::ptr::null_mut(), &mut v)
        };
        (rc == 0 && (v.CStatus == 0 || v.CStatus == 1)).then(|| unsafe { v.Anonymous.doubleValue })
    }

    fn array(&self) -> Vec<(String, f64)> {
        let mut size = 0u32;
        let mut count = 0u32;
        let fmt = PDH_FMT_DOUBLE | PDH_FMT_NOCAP100_;
        unsafe {
            PdhGetFormattedCounterArrayW(self.0, fmt, &mut size, &mut count, std::ptr::null_mut());
        }
        if size == 0 {
            return Vec::new();
        }
        // u64 backing for alignment.
        let mut buf = vec![0u64; (size as usize).div_ceil(8) + 1];
        let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        let rc = unsafe { PdhGetFormattedCounterArrayW(self.0, fmt, &mut size, &mut count, items) };
        if rc != 0 {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            unsafe {
                let it = &*items.add(i);
                if it.FmtValue.CStatus != 0 && it.FmtValue.CStatus != 1 {
                    continue;
                }
                let mut len = 0usize;
                while *it.szName.add(len) != 0 {
                    len += 1;
                }
                let name = String::from_utf16_lossy(std::slice::from_raw_parts(it.szName, len));
                out.push((name, it.FmtValue.Anonymous.doubleValue));
            }
        }
        out
    }
}

struct Pdh {
    q: Query,
    cpu_total: Option<Counter>,
    cpu_each: Option<Counter>,
    cpu_perf: Option<Counter>,
    cpu_freq: Option<Counter>,
    cpu_perf_each: Option<Counter>,
    cpu_freq_each: Option<Counter>,
    gpu_engine: Option<Counter>,
    gpu_mem: Option<Counter>,
    cpu_energy: Option<Counter>,
    gpu_proc_dedicated: Option<Counter>,
    gpu_proc_shared: Option<Counter>,
}

impl Pdh {
    fn new() -> Option<Self> {
        let q = Query::new()?;
        let s = Self {
            cpu_total: q.add("\\Processor Information(_Total)\\% Processor Utility"),
            cpu_each: q.add("\\Processor Information(*)\\% Processor Utility"),
            cpu_perf: q.add("\\Processor Information(_Total)\\% Processor Performance"),
            cpu_freq: q.add("\\Processor Information(_Total)\\Processor Frequency"),
            cpu_perf_each: q.add("\\Processor Information(*)\\% Processor Performance"),
            cpu_freq_each: q.add("\\Processor Information(*)\\Processor Frequency"),
            gpu_engine: q.add("\\GPU Engine(*)\\Utilization Percentage"),
            gpu_mem: q.add("\\GPU Adapter Memory(*)\\Dedicated Usage"),
            // Intel RAPL through the Windows Energy Meter Interface (milliwatts).
            cpu_energy: q.add("\\Energy Meter(RAPL_Package0_PKG)\\Power"),
            gpu_proc_dedicated: q.add("\\GPU Process Memory(*)\\Dedicated Usage"),
            gpu_proc_shared: q.add("\\GPU Process Memory(*)\\Shared Usage"),
            q,
        };
        s.q.collect();
        Some(s)
    }
}

fn parse_pid(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("pid_")?;
    rest.split('_').next()?.parse().ok()
}

/// Engine key without the pid part: `luid_…_phys_0_eng_0_engtype_3D`.
fn engine_key(name: &str) -> &str {
    name.find("luid_").map(|i| &name[i..]).unwrap_or(name)
}

fn push_hist(h: &mut VecDeque<f32>, v: f32) {
    h.push_back(v);
    while h.len() > HISTORY {
        h.pop_front();
    }
}

fn foreground_refresh_hz() -> Option<f32> {
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromWindow, DEVMODEW, ENUM_CURRENT_SETTINGS, MONITORINFOEXW,
        MONITOR_DEFAULTTOPRIMARY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    unsafe {
        let mon = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        let mut mi: MONITORINFOEXW = std::mem::zeroed();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(mon, &mut mi as *mut MONITORINFOEXW as *mut _) == 0 {
            return None;
        }
        let mut dm: DEVMODEW = std::mem::zeroed();
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        if EnumDisplaySettingsW(mi.szDevice.as_ptr(), ENUM_CURRENT_SETTINGS, &mut dm) == 0 {
            return None;
        }
        (dm.dmDisplayFrequency > 1).then_some(dm.dmDisplayFrequency as f32)
    }
}

fn run(hub: Arc<SensorHub>) {
    let mut pdh = Pdh::new();
    let mut pdh_age = Instant::now();
    let nvml = Nvml::load();
    let ncpu = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let mut sampler = crate::sysproc::ThreadSampler::new();
    std::thread::sleep(Duration::from_millis(250));

    while !hub.stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        // GPU engine instances come and go with processes; rebuild the query periodically.
        if pdh_age.elapsed() > Duration::from_secs(60) {
            pdh = Pdh::new();
            pdh_age = Instant::now();
            std::thread::sleep(Duration::from_millis(200));
        }

        let mut snap = hub.snapshot.read().clone();
        snap.updated = Some(Instant::now());

        let mut gpu_by_pid: HashMap<u32, f32> = HashMap::new();
        let tpid = hub.target_pid.load(Ordering::Relaxed);
        let mut proc_vram: (Option<f32>, Option<f32>) = (None, None);
        if let Some(p) = pdh.as_ref() {
            p.q.collect();
            snap.cpu_load = p.cpu_total.as_ref().and_then(Counter::value).map(|v| v.clamp(0.0, 100.0) as f32);
            if let (Some(perf), Some(freq)) =
                (p.cpu_perf.as_ref().and_then(Counter::value), p.cpu_freq.as_ref().and_then(Counter::value))
            {
                snap.cpu_clock_mhz = Some((freq * perf / 100.0) as f32);
            }
            // Instance names are "group,index".
            let per_core = |c: Option<&Counter>| -> Option<Vec<f32>> {
                let c = c?;
                let mut per = vec![0f32; ncpu];
                for (name, v) in c.array() {
                    let mut it = name.split(',');
                    let (Some(g), Some(i)) = (it.next(), it.next()) else { continue };
                    let (Ok(g), Ok(i)) = (g.parse::<usize>(), i.parse::<usize>()) else { continue };
                    let idx = g * 64 + i;
                    if idx >= per.len() {
                        per.resize(idx + 1, 0.0);
                    }
                    per[idx] = v as f32;
                }
                Some(per)
            };
            if let Some(per) = per_core(p.cpu_each.as_ref()) {
                snap.per_cpu = per.into_iter().map(|v| v.clamp(0.0, 100.0)).collect();
                snap.cpu_max_core = snap.per_cpu.iter().cloned().reduce(f32::max);
            }
            snap.cpu_power_w =
                p.cpu_energy.as_ref().and_then(Counter::value).filter(|v| *v > 0.0).map(|mw| (mw / 1000.0) as f32);
            if let (Some(perf), Some(freq)) = (per_core(p.cpu_perf_each.as_ref()), per_core(p.cpu_freq_each.as_ref())) {
                snap.per_cpu_mhz = perf.iter().zip(freq.iter()).map(|(p, f)| f * p / 100.0).collect();
            }
            if let Some(c) = p.gpu_engine.as_ref() {
                let mut engines: HashMap<String, f64> = HashMap::new();
                for (name, v) in c.array() {
                    *engines.entry(engine_key(&name).to_string()).or_default() += v;
                    if let Some(pid) = parse_pid(&name) {
                        let e = gpu_by_pid.entry(pid).or_default();
                        *e = e.max(v as f32);
                    }
                }
                let load = engines.values().cloned().fold(0.0, f64::max).clamp(0.0, 100.0) as f32;
                snap.gpu.load = Some(load);
            }
            let per_pid_gb = |c: Option<&Counter>| -> Option<f32> {
                let sum: f64 = c?.array().into_iter().filter(|(n, _)| parse_pid(n) == Some(tpid)).map(|(_, v)| v).sum();
                (tpid != 0).then_some((sum / 1073741824.0) as f32)
            };
            proc_vram = (per_pid_gb(p.gpu_proc_dedicated.as_ref()), per_pid_gb(p.gpu_proc_shared.as_ref()));
            if let Some(c) = p.gpu_mem.as_ref() {
                let used = c.array().into_iter().map(|(_, v)| v).fold(0.0, f64::max);
                if used > 0.0 {
                    snap.gpu.vram_used_gb = Some((used / 1073741824.0) as f32);
                }
            }
        }

        if let Some(n) = nvml.as_ref() {
            let s = n.sample();
            snap.gpu.name = s.name;
            snap.gpu.temp = s.temp;
            snap.gpu.clock_mhz = s.clock_mhz;
            snap.gpu.mem_clock_mhz = s.mem_clock_mhz;
            snap.gpu.power_w = s.power_w;
            snap.gpu.fan_pct = s.fan_pct;
            snap.gpu.throttle = s.throttle;
            if s.load.is_some() {
                // NVML reports the whole GPU; prefer it over PDH engine sums.
                snap.gpu.load = s.load;
            }
            if s.vram_used_gb.is_some() {
                snap.gpu.vram_used_gb = s.vram_used_gb;
                snap.gpu.vram_total_gb = s.vram_total_gb;
            }
        }

        // Driver-backed CPU sensors published by HWiNFO or MSI Afterburner (first wins per value).
        let hw = crate::hwinfo::read();
        let ab = crate::afterburner::read();
        snap.hwinfo = hw.is_some() || ab.is_some();
        let hw = hw.unwrap_or_default();
        let ab = ab.unwrap_or_default();
        snap.cpu_temp = hw.temp.or(ab.temp);
        snap.cpu_voltage = hw.voltage.or(ab.voltage);
        snap.cpu_fan_rpm = hw.fan_rpm.or(ab.fan_rpm);
        if snap.cpu_power_w.is_none() {
            snap.cpu_power_w = hw.power_w.or(ab.power_w);
        }

        let mut ms: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut ms) } != 0 {
            snap.ram_total_gb = ms.ullTotalPhys as f32 / 1073741824.0;
            snap.ram_used_gb = (ms.ullTotalPhys - ms.ullAvailPhys) as f32 / 1073741824.0;
        }

        // Game CPU / RAM / threads from the kernel process list: no handle to the game.
        snap.target = sampler.sample(tpid).map(|p| ProcUsage {
            pid: tpid,
            cpu_pct: p.load.map_or(0.0, |l| (l.cores_busy / ncpu.max(1) as f32 * 100.0).clamp(0.0, 100.0)),
            ram_gb: p.working_set_gb,
            gpu_pct: gpu_by_pid.get(&tpid).copied(),
            threads: p.load,
            gpu_dedicated_gb: proc_vram.0,
            gpu_shared_gb: proc_vram.1,
        });
        snap.refresh_hz = foreground_refresh_hz();

        let cpu = snap.cpu_load.unwrap_or(0.0);
        let gpu = snap.gpu.load.unwrap_or(0.0);
        let ram = if snap.ram_total_gb > 0.0 { snap.ram_used_gb / snap.ram_total_gb * 100.0 } else { 0.0 };
        push_hist(&mut snap.cpu_hist, cpu);
        push_hist(&mut snap.gpu_hist, gpu);
        push_hist(&mut snap.ram_hist, ram);

        *hub.snapshot.write() = snap;

        let interval = Duration::from_millis(hub.interval_ms.load(Ordering::Relaxed).clamp(200, 5000) as u64);
        let spent = started.elapsed();
        if spent < interval {
            std::thread::sleep(interval - spent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_engine_names() {
        let n = "pid_1234_luid_0x00000000_0x0000D1A5_phys_0_eng_0_engtype_3D";
        assert_eq!(parse_pid(n), Some(1234));
        assert_eq!(engine_key(n), "luid_0x00000000_0x0000D1A5_phys_0_eng_0_engtype_3D");
    }

    #[test]
    fn live_sample() {
        let hub = SensorHub::start(300);
        hub.set_target(std::process::id());
        std::thread::sleep(Duration::from_millis(1600));
        let s = hub.get();
        hub.stop();
        eprintln!(
            "pwr={:?} temp={:?} maxcore={:?} cpu={:?} clock={:?} mhz={:?} cores={} ram={:.1}/{:.1} gpu={:?}",
            s.cpu_power_w,
            s.cpu_temp,
            s.cpu_max_core,
            s.cpu_load,
            s.cpu_clock_mhz,
            &s.per_cpu_mhz[..s.per_cpu_mhz.len().min(4)],
            s.per_cpu.len(),
            s.ram_used_gb,
            s.ram_total_gb,
            s.gpu
        );
        eprintln!("target={:?} refresh={:?} throttle={:?}", s.target, s.refresh_hz, s.gpu.throttle);
        assert!(s.ram_total_gb > 0.5);
    }
}

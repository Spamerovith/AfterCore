//! Bottleneck analysis that looks for evidence, not just high load.
//!
//! A resource is called the limiter only when it actually holds the frame rate back:
//! * **GPU**: the GPU is busy almost all the time (≥ 93 %). NVML throttle reasons tell
//!   whether it is additionally held by its power or temperature limit.
//! * **CPU**: the GPU is waiting (< 90 %) while the game's busiest thread runs flat out
//!   (that thread decides the frame rate), or all cores are saturated.
//! * **VRAM**: video memory is full *and* the game has started spilling into system memory
//!   (its "shared" GPU memory grows) *and* frames stutter.
//! * **RAM**: the game takes hard page faults (reads its memory back from the page file)
//!   while RAM is nearly full *and* frames stutter.
//! * **Cap**: the frame rate sits steadily on the refresh rate or a typical limit, nothing is
//!   saturated - V-Sync / a limiter, not a bottleneck.
//!
//! Values are smoothed and a new verdict must hold for two sensor ticks before it is shown.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limiter {
    Gpu,
    Cpu,
    Vram,
    Ram,
    Cap,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    #[default]
    Ok,
    /// Close to the limit, but no evidence it slows the game.
    Pressure,
    /// This resource limits the game right now.
    Limit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Resource {
    pub pct: Option<f32>,
    pub level: Level,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Verdict {
    pub limiter: Option<Limiter>,
    /// Short reason: "power limit", "main thread", "spill 1.4 GB", "paging 420/s", "V-Sync".
    pub detail: String,
    pub cap_fps: Option<f32>,
    pub gpu: Resource,
    pub cpu: Resource,
    pub vram: Resource,
    pub ram: Resource,
    pub stutter: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Inputs<'a> {
    pub presenting: bool,
    pub fps: f32,
    pub avg_fps: f32,
    pub low1: f32,
    /// Recent frametimes in ms, oldest first.
    pub frametimes: &'a [f32],
    pub refresh_hz: Option<f32>,
    pub gpu_util: Option<f32>,
    pub gpu_throttle: Option<u64>,
    pub vram_used_gb: Option<f32>,
    pub vram_total_gb: Option<f32>,
    pub game_shared_gb: Option<f32>,
    /// Game's busiest thread, % of one logical processor.
    pub main_thread: Option<f32>,
    /// Game's threads together, in logical processors.
    pub cores_busy: Option<f32>,
    pub ncpu: usize,
    pub cpu_total: Option<f32>,
    pub max_core: Option<f32>,
    pub hard_faults_s: Option<f32>,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Ema(Option<f32>);

impl Ema {
    fn push(&mut self, v: Option<f32>, a: f32) -> Option<f32> {
        if let Some(v) = v {
            self.0 = Some(match self.0 {
                Some(o) => o + (v - o) * a,
                None => v,
            });
        }
        self.0
    }
}

#[derive(Debug, Default)]
pub struct Analyzer {
    gpu: Ema,
    main: Ema,
    faults: Ema,
    shared: Ema,
    shared_base: Option<f32>,
    stutter: Ema,
    current: Option<Option<Limiter>>,
    pending: Option<Option<Limiter>>,
    streak: u32,
}

const NV_POWER: u64 = 0x4 | 0x80; // SW power cap, HW power brake
const NV_THERMAL: u64 = 0x8 | 0x20 | 0x40; // HW slowdown, SW / HW thermal

const COMMON_CAPS: [f32; 12] = [30.0, 48.0, 50.0, 60.0, 75.0, 90.0, 100.0, 120.0, 144.0, 165.0, 180.0, 240.0];

fn median(v: &[f32]) -> f32 {
    let mut s: Vec<f32> = v.iter().copied().filter(|x| x.is_finite() && *x > 0.0).collect();
    if s.is_empty() {
        return 0.0;
    }
    s.sort_by(|a, b| a.total_cmp(b));
    s[s.len() / 2]
}

/// Hitches in the recent frames: frames much longer than the typical one.
pub fn stutters(frametimes: &[f32], avg_fps: f32, low1: f32) -> bool {
    let tail = &frametimes[frametimes.len().saturating_sub(240)..];
    let m = median(tail);
    if m <= 0.0 {
        return false;
    }
    let spikes = tail.iter().filter(|&&f| f > (m * 2.5).max(m + 6.0)).count();
    spikes >= 2 || (low1 > 0.0 && avg_fps > 0.0 && low1 < avg_fps * 0.5)
}

/// Frame rate pinned to the refresh rate or a typical limiter value with even pacing.
pub fn capped_at(fps: f32, frametimes: &[f32], refresh: Option<f32>) -> Option<f32> {
    if fps < 20.0 {
        return None;
    }
    let tail = &frametimes[frametimes.len().saturating_sub(120)..];
    if tail.len() < 20 {
        return None;
    }
    let mean = tail.iter().sum::<f32>() / tail.len() as f32;
    let var = tail.iter().map(|f| (f - mean).powi(2)).sum::<f32>() / tail.len() as f32;
    if mean <= 0.0 || var.sqrt() / mean > 0.12 {
        return None;
    }
    if let Some(r) = refresh {
        // V-Sync (= refresh), half refresh, or G-Sync + Reflex / NVCP limit a few % below it.
        for target in [r, r / 2.0] {
            if fps <= target * 1.02 && fps >= target * 0.9 {
                return Some(target.round());
            }
        }
    }
    COMMON_CAPS.iter().copied().find(|c| (fps - c).abs() <= c * 0.02)
}

impl Analyzer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, i: &Inputs) -> Verdict {
        let gpu = self.gpu.push(i.gpu_util, 0.45);
        let main = self.main.push(i.main_thread, 0.45);
        let faults = self.faults.push(i.hard_faults_s, 0.4);
        let shared = self.shared.push(i.game_shared_gb, 0.5);
        if let Some(s) = i.game_shared_gb {
            self.shared_base = Some(self.shared_base.map_or(s, |b| b.min(s)));
        }
        let raw_stutter = i.presenting && stutters(i.frametimes, i.avg_fps, i.low1);
        let stutter = self.stutter.push(Some(if raw_stutter { 1.0 } else { 0.0 }), 0.5).unwrap_or(0.0) >= 0.5;

        let pct = |u: Option<f32>, t: Option<f32>| match (u, t) {
            (Some(u), Some(t)) if t > 0.0 => Some((u / t * 100.0).clamp(0.0, 100.0)),
            _ => None,
        };
        let vram_pct = pct(i.vram_used_gb, i.vram_total_gb);
        let ram_pct = pct(Some(i.ram_used_gb), Some(i.ram_total_gb));
        let ram_free = i.ram_total_gb - i.ram_used_gb;
        let spill = match (shared, self.shared_base) {
            (Some(s), Some(b)) => (s - b).max(0.0),
            _ => 0.0,
        };
        let spilling = spill >= 0.75 || (shared.unwrap_or(0.0) >= 2.5 && vram_pct.unwrap_or(0.0) >= 95.0);
        let g = gpu.unwrap_or(0.0);
        let cpu_pct = main.or(i.max_core);
        let cap = if i.presenting && g < 92.0 && main.unwrap_or(0.0) < 85.0 {
            capped_at(i.fps, i.frametimes, i.refresh_hz)
        } else {
            None
        };

        let mut detail = String::new();
        let candidate = if !i.presenting {
            None
        } else if vram_pct.unwrap_or(0.0) >= 93.0 && spilling && stutter {
            detail = format!("spill {:.1} GB", spill.max(shared.unwrap_or(0.0) - self.shared_base.unwrap_or(0.0)));
            Some(Limiter::Vram)
        } else if faults.unwrap_or(0.0) >= 150.0 && (ram_pct.unwrap_or(0.0) >= 88.0 || ram_free < 2.0) && stutter {
            detail = format!("paging {:.0}/s", faults.unwrap_or(0.0));
            Some(Limiter::Ram)
        } else if g >= 93.0 {
            let t = i.gpu_throttle.unwrap_or(0);
            if t & NV_THERMAL != 0 {
                detail = "thermal".into();
            } else if t & NV_POWER != 0 {
                detail = "power limit".into();
            }
            Some(Limiter::Gpu)
        } else if cap.is_some() {
            detail = match i.refresh_hz {
                Some(r) if cap.is_some_and(|c| (c - r).abs() < 1.5) => "V-Sync".into(),
                _ => "limit".into(),
            };
            Some(Limiter::Cap)
        } else if g < 90.0 && main.unwrap_or(0.0) >= 85.0 {
            detail = "main thread".into();
            Some(Limiter::Cpu)
        } else if g < 90.0
            && (i.cores_busy.is_some_and(|c| i.ncpu > 0 && c >= i.ncpu as f32 * 0.85) || i.cpu_total.unwrap_or(0.0) >= 92.0)
        {
            detail = "all cores".into();
            Some(Limiter::Cpu)
        } else if g < 75.0 && main.unwrap_or(0.0) >= 60.0 {
            // GPU idles a quarter of the time and nothing else is full: the frame waits on the CPU.
            detail = "main thread".into();
            Some(Limiter::Cpu)
        } else {
            None
        };

        // Hysteresis: a new verdict must repeat before it replaces the shown one.
        match self.current {
            None => self.current = Some(candidate),
            Some(cur) if cur == candidate => {
                self.pending = None;
                self.streak = 0;
            }
            Some(_) => {
                if self.pending == Some(candidate) {
                    self.streak += 1;
                } else {
                    self.pending = Some(candidate);
                    self.streak = 1;
                }
                if self.streak >= 2 {
                    self.current = Some(candidate);
                    self.pending = None;
                    self.streak = 0;
                }
            }
        }
        let limiter = self.current.flatten();
        if limiter != candidate {
            detail.clear();
        }

        let level = |is: bool, pressure: bool| {
            if is {
                Level::Limit
            } else if pressure {
                Level::Pressure
            } else {
                Level::Ok
            }
        };
        Verdict {
            limiter,
            detail,
            cap_fps: if limiter == Some(Limiter::Cap) { cap } else { None },
            gpu: Resource { pct: gpu, level: level(limiter == Some(Limiter::Gpu), g >= 85.0) },
            cpu: Resource {
                pct: cpu_pct,
                level: level(limiter == Some(Limiter::Cpu), cpu_pct.unwrap_or(0.0) >= 75.0),
            },
            vram: Resource {
                pct: vram_pct,
                level: level(limiter == Some(Limiter::Vram), vram_pct.unwrap_or(0.0) >= 90.0 || spilling),
            },
            ram: Resource {
                pct: ram_pct,
                level: level(
                    limiter == Some(Limiter::Ram),
                    ram_pct.unwrap_or(0.0) >= 90.0 || (faults.unwrap_or(0.0) >= 150.0 && ram_pct.unwrap_or(0.0) >= 80.0),
                ),
            },
            stutter,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smooth(ms: f32) -> Vec<f32> {
        (0..200).map(|i| ms + (i % 3) as f32 * 0.05).collect()
    }

    fn base(ft: &[f32]) -> Inputs<'_> {
        let fps = 1000.0 / median(ft);
        Inputs {
            presenting: true,
            fps,
            avg_fps: fps,
            low1: fps * 0.8,
            frametimes: ft,
            refresh_hz: Some(240.0),
            gpu_util: Some(70.0),
            vram_used_gb: Some(9.0),
            vram_total_gb: Some(16.0),
            game_shared_gb: Some(0.6),
            main_thread: Some(40.0),
            cores_busy: Some(4.0),
            ncpu: 24,
            cpu_total: Some(30.0),
            max_core: Some(70.0),
            hard_faults_s: Some(5.0),
            ram_used_gb: 20.0,
            ram_total_gb: 32.0,
            ..Default::default()
        }
    }

    fn run(i: &Inputs) -> Verdict {
        let mut a = Analyzer::new();
        let mut v = Verdict::default();
        for _ in 0..4 {
            v = a.update(i);
        }
        v
    }

    #[test]
    fn gpu_bound_with_power_limit() {
        let ft = smooth(8.0);
        let v = run(&Inputs { gpu_util: Some(98.0), gpu_throttle: Some(0x4), ..base(&ft) });
        assert_eq!(v.limiter, Some(Limiter::Gpu));
        assert_eq!(v.detail, "power limit");
        assert_eq!(v.gpu.level, Level::Limit);
    }

    #[test]
    fn high_ram_use_alone_is_not_a_bottleneck() {
        let ft = smooth(8.0);
        let v = run(&Inputs { ram_used_gb: 30.5, gpu_util: Some(98.0), ..base(&ft) });
        assert_eq!(v.limiter, Some(Limiter::Gpu));
        assert_eq!(v.ram.level, Level::Pressure);
    }

    #[test]
    fn ram_bound_needs_paging_and_stutter() {
        let mut ft = smooth(8.0);
        for k in [50, 90, 130, 170] {
            ft[k] = 60.0;
        }
        let v = run(&Inputs { ram_used_gb: 30.8, hard_faults_s: Some(900.0), ..base(&ft) });
        assert_eq!(v.limiter, Some(Limiter::Ram));
        // Same paging without stutters: not blamed.
        let ft2 = smooth(8.0);
        let v2 = run(&Inputs { ram_used_gb: 30.8, hard_faults_s: Some(900.0), ..base(&ft2) });
        assert_ne!(v2.limiter, Some(Limiter::Ram));
    }

    #[test]
    fn vram_bound_needs_spill_and_stutter() {
        let mut ft = smooth(10.0);
        for k in [40, 100, 160] {
            ft[k] = 70.0;
        }
        let mut a = Analyzer::new();
        a.update(&Inputs { vram_used_gb: Some(15.6), game_shared_gb: Some(0.5), ..base(&ft) });
        let mut v = Verdict::default();
        for _ in 0..4 {
            v = a.update(&Inputs { vram_used_gb: Some(15.6), game_shared_gb: Some(2.1), ..base(&ft) });
        }
        assert_eq!(v.limiter, Some(Limiter::Vram), "{v:?}");
        // Full VRAM without spilling is only pressure.
        let ft2 = smooth(10.0);
        let v2 = run(&Inputs { vram_used_gb: Some(15.6), gpu_util: Some(97.0), ..base(&ft2) });
        assert_eq!(v2.limiter, Some(Limiter::Gpu));
        assert_eq!(v2.vram.level, Level::Pressure);
    }

    #[test]
    fn cpu_main_thread() {
        let ft = smooth(9.0);
        let v = run(&Inputs { gpu_util: Some(62.0), main_thread: Some(97.0), ..base(&ft) });
        assert_eq!(v.limiter, Some(Limiter::Cpu));
        assert_eq!(v.detail, "main thread");
    }

    #[test]
    fn vsync_cap_is_not_a_bottleneck() {
        let ft = smooth(1000.0 / 144.0);
        let v = run(&Inputs { refresh_hz: Some(144.0), gpu_util: Some(55.0), main_thread: Some(45.0), ..base(&ft) });
        assert_eq!(v.limiter, Some(Limiter::Cap));
        assert_eq!(v.cap_fps, Some(144.0));
    }

    #[test]
    fn verdict_needs_two_ticks_to_change() {
        let ft = smooth(8.0);
        let mut a = Analyzer::new();
        for _ in 0..3 {
            a.update(&Inputs { gpu_util: Some(99.0), ..base(&ft) });
        }
        // One odd sample does not flip the verdict.
        let v = a.update(&Inputs { gpu_util: Some(20.0), main_thread: Some(99.0), ..base(&ft) });
        assert_eq!(v.limiter, Some(Limiter::Gpu));
    }
}

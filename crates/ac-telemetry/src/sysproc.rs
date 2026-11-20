//! Per-thread CPU time and hard page faults of the game, read from the kernel's process list
//! (`NtQuerySystemInformation(SystemProcessInformation)`, the same source Task Manager uses).
//!
//! No handle to the game or its threads is opened, so anti-cheats see nothing.

use std::collections::HashMap;
use std::time::Instant;

use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

type NtQsi = unsafe extern "system" fn(u32, *mut u8, u32, *mut u32) -> i32;

const SYSTEM_PROCESS_INFORMATION: u32 = 5;
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
const PROC_SIZE: usize = 0x100;
const THREAD_SIZE: usize = 0x50;

#[derive(Clone, Debug, Default)]
pub struct ProcSnapshot {
    /// Thread id → kernel + user time (100 ns).
    pub threads: HashMap<u32, u64>,
    pub hard_faults: u32,
    /// Working set in bytes.
    pub working_set: u64,
}

fn rd<const N: usize>(b: &[u8], o: usize) -> Option<[u8; N]> {
    b.get(o..o + N)?.try_into().ok()
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    rd::<4>(b, o).map(u32::from_le_bytes)
}
fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    rd::<8>(b, o).map(u64::from_le_bytes)
}

fn parse(buf: &[u8], pid: u32) -> Option<ProcSnapshot> {
    let mut off = 0usize;
    loop {
        let next = u32_at(buf, off)? as usize;
        let nthreads = u32_at(buf, off + 0x04)? as usize;
        let this_pid = u64_at(buf, off + 0x50)? as u32;
        if this_pid == pid {
            let mut s = ProcSnapshot {
                hard_faults: u32_at(buf, off + 0x10)?,
                working_set: u64_at(buf, off + 0x90)?,
                threads: HashMap::with_capacity(nthreads),
            };
            for i in 0..nthreads {
                let t = off + PROC_SIZE + i * THREAD_SIZE;
                let time = u64_at(buf, t)?.wrapping_add(u64_at(buf, t + 0x08)?);
                let tid = u64_at(buf, t + 0x30)? as u32;
                s.threads.insert(tid, time);
            }
            return Some(s);
        }
        if next == 0 {
            return None;
        }
        off += next;
    }
}

pub struct ThreadSampler {
    f: Option<NtQsi>,
    buf: Vec<u8>,
    prev: Option<(u32, ProcSnapshot, Instant)>,
}

/// One sample of the game process.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProcSample {
    pub working_set_gb: f32,
    /// `None` on the first sample after the target changed.
    pub load: Option<ThreadLoad>,
}

/// What the game's threads did since the previous sample.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ThreadLoad {
    /// Busiest thread in % of one logical processor (main / render thread).
    pub main_thread_pct: f32,
    /// Sum over threads, in logical processors (2.5 = two and a half cores busy).
    pub cores_busy: f32,
    pub hard_faults_per_s: f32,
}

impl ThreadSampler {
    pub fn new() -> Self {
        let name: Vec<u16> = "ntdll.dll".encode_utf16().chain(Some(0)).collect();
        let f = unsafe {
            let m = GetModuleHandleW(name.as_ptr());
            if m.is_null() {
                None
            } else {
                GetProcAddress(m, c"NtQuerySystemInformation".as_ptr().cast())
                    .map(|p| std::mem::transmute::<_, NtQsi>(p))
            }
        };
        Self { f, buf: vec![0u8; 1 << 20], prev: None }
    }

    fn snapshot(&mut self, pid: u32) -> Option<ProcSnapshot> {
        let f = self.f?;
        for _ in 0..4 {
            let mut need = 0u32;
            let st = unsafe { f(SYSTEM_PROCESS_INFORMATION, self.buf.as_mut_ptr(), self.buf.len() as u32, &mut need) };
            if st == STATUS_INFO_LENGTH_MISMATCH {
                self.buf.resize((need as usize).max(self.buf.len()) * 3 / 2, 0);
                continue;
            }
            if st < 0 {
                return None;
            }
            return parse(&self.buf, pid);
        }
        None
    }

    pub fn sample(&mut self, pid: u32) -> Option<ProcSample> {
        if pid == 0 {
            self.prev = None;
            return None;
        }
        let now = Instant::now();
        let Some(cur) = self.snapshot(pid) else {
            self.prev = None;
            return None;
        };
        let working_set_gb = cur.working_set as f32 / 1073741824.0;
        let load = match &self.prev {
            Some((p, prev, at)) if *p == pid => {
                let dt = now.duration_since(*at).as_secs_f64().max(0.05);
                let mut main = 0f64;
                let mut sum = 0f64;
                for (tid, t) in &cur.threads {
                    if let Some(old) = prev.threads.get(tid) {
                        let pct = t.saturating_sub(*old) as f64 / 1e7 / dt * 100.0;
                        main = main.max(pct);
                        sum += pct;
                    }
                }
                Some(ThreadLoad {
                    main_thread_pct: main.min(100.0) as f32,
                    cores_busy: (sum / 100.0) as f32,
                    hard_faults_per_s: (cur.hard_faults.saturating_sub(prev.hard_faults) as f64 / dt) as f32,
                })
            }
            _ => None,
        };
        self.prev = Some((pid, cur, now));
        Some(ProcSample { working_set_gb, load })
    }
}

impl Default for ThreadSampler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_own_process() {
        let mut s = ThreadSampler::new();
        let pid = std::process::id();
        assert!(s.sample(pid).unwrap().load.is_none()); // first call only primes
        let t0 = Instant::now();
        let mut x = 0u64;
        while t0.elapsed().as_millis() < 300 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
        }
        std::hint::black_box(x);
        let p = s.sample(pid).expect("own process visible");
        assert!(p.working_set_gb > 0.001 && p.working_set_gb < 4.0, "{p:?}");
        let l = p.load.unwrap();
        eprintln!("{l:?}");
        assert!(l.main_thread_pct > 50.0, "busy loop should load one thread: {l:?}");
    }
}

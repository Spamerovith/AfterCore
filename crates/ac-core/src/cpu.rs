//! CPU topology, core presets and applying affinity / CPU sets to processes.
//!
//! Topology comes from `GetSystemCpuSetInformation`, which exposes for every logical
//! processor its physical core, efficiency class (P vs E cores on hybrid CPUs) and last
//! level cache index (CCD / CCX on Ryzen).

use std::collections::BTreeMap;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessAffinityMask, OpenProcess, SetPriorityClass, SetProcessAffinityMask,
    ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS,
    NORMAL_PRIORITY_CLASS, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    PROCESS_SET_LIMITED_INFORMATION,
};

use crate::config::{CoreSet, CpuMode, Priority};
use crate::win::OwnedHandle;

#[link(name = "kernel32")]
extern "system" {
    fn GetSystemCpuSetInformation(info: *mut u8, len: u32, ret: *mut u32, process: HANDLE, flags: u32) -> i32;
    fn SetProcessDefaultCpuSets(process: HANDLE, ids: *const u32, count: u32) -> i32;
}

#[derive(Clone, Debug)]
pub struct LogicalCpu {
    /// Global logical index (group * 64 + index in group).
    pub index: u32,
    pub cpu_set_id: u32,
    pub group: u16,
    pub core: u32,
    pub efficiency: u8,
    pub llc: u8,
    /// 0 for the first hardware thread of a core, 1 for its SMT sibling…
    pub smt: u8,
}

#[derive(Clone, Debug)]
pub struct PhysCore {
    pub id: u32,
    pub efficiency: u8,
    pub llc: u8,
    pub threads: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Preset {
    pub id: &'static str,
    pub title: String,
    pub hint: String,
    pub cpus: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Topology {
    pub name: String,
    pub cpus: Vec<LogicalCpu>,
    pub cores: Vec<PhysCore>,
    pub hybrid: bool,
    pub max_efficiency: u8,
    pub llc_count: usize,
}

impl Topology {
    pub fn detect() -> Self {
        let mut cpus = query_cpu_sets();
        if cpus.is_empty() {
            // Fallback: flat topology.
            let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as u32;
            cpus = (0..n)
                .map(|i| LogicalCpu { index: i, cpu_set_id: 256 + i, group: 0, core: i, efficiency: 0, llc: 0, smt: 0 })
                .collect();
        }
        cpus.sort_by_key(|c| c.index);

        let mut by_core: BTreeMap<(u16, u32), PhysCore> = BTreeMap::new();
        for c in cpus.iter_mut() {
            let e = by_core.entry((c.group, c.core)).or_insert(PhysCore {
                id: c.core,
                efficiency: c.efficiency,
                llc: c.llc,
                threads: Vec::new(),
            });
            c.smt = e.threads.len() as u8;
            e.threads.push(c.index);
        }
        let mut cores: Vec<PhysCore> = by_core.into_values().collect();
        cores.sort_by_key(|c| c.threads[0]);

        let max_efficiency = cpus.iter().map(|c| c.efficiency).max().unwrap_or(0);
        let min_efficiency = cpus.iter().map(|c| c.efficiency).min().unwrap_or(0);
        let mut llcs: Vec<u8> = cpus.iter().map(|c| c.llc).collect();
        llcs.sort();
        llcs.dedup();

        Self {
            name: cpu_name(),
            hybrid: max_efficiency != min_efficiency,
            max_efficiency,
            llc_count: llcs.len(),
            cpus,
            cores,
        }
    }

    pub fn logical_count(&self) -> usize {
        self.cpus.len()
    }

    pub fn all(&self) -> Vec<u32> {
        self.cpus.iter().map(|c| c.index).collect()
    }

    pub fn is_pcore(&self, core: &PhysCore) -> bool {
        !self.hybrid || core.efficiency == self.max_efficiency
    }

    pub fn smt_present(&self) -> bool {
        self.cores.iter().any(|c| c.threads.len() > 1)
    }

    pub fn core_of(&self, cpu: u32) -> Option<&PhysCore> {
        self.cores.iter().find(|c| c.threads.contains(&cpu))
    }

    /// Topology-aware built-in presets. Only presets that make sense for this CPU are returned.
    pub fn presets(&self) -> Vec<Preset> {
        let mut out = vec![Preset {
            id: "all",
            title: "Все ядра".into(),
            hint: "Без ограничений - как задумано Windows".into(),
            cpus: self.all(),
        }];

        if self.hybrid {
            let p: Vec<u32> = self.cores.iter().filter(|c| self.is_pcore(c)).flat_map(|c| c.threads.clone()).collect();
            let e: Vec<u32> = self.cores.iter().filter(|c| !self.is_pcore(c)).flat_map(|c| c.threads.clone()).collect();
            let p_nosmt: Vec<u32> = self.cores.iter().filter(|c| self.is_pcore(c)).map(|c| c.threads[0]).collect();
            out.push(Preset {
                id: "pcores",
                title: "P-ядра".into(),
                hint: "Только производительные ядра - меньше статтеров в играх".into(),
                cpus: p.clone(),
            });
            if p_nosmt.len() != p.len() {
                out.push(Preset {
                    id: "pcores_nosmt",
                    title: "P-ядра без HT".into(),
                    hint: "По одному потоку на каждое P-ядро".into(),
                    cpus: p_nosmt,
                });
            }
            out.push(Preset {
                id: "ecores",
                title: "E-ядра".into(),
                hint: "Энергоэффективные ядра - для фоновых задач, стрима, лаунчеров".into(),
                cpus: e,
            });
        }

        if self.smt_present() {
            out.push(Preset {
                id: "nosmt",
                title: if self.hybrid { "Без HT (все ядра)" } else { "Без SMT / HT" }.into(),
                hint: "Один поток на физическое ядро".into(),
                cpus: self.cores.iter().map(|c| c.threads[0]).collect(),
            });
        }

        if self.llc_count > 1 {
            let mut llcs: Vec<u8> = self.cores.iter().map(|c| c.llc).collect();
            llcs.sort();
            llcs.dedup();
            for (i, l) in llcs.iter().enumerate().take(4) {
                let id: &'static str = ["ccd0", "ccd1", "ccd2", "ccd3"][i];
                out.push(Preset {
                    id,
                    title: format!("CCD {i}"),
                    hint: "Ядра с общим L3-кэшем - на X3D это может быть V-Cache".into(),
                    cpus: self.cores.iter().filter(|c| c.llc == *l).flat_map(|c| c.threads.clone()).collect(),
                });
            }
        }

        let n = self.cores.len();
        if n >= 4 {
            out.push(Preset {
                id: "no_core0",
                title: "Без ядра 0".into(),
                hint: "Освобождает ядро 0, которое чаще занято системными прерываниями".into(),
                cpus: self.cores.iter().skip(1).flat_map(|c| c.threads.clone()).collect(),
            });
            out.push(Preset {
                id: "half",
                title: "Первая половина".into(),
                hint: "Первая половина физических ядер".into(),
                cpus: self.cores.iter().take(n / 2).flat_map(|c| c.threads.clone()).collect(),
            });
        }
        out.retain(|p| !p.cpus.is_empty());
        out
    }

    pub fn preset(&self, id: &str) -> Option<Preset> {
        self.presets().into_iter().find(|p| p.id == id)
    }

    /// Resolve a rule's core set reference (`builtin:<id>` or saved set name).
    pub fn resolve_set(&self, reference: &str, saved: &[CoreSet]) -> Option<Vec<u32>> {
        if reference.is_empty() {
            return None;
        }
        if let Some(id) = reference.strip_prefix("builtin:") {
            return self.preset(id).map(|p| p.cpus);
        }
        saved.iter().find(|s| s.name == reference).map(|s| s.cpus.clone())
    }

    pub fn set_label(&self, reference: &str) -> String {
        if reference.is_empty() {
            return "Не менять".into();
        }
        if let Some(id) = reference.strip_prefix("builtin:") {
            return self.preset(id).map(|p| p.title).unwrap_or_else(|| id.to_string());
        }
        reference.to_string()
    }

    pub fn mask_of(cpus: &[u32]) -> usize {
        cpus.iter().filter(|&&c| c < usize::BITS).fold(0usize, |m, &c| m | (1usize << c))
    }

    pub fn cpus_of_mask(mask: usize) -> Vec<u32> {
        (0..usize::BITS).filter(|b| mask & (1usize << b) != 0).collect()
    }
}

fn query_cpu_sets() -> Vec<LogicalCpu> {
    let mut len = 0u32;
    unsafe {
        GetSystemCpuSetInformation(std::ptr::null_mut(), 0, &mut len, GetCurrentProcess(), 0);
    }
    if len == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; len as usize];
    let ok = unsafe { GetSystemCpuSetInformation(buf.as_mut_ptr(), len, &mut len, GetCurrentProcess(), 0) };
    if ok == 0 {
        return Vec::new();
    }
    // SYSTEM_CPU_SET_INFORMATION:
    //   u32 Size, u32 Type, then CpuSet { u32 Id, u16 Group, u8 LogicalProcessorIndex,
    //   u8 CoreIndex, u8 LastLevelCacheIndex, u8 NumaNodeIndex, u8 EfficiencyClass, … }
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 8 <= len as usize {
        let size = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
        let kind = u32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap());
        if size == 0 {
            break;
        }
        if kind == 0 && off + 19 <= len as usize {
            let id = u32::from_le_bytes(buf[off + 8..off + 12].try_into().unwrap());
            let group = u16::from_le_bytes(buf[off + 12..off + 14].try_into().unwrap());
            let lp = buf[off + 14];
            let core = buf[off + 15];
            let llc = buf[off + 16];
            let eff = buf[off + 18];
            out.push(LogicalCpu {
                index: group as u32 * 64 + lp as u32,
                cpu_set_id: id,
                group,
                core: group as u32 * 256 + core as u32,
                efficiency: eff,
                llc,
                smt: 0,
            });
        }
        off += size;
    }
    out
}

fn cpu_name() -> String {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let key = crate::win::wide("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0");
    let val = crate::win::wide("ProcessorNameString");
    let mut buf = [0u16; 256];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            val.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if rc == 0 {
        crate::win::from_wide(&buf).trim().to_string()
    } else {
        "Процессор".into()
    }
}

// ─────────────────────────── Applying ───────────────────────────

fn open(pid: u32, access: u32) -> Result<OwnedHandle, String> {
    let h = unsafe { OpenProcess(access, 0, pid) };
    OwnedHandle::new(h).ok_or_else(|| {
        let err = std::io::Error::last_os_error();
        format!("нет доступа к процессу {pid}: {err}")
    })
}

pub fn get_affinity(pid: u32) -> Option<usize> {
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?;
    let mut proc_mask = 0usize;
    let mut sys_mask = 0usize;
    let ok = unsafe { GetProcessAffinityMask(h.raw(), &mut proc_mask, &mut sys_mask) };
    (ok != 0).then_some(proc_mask)
}

pub fn apply_cores(pid: u32, cpus: &[u32], mode: CpuMode, topo: &Topology) -> Result<(), String> {
    if cpus.is_empty() {
        return Err("не выбрано ни одного ядра".into());
    }
    match mode {
        CpuMode::Affinity => {
            let h = open(pid, PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION)?;
            // Clear soft CPU sets so they don't fight the mask.
            unsafe { SetProcessDefaultCpuSets(h.raw(), std::ptr::null(), 0) };
            let mask = Topology::mask_of(cpus);
            if unsafe { SetProcessAffinityMask(h.raw(), mask) } == 0 {
                return Err(format!("SetProcessAffinityMask: {}", std::io::Error::last_os_error()));
            }
        }
        CpuMode::CpuSets => {
            let h = open(pid, PROCESS_SET_LIMITED_INFORMATION | PROCESS_SET_INFORMATION)?;
            let all = Topology::mask_of(&topo.all());
            unsafe { SetProcessAffinityMask(h.raw(), all) };
            let ids: Vec<u32> = topo.cpus.iter().filter(|c| cpus.contains(&c.index)).map(|c| c.cpu_set_id).collect();
            if unsafe { SetProcessDefaultCpuSets(h.raw(), ids.as_ptr(), ids.len() as u32) } == 0 {
                return Err(format!("SetProcessDefaultCpuSets: {}", std::io::Error::last_os_error()));
            }
        }
    }
    Ok(())
}

/// Return the process to "all cores, no CPU sets".
pub fn reset_cores(pid: u32, topo: &Topology) -> Result<(), String> {
    let h = open(pid, PROCESS_SET_INFORMATION | PROCESS_SET_LIMITED_INFORMATION)?;
    unsafe { SetProcessDefaultCpuSets(h.raw(), std::ptr::null(), 0) };
    if unsafe { SetProcessAffinityMask(h.raw(), Topology::mask_of(&topo.all())) } == 0 {
        return Err(format!("SetProcessAffinityMask: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

pub fn set_priority(pid: u32, p: Priority) -> Result<(), String> {
    let h = open(pid, PROCESS_SET_INFORMATION)?;
    let class = match p {
        Priority::Idle => IDLE_PRIORITY_CLASS,
        Priority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
        Priority::Normal => NORMAL_PRIORITY_CLASS,
        Priority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
        Priority::High => HIGH_PRIORITY_CLASS,
    };
    if unsafe { SetPriorityClass(h.raw(), class) } == 0 {
        return Err(format!("SetPriorityClass: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_topology() {
        let t = Topology::detect();
        assert!(t.logical_count() >= 1);
        assert_eq!(t.cores.iter().map(|c| c.threads.len()).sum::<usize>(), t.logical_count());
        let presets = t.presets();
        assert_eq!(presets[0].cpus.len(), t.logical_count());
        eprintln!(
            "{}: {} cores / {} threads, hybrid={}, llc={}, presets={:?}",
            t.name,
            t.cores.len(),
            t.logical_count(),
            t.hybrid,
            t.llc_count,
            presets.iter().map(|p| (p.id, p.cpus.len())).collect::<Vec<_>>()
        );
    }

    #[test]
    fn mask_roundtrip() {
        let cpus = vec![0, 2, 5, 31];
        assert_eq!(Topology::cpus_of_mask(Topology::mask_of(&cpus)), cpus);
    }
}

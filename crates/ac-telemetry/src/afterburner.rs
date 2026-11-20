//! Optional sensors from MSI Afterburner's hardware-monitoring shared memory
//! (`MAHMSharedMemory`, a documented read-only interface many tools use).
//!
//! Afterburner reads CPU temperature with its own driver; we only read what it publishes,
//! so AfterCore itself still ships no driver.

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, VirtualQuery, FILE_MAP_READ, MEMORY_BASIC_INFORMATION,
};

use crate::hwinfo::CpuSensors;

const SIGNATURE: u32 = 0x4D41_484D; // 'MAHM'
const NAME_LEN: usize = 260;
const DATA_OFF: usize = NAME_LEN * 5;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub units: String,
    pub value: f32,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

pub fn parse(mem: &[u8]) -> Option<Vec<Entry>> {
    if u32_at(mem, 0)? != SIGNATURE {
        return None;
    }
    let header = u32_at(mem, 8)? as usize;
    let num = u32_at(mem, 12)? as usize;
    let size = u32_at(mem, 16)? as usize;
    if size < DATA_OFF + 4 || num > 10_000 {
        return None;
    }
    let mut out = Vec::with_capacity(num);
    for i in 0..num {
        let e = mem.get(header + i * size..header + (i + 1) * size)?;
        let value = f32::from_le_bytes(e[DATA_OFF..DATA_OFF + 4].try_into().ok()?);
        out.push(Entry { name: cstr(&e[..NAME_LEN]), units: cstr(&e[NAME_LEN..NAME_LEN * 2]), value });
    }
    Some(out)
}

fn valid(v: f32) -> bool {
    v.is_finite() && v > 0.0 && v < 1.0e6
}

pub fn select(es: &[Entry]) -> CpuSensors {
    let find = |n: &str| es.iter().find(|e| e.name.eq_ignore_ascii_case(n) && valid(e.value)).map(|e| e.value);
    // Package temperature, else the hottest per-core value ("CPU1 temperature", …).
    let temp = find("CPU temperature").or_else(|| {
        es.iter()
            .filter(|e| {
                let n = e.name.to_ascii_lowercase();
                n.starts_with("cpu") && n.ends_with("temperature") && valid(e.value)
            })
            .map(|e| e.value)
            .reduce(f32::max)
    });
    CpuSensors { temp, power_w: find("CPU power"), voltage: find("CPU voltage"), fan_rpm: None }
}

pub fn read_entries() -> Option<Vec<Entry>> {
    let name: Vec<u16> = "MAHMSharedMemory".encode_utf16().chain(Some(0)).collect();
    unsafe {
        let h = OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr());
        if h.is_null() {
            return None;
        }
        let view = MapViewOfFile(h, FILE_MAP_READ, 0, 0, 0);
        let r = if view.Value.is_null() {
            None
        } else {
            let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
            let len = if VirtualQuery(view.Value, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) != 0 {
                mbi.RegionSize
            } else {
                0
            };
            let r = parse(std::slice::from_raw_parts(view.Value as *const u8, len));
            UnmapViewOfFile(view);
            r
        };
        CloseHandle(h);
        r
    }
}

/// `None` when Afterburner is not running.
pub fn read() -> Option<CpuSensors> {
    read_entries().map(|e| select(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_package_or_hottest_core() {
        let e = |n: &str, v: f32| Entry { name: n.into(), units: "°C".into(), value: v };
        let s = select(&[e("CPU1 temperature", 61.0), e("CPU2 temperature", 66.0), e("GPU temperature", 50.0)]);
        assert_eq!(s.temp, Some(66.0));
        let s = select(&[e("CPU temperature", 58.0), e("CPU2 temperature", 66.0)]);
        assert_eq!(s.temp, Some(58.0));
        assert_eq!(select(&[e("CPU temperature", f32::MAX)]).temp, None);
    }

    #[test]
    fn live_dump() {
        if let Some(es) = read_entries() {
            for e in es.iter().filter(|e| e.name.to_ascii_lowercase().contains("cpu")) {
                eprintln!("MAHM {:?} = {} {}", e.name, e.value, e.units);
            }
            eprintln!("selected: {:?}", select(&es));
        } else {
            eprintln!("MAHM: not available");
        }
    }
}

//! Optional CPU sensors from HWiNFO's shared memory.
//!
//! Real CPU temperature, core voltage and fan speed are only readable through a kernel
//! driver (MSR / SMBus / Super-I/O). AfterCore deliberately ships no driver: such drivers
//! (WinRing0 and friends) are on Microsoft's vulnerable-driver blocklist and anti-cheats
//! flag them. Instead, if the user runs HWiNFO with "Shared Memory Support" enabled, we read
//! the values it already publishes. This is read-only memory owned by HWiNFO, not the game.

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, VirtualQuery, FILE_MAP_READ, MEMORY_BASIC_INFORMATION,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CpuSensors {
    pub temp: Option<f32>,
    pub power_w: Option<f32>,
    pub voltage: Option<f32>,
    pub fan_rpm: Option<f32>,
}

const SIGNATURE: u32 = 0x5369_5748; // "HWiS"

#[derive(Debug)]
struct Reading {
    kind: u32,
    label: String,
    value: f64,
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn parse(mem: &[u8]) -> Option<Vec<Reading>> {
    if u32_at(mem, 0)? != SIGNATURE {
        return None;
    }
    let off = u32_at(mem, 32)? as usize;
    let size = u32_at(mem, 36)? as usize;
    let num = u32_at(mem, 40)? as usize;
    if size < 292 || num > 100_000 {
        return None;
    }
    let mut out = Vec::with_capacity(num);
    for i in 0..num {
        let r = mem.get(off + i * size..off + (i + 1) * size)?;
        out.push(Reading {
            kind: u32_at(r, 0)?,
            label: cstr(&r[12..140]),
            value: f64::from_le_bytes(r[284..292].try_into().ok()?),
        });
    }
    Some(out)
}

const T_TEMP: u32 = 1;
const T_VOLT: u32 = 2;
const T_FAN: u32 = 3;
const T_POWER: u32 = 5;

fn pick(rs: &[Reading], kind: u32, labels: &[&str]) -> Option<f32> {
    labels.iter().find_map(|l| {
        rs.iter()
            .find(|r| r.kind == kind && r.label.eq_ignore_ascii_case(l) && r.value.is_finite() && r.value > 0.0)
            .map(|r| r.value as f32)
    })
}

fn select(rs: &[Reading]) -> CpuSensors {
    CpuSensors {
        temp: pick(rs, T_TEMP, &["CPU Package", "CPU (Tctl/Tdie)", "CPU Die (average)", "CPU (Tdie)", "Core Max", "CPU"]),
        power_w: pick(rs, T_POWER, &["CPU Package Power", "CPU PPT", "CPU Power"]),
        voltage: pick(
            rs,
            T_VOLT,
            &["Vcore", "CPU Core Voltage (SVI3 TFN)", "CPU Core Voltage (SVI2 TFN)", "Core VIDs (avg)", "CPU Core"],
        ),
        fan_rpm: rs
            .iter()
            .find(|r| r.kind == T_FAN && r.label.to_ascii_lowercase().starts_with("cpu") && r.value > 0.0)
            .map(|r| r.value as f32),
    }
}

/// `None` when HWiNFO is not running or its shared memory is disabled.
pub fn read() -> Option<CpuSensors> {
    let name: Vec<u16> = "Global\\HWiNFO_SENS_SM2".encode_utf16().chain(Some(0)).collect();
    unsafe {
        let h = OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr());
        if h.is_null() {
            return None;
        }
        let view = MapViewOfFile(h, FILE_MAP_READ, 0, 0, 0);
        let result = if view.Value.is_null() {
            None
        } else {
            let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
            let len = if VirtualQuery(view.Value, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) != 0 {
                mbi.RegionSize
            } else {
                0
            };
            let mem = std::slice::from_raw_parts(view.Value as *const u8, len);
            let r = parse(mem).map(|rs| select(&rs));
            UnmapViewOfFile(view);
            r
        };
        CloseHandle(h);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(kind: u32, label: &str, value: f64) -> Vec<u8> {
        let mut r = vec![0u8; 316];
        r[0..4].copy_from_slice(&kind.to_le_bytes());
        r[12..12 + label.len()].copy_from_slice(label.as_bytes());
        r[284..292].copy_from_slice(&value.to_le_bytes());
        r
    }

    #[test]
    fn parses_shared_memory_layout() {
        let rs = [
            reading(T_TEMP, "Core Max", 71.0),
            reading(T_TEMP, "CPU Package", 68.0),
            reading(T_POWER, "CPU Package Power", 112.5),
            reading(T_VOLT, "Vcore", 1.21),
            reading(T_FAN, "CPU", 1350.0),
        ];
        let mut mem = vec![0u8; 44];
        mem[0..4].copy_from_slice(&SIGNATURE.to_le_bytes());
        mem[32..36].copy_from_slice(&44u32.to_le_bytes());
        mem[36..40].copy_from_slice(&316u32.to_le_bytes());
        mem[40..44].copy_from_slice(&(rs.len() as u32).to_le_bytes());
        for r in &rs {
            mem.extend_from_slice(r);
        }
        let s = select(&parse(&mem).unwrap());
        assert_eq!(s.temp, Some(68.0));
        assert_eq!(s.power_w, Some(112.5));
        assert_eq!(s.voltage, Some(1.21));
        assert_eq!(s.fan_rpm, Some(1350.0));
        assert!(parse(&[0u8; 44]).is_none());
    }
}

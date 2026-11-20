//! Minimal dynamic NVML binding (ships with the NVIDIA driver as `nvml.dll`).

use std::ffi::c_void;

use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

type Ret = i32;
type Dev = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Default)]
struct Memory {
    total: u64,
    free: u64,
    used: u64,
}

#[derive(Clone, Debug, Default)]
pub struct NvSample {
    pub name: String,
    pub load: Option<f32>,
    pub temp: Option<f32>,
    pub clock_mhz: Option<f32>,
    pub mem_clock_mhz: Option<f32>,
    pub power_w: Option<f32>,
    pub fan_pct: Option<f32>,
    pub vram_used_gb: Option<f32>,
    pub vram_total_gb: Option<f32>,
    /// `nvmlClocksThrottleReasons` bit mask.
    pub throttle: Option<u64>,
}

pub struct Nvml {
    dev: Dev,
    name: String,
    get_temp: unsafe extern "C" fn(Dev, u32, *mut u32) -> Ret,
    get_util: unsafe extern "C" fn(Dev, *mut Utilization) -> Ret,
    get_clock: unsafe extern "C" fn(Dev, u32, *mut u32) -> Ret,
    get_power: unsafe extern "C" fn(Dev, *mut u32) -> Ret,
    get_fan: unsafe extern "C" fn(Dev, *mut u32) -> Ret,
    get_mem: unsafe extern "C" fn(Dev, *mut Memory) -> Ret,
    get_throttle: Option<unsafe extern "C" fn(Dev, *mut u64) -> Ret>,
}

unsafe impl Send for Nvml {}

macro_rules! sym {
    ($lib:expr, $name:literal) => {{
        let p = GetProcAddress($lib, concat!($name, "\0").as_ptr());
        match p {
            Some(f) => std::mem::transmute(f),
            None => return None,
        }
    }};
}

impl Nvml {
    pub fn load() -> Option<Self> {
        unsafe {
            let mut lib = LoadLibraryW(crate::nvml::w("nvml.dll").as_ptr());
            if lib.is_null() {
                lib = LoadLibraryW(w("C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvml.dll").as_ptr());
            }
            if lib.is_null() {
                return None;
            }
            let init: unsafe extern "C" fn() -> Ret = sym!(lib, "nvmlInit_v2");
            if init() != 0 {
                return None;
            }
            let count: unsafe extern "C" fn(*mut u32) -> Ret = sym!(lib, "nvmlDeviceGetCount_v2");
            let mut n = 0u32;
            if count(&mut n) != 0 || n == 0 {
                return None;
            }
            let by_index: unsafe extern "C" fn(u32, *mut Dev) -> Ret = sym!(lib, "nvmlDeviceGetHandleByIndex_v2");
            let mut dev: Dev = std::ptr::null_mut();
            if by_index(0, &mut dev) != 0 {
                return None;
            }
            let get_name: unsafe extern "C" fn(Dev, *mut u8, u32) -> Ret = sym!(lib, "nvmlDeviceGetName");
            let mut buf = [0u8; 96];
            let name = if get_name(dev, buf.as_mut_ptr(), buf.len() as u32) == 0 {
                let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                String::from_utf8_lossy(&buf[..end]).to_string()
            } else {
                "NVIDIA GPU".into()
            };
            Some(Self {
                dev,
                name,
                get_temp: sym!(lib, "nvmlDeviceGetTemperature"),
                get_util: sym!(lib, "nvmlDeviceGetUtilizationRates"),
                get_clock: sym!(lib, "nvmlDeviceGetClockInfo"),
                get_power: sym!(lib, "nvmlDeviceGetPowerUsage"),
                get_fan: sym!(lib, "nvmlDeviceGetFanSpeed"),
                get_mem: sym!(lib, "nvmlDeviceGetMemoryInfo"),
                get_throttle: GetProcAddress(lib, b"nvmlDeviceGetCurrentClocksThrottleReasons\0".as_ptr())
                    .map(|f| std::mem::transmute::<_, unsafe extern "C" fn(Dev, *mut u64) -> Ret>(f)),
            })
        }
    }

    pub fn sample(&self) -> NvSample {
        let mut s = NvSample { name: self.name.clone(), ..Default::default() };
        unsafe {
            let mut v = 0u32;
            if (self.get_temp)(self.dev, 0, &mut v) == 0 {
                s.temp = Some(v as f32);
            }
            let mut u = Utilization::default();
            if (self.get_util)(self.dev, &mut u) == 0 {
                s.load = Some(u.gpu as f32);
            }
            if (self.get_clock)(self.dev, 0, &mut v) == 0 {
                s.clock_mhz = Some(v as f32);
            }
            if (self.get_clock)(self.dev, 2, &mut v) == 0 {
                s.mem_clock_mhz = Some(v as f32);
            }
            if (self.get_power)(self.dev, &mut v) == 0 {
                s.power_w = Some(v as f32 / 1000.0);
            }
            if (self.get_fan)(self.dev, &mut v) == 0 {
                s.fan_pct = Some(v as f32);
            }
            let mut m = Memory::default();
            if (self.get_mem)(self.dev, &mut m) == 0 {
                s.vram_used_gb = Some(m.used as f32 / 1073741824.0);
                s.vram_total_gb = Some(m.total as f32 / 1073741824.0);
            }
            if let Some(f) = self.get_throttle {
                let mut r = 0u64;
                if f(self.dev, &mut r) == 0 {
                    s.throttle = Some(r);
                }
            }
        }
        s
    }
}

pub(crate) fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

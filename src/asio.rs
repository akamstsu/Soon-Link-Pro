use bridge::*;
use std::{ffi::c_void, ptr};
use winreg::{enums::*, RegKey};
#[derive(Clone, Debug)]
pub struct Device {
    pub name: String,
    pub clsid: Guid,
}
pub fn parse_guid(s: &str) -> Option<Guid> {
    let clean = s.trim_matches(|c| c == '{' || c == '}').replace('-', "");
    if clean.len() != 32 {
        return None;
    }
    let n = u128::from_str_radix(&clean, 16).ok()?;
    let bytes = n.to_be_bytes();
    Some(Guid {
        d1: u32::from_be_bytes(bytes[0..4].try_into().ok()?),
        d2: u16::from_be_bytes(bytes[4..6].try_into().ok()?),
        d3: u16::from_be_bytes(bytes[6..8].try_into().ok()?),
        d4: bytes[8..16].try_into().ok()?,
    })
}
pub fn devices() -> Vec<Device> {
    let mut out = Vec::new();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        if let Ok(k) = RegKey::predef(root).open_subkey("SOFTWARE\\ASIO") {
            for name in k.enum_keys().flatten() {
                if let Ok(v) = k.open_subkey(&name) {
                    if let Ok(s) = v.get_value::<String, _>("CLSID") {
                        if let Some(clsid) = parse_guid(&s) {
                            if !out.iter().any(|d: &Device| d.clsid == clsid) {
                                out.push(Device { name, clsid });
                            }
                        }
                    }
                }
            }
        }
    }
    out
}
pub fn unified_installed() -> bool {
    devices()
        .iter()
        .any(|d| d.clsid == UNIFIED_GUID && d.name == "Soon Link Pro ASIO")
}
pub struct Asio {
    pub ptr: *mut c_void,
    com: bool,
}
macro_rules! call {($self:expr,$idx:expr,$fn:ty $(,$arg:expr)*)=>{{let table=*($self.ptr as *const *const usize);let f: $fn=std::mem::transmute(*table.add($idx));f($self.ptr $(,$arg)*)}};}
impl Asio {
    pub fn open(device: &Device) -> Result<Self, String> {
        unsafe {
            use windows_sys::Win32::System::Com::*;
            let hr = CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32);
            let mut p = ptr::null_mut();
            let id = &device.clsid as *const Guid as *const windows_sys::core::GUID;
            let result = CoCreateInstance(id, ptr::null_mut(), CLSCTX_INPROC_SERVER, id, &mut p);
            if result < 0 {
                if hr >= 0 {
                    CoUninitialize();
                }
                return Err(format!(
                    "ASIO 加载失败 0x{:08X}：{}",
                    result as u32, device.name
                ));
            }
            let x = Self {
                ptr: p,
                com: hr >= 0,
            };
            x.initialize()?;
            Ok(x)
        }
    }
    fn initialize(&self) -> Result<(), String> {
        unsafe {
            let ok = call!(
                self,
                3,
                unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
                ptr::null_mut()
            );
            if ok == 0 {
                Err(self.error())
            } else {
                Ok(())
            }
        }
    }
    pub fn error(&self) -> String {
        unsafe {
            let mut buf = [0u8; 124];
            call!(
                self,
                6,
                unsafe extern "system" fn(*mut c_void, *mut u8),
                buf.as_mut_ptr()
            );
            String::from_utf8_lossy(&buf[..buf.iter().position(|b| *b == 0).unwrap_or(124)])
                .to_string()
        }
    }
    fn result(&self, code: i32) -> Result<(), String> {
        if code == 0 || code == 0x3f4847a0 {
            Ok(())
        } else {
            Err(format!("ASIO {code}: {}", self.error()))
        }
    }
    pub fn channels(&self) -> Result<(i32, i32), String> {
        unsafe {
            let (mut i, mut o) = (0, 0);
            self.result(call!(
                self,
                9,
                unsafe extern "system" fn(*mut c_void, *mut i32, *mut i32) -> i32,
                &mut i,
                &mut o
            ))?;
            Ok((i, o))
        }
    }
    pub fn sizes(&self) -> Result<(i32, i32, i32, i32), String> {
        unsafe {
            let (mut a, mut b, mut c, mut d) = (0, 0, 0, 0);
            self.result(call!(
                self,
                11,
                unsafe extern "system" fn(
                    *mut c_void,
                    *mut i32,
                    *mut i32,
                    *mut i32,
                    *mut i32,
                ) -> i32,
                &mut a,
                &mut b,
                &mut c,
                &mut d
            ))?;
            Ok((a, b, c, d))
        }
    }
    pub fn sample_rate(&self) -> Result<f64, String> {
        unsafe {
            let mut r = 0.0;
            self.result(call!(
                self,
                13,
                unsafe extern "system" fn(*mut c_void, *mut f64) -> i32,
                &mut r
            ))?;
            Ok(r)
        }
    }
    pub fn set_rate(&self, r: f64) -> Result<(), String> {
        unsafe {
            self.result(call!(
                self,
                14,
                unsafe extern "system" fn(*mut c_void, f64) -> i32,
                r
            ))
        }
    }
    pub fn channel_info(&self, input: bool, channel: i32) -> Result<ChannelInfo, String> {
        unsafe {
            let mut c = ChannelInfo {
                channel,
                input: input as i32,
                active: 0,
                group: 0,
                sample_type: 0,
                name: [0; 32],
            };
            self.result(call!(
                self,
                18,
                unsafe extern "system" fn(*mut c_void, *mut ChannelInfo) -> i32,
                &mut c
            ))?;
            Ok(c)
        }
    }
    pub fn create(&self, b: &mut [BufferInfo], frames: usize, c: &Callbacks) -> Result<(), String> {
        unsafe {
            self.result(call!(
                self,
                19,
                unsafe extern "system" fn(
                    *mut c_void,
                    *mut BufferInfo,
                    i32,
                    i32,
                    *const Callbacks,
                ) -> i32,
                b.as_mut_ptr(),
                b.len() as i32,
                frames as i32,
                c
            ))
        }
    }
    pub fn start(&self) -> Result<(), String> {
        unsafe {
            self.result(call!(
                self,
                7,
                unsafe extern "system" fn(*mut c_void) -> i32
            ))
        }
    }
    pub fn stop(&self) {
        unsafe {
            call!(self, 8, unsafe extern "system" fn(*mut c_void) -> i32);
        }
    }
    pub fn dispose(&self) {
        unsafe {
            call!(self, 20, unsafe extern "system" fn(*mut c_void) -> i32);
        }
    }
    pub fn control_panel(&self) {
        unsafe {
            call!(self, 21, unsafe extern "system" fn(*mut c_void) -> i32);
        }
    }
}
impl Drop for Asio {
    fn drop(&mut self) {
        self.stop();
        self.dispose();
        unsafe {
            call!(self, 2, unsafe extern "system" fn(*mut c_void) -> u32);
            if self.com {
                windows_sys::Win32::System::Com::CoUninitialize();
            }
        }
    }
}

pub fn supported(t: i32) -> bool {
    matches!(t,0..=4|16..=20)
}
pub unsafe fn read_sample(p: *const c_void, t: i32, i: usize) -> f32 {
    let b = p as *const u8;
    match t {
        0 | 16 => {
            let mut v = [0; 2];
            ptr::copy_nonoverlapping(b.add(i * 2), v.as_mut_ptr(), 2);
            if t == 0 {
                i16::from_be_bytes(v) as f32 / 32768.0
            } else {
                i16::from_le_bytes(v) as f32 / 32768.0
            }
        }
        1 | 17 => {
            let b = b.add(i * 3);
            let v = if t == 17 {
                (*b as i32) | ((*b.add(1) as i32) << 8) | ((*b.add(2) as i32) << 16)
            } else {
                ((*b as i32) << 16) | ((*b.add(1) as i32) << 8) | *b.add(2) as i32
            };
            ((v << 8) >> 8) as f32 / 8388608.0
        }
        2 | 18 => {
            let mut v = [0; 4];
            ptr::copy_nonoverlapping(b.add(i * 4), v.as_mut_ptr(), 4);
            if t == 2 {
                i32::from_be_bytes(v) as f32 / 2147483648.0
            } else {
                i32::from_le_bytes(v) as f32 / 2147483648.0
            }
        }
        3 | 19 => {
            let mut v = [0; 4];
            ptr::copy_nonoverlapping(b.add(i * 4), v.as_mut_ptr(), 4);
            f32::from_bits(if t == 3 {
                u32::from_be_bytes(v)
            } else {
                u32::from_le_bytes(v)
            })
        }
        4 | 20 => {
            let mut v = [0; 8];
            ptr::copy_nonoverlapping(b.add(i * 8), v.as_mut_ptr(), 8);
            f64::from_bits(if t == 4 {
                u64::from_be_bytes(v)
            } else {
                u64::from_le_bytes(v)
            }) as f32
        }
        _ => 0.0,
    }
}
pub unsafe fn write_sample(p: *mut c_void, t: i32, i: usize, value: f32) {
    let v = if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let b = p as *mut u8;
    match t {
        0 | 16 => {
            let n = (v * 32767.0).round() as i16;
            let bytes = if t == 0 {
                n.to_be_bytes()
            } else {
                n.to_le_bytes()
            };
            ptr::copy_nonoverlapping(bytes.as_ptr(), b.add(i * 2), 2);
        }
        1 | 17 => {
            let n = (v * 8388607.0).round() as i32;
            let bytes = if t == 1 {
                n.to_be_bytes()
            } else {
                n.to_le_bytes()
            };
            ptr::copy_nonoverlapping(
                if t == 1 {
                    bytes.as_ptr().add(1)
                } else {
                    bytes.as_ptr()
                },
                b.add(i * 3),
                3,
            );
        }
        2 | 18 => {
            let n = (v as f64 * PCM_I32_PEAK).round() as i32;
            let bytes = if t == 2 {
                n.to_be_bytes()
            } else {
                n.to_le_bytes()
            };
            ptr::copy_nonoverlapping(bytes.as_ptr(), b.add(i * 4), 4);
        }
        3 | 19 => {
            let bytes = if t == 3 {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            };
            ptr::copy_nonoverlapping(bytes.as_ptr(), b.add(i * 4), 4);
        }
        4 | 20 => {
            let bytes = if t == 4 {
                (v as f64).to_be_bytes()
            } else {
                (v as f64).to_le_bytes()
            };
            ptr::copy_nonoverlapping(bytes.as_ptr(), b.add(i * 8), 8);
        }
        _ => (),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pcm_roundtrip() {
        for t in [0, 1, 2, 3, 4, 16, 17, 18, 19, 20] {
            let mut bytes = [0u8; 64];
            for (i, v) in [-1.0, -0.25, 0.0, 0.75, 1.0].iter().enumerate() {
                unsafe {
                    write_sample(bytes.as_mut_ptr().cast(), t, i, *v);
                    let r = read_sample(bytes.as_ptr().cast(), t, i);
                    assert!((r - v).abs() < 0.00004, "type {t}: {r} != {v}");
                }
            }
        }
    }
}

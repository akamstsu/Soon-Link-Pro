use super::*;
#[repr(C)]
pub struct FrameRing {
    read: AtomicU32,
    write: AtomicU32,
    pub dropped: AtomicU32,
    samples: [AtomicU32; HARDWARE_CAPACITY * MAX_HARDWARE_CHANNELS],
}
impl FrameRing {
    pub fn available(&self) -> usize {
        (self
            .write
            .load(Ordering::Acquire)
            .wrapping_sub(self.read.load(Ordering::Relaxed)) as usize)
            .min(HARDWARE_CAPACITY)
    }
    pub fn discard(&self) {
        self.read
            .store(self.write.load(Ordering::Acquire), Ordering::Release);
    }
    pub fn push(&self, samples: &[f32]) -> bool {
        if samples.len() > MAX_HARDWARE_CHANNELS {
            return false;
        }
        let w = self.write.load(Ordering::Relaxed);
        if w.wrapping_sub(self.read.load(Ordering::Acquire)) as usize >= HARDWARE_CAPACITY {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let base = (w as usize % HARDWARE_CAPACITY) * MAX_HARDWARE_CHANNELS;
        for (c, v) in samples.iter().enumerate() {
            self.samples[base + c].store(v.to_bits(), Ordering::Relaxed);
        }
        self.write.store(w.wrapping_add(1), Ordering::Release);
        true
    }
    pub fn pop(&self, samples: &mut [f32]) -> bool {
        samples.fill(0.0);
        if samples.len() > MAX_HARDWARE_CHANNELS {
            return false;
        }
        let r = self.read.load(Ordering::Relaxed);
        let available = self.write.load(Ordering::Acquire).wrapping_sub(r) as usize;
        if available == 0 || available > HARDWARE_CAPACITY {
            return false;
        }
        let base = (r as usize % HARDWARE_CAPACITY) * MAX_HARDWARE_CHANNELS;
        for (c, v) in samples.iter_mut().enumerate() {
            *v = f32::from_bits(self.samples[base + c].load(Ordering::Relaxed));
        }
        self.read.store(r.wrapping_add(1), Ordering::Release);
        true
    }
}
#[repr(C)]
pub struct HardwareShared {
    pub generation: AtomicU64,
    pub input_channels: AtomicU32,
    pub output_channels: AtomicU32,
    pub client_buffer: AtomicU32,
    pub virtual_buses: AtomicU32,
    pub engine_frames: AtomicU64,
    pub to_daw: FrameRing,
    pub from_daw: FrameRing,
}
pub struct HardwareMapping {
    handle: HANDLE,
    ptr: *mut HardwareShared,
}
unsafe impl Send for HardwareMapping {}
unsafe impl Sync for HardwareMapping {}
impl HardwareMapping {
    pub fn open() -> Result<Self, String> {
        unsafe {
            let name: Vec<_> = "Local\\SoonLinkProHardware_v2\0".encode_utf16().collect();
            let handle = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                0,
                std::mem::size_of::<HardwareShared>() as u32,
                name.as_ptr(),
            );
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let view = MapViewOfFile(
                handle,
                FILE_MAP_ALL_ACCESS,
                0,
                0,
                std::mem::size_of::<HardwareShared>(),
            );
            if view.Value.is_null() {
                CloseHandle(handle);
                return Err("Cannot map physical audio channels".into());
            }
            Ok(Self {
                handle,
                ptr: view.Value.cast(),
            })
        }
    }
    pub fn shared(&self) -> &HardwareShared {
        unsafe { &*self.ptr }
    }
}
impl Drop for HardwareMapping {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr.cast(),
            });
            CloseHandle(self.handle);
        }
    }
}

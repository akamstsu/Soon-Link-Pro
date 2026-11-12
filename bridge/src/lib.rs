use std::{
    ffi::c_void,
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Memory::{
            CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
            MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
        },
        SystemInformation::GetTickCount64,
    },
};

mod config;
pub use config::*;
const _: () = assert!(CAPACITY.is_power_of_two());
pub const CLSIDS: [&str; LEGACY_BUSES] = [
    "{A1258C01-6F49-48F8-A501-663051290001}",
    "{A1258C02-6F49-48F8-A501-663051290002}",
    "{A1258C03-6F49-48F8-A501-663051290003}",
    "{A1258C04-6F49-48F8-A501-663051290004}",
];
pub const MAGIC: u32 = 0x50425431;
// The saved normalized control uses a -40..0 dB scale, with zero meaning mute.
// Keep the scale stable when loading existing profiles.
pub fn fader_db(position: f32) -> f32 {
    match position {
        value if value.is_nan() || value <= 0.0 => f32::NEG_INFINITY,
        value => -40.0 * (1.0 - value.min(1.0)),
    }
}

pub fn gain(position: f32) -> f32 {
    let decibels = fader_db(position);
    if decibels == f32::NEG_INFINITY {
        return 0.0;
    }
    (decibels * (std::f32::consts::LN_10 / 20.0)).exp()
}
pub fn fader_label(fader: f64) -> String {
    let db = fader_db(fader as f32);
    if db.is_finite() {
        format!("{db:.1} dB")
    } else {
        "−∞ dB".into()
    }
}
pub fn clock_ms() -> u64 {
    unsafe { GetTickCount64() }
}

#[repr(C)]
pub struct Ring {
    read: AtomicU32,
    write: AtomicU32,
    pub dropped: AtomicU32,
    samples: [AtomicU32; CAPACITY * CHANNELS],
}
/// Shared-memory SPSC queue: one producer and one consumer per instance.
/// Publish the write cursor after samples, and the read cursor after consumption.
impl Ring {
    pub fn push(&self, frame: [f32; 2]) -> bool {
        self.push_frames(std::iter::once(frame))
    }
    pub fn push_limited(&self, frame: [f32; 2], limit: usize) -> bool {
        self.push_frames_limited(std::iter::once(frame), limit)
    }
    pub fn push_frames(&self, frames: impl ExactSizeIterator<Item = [f32; 2]>) -> bool {
        self.push_frames_limited(frames, CAPACITY)
    }
    pub fn push_frames_limited(
        &self,
        frames: impl ExactSizeIterator<Item = [f32; 2]>,
        limit: usize,
    ) -> bool {
        let requested = frames.len();
        if requested == 0 {
            return false;
        }
        let head = self.write.load(Ordering::Relaxed);
        let tail = self.read.load(Ordering::Acquire);
        let occupied = head.wrapping_sub(tail) as usize;
        let budget = limit.min(CAPACITY);
        if occupied > budget || requested > budget - occupied {
            self.dropped
                .fetch_add(requested.min(u32::MAX as usize) as u32, Ordering::Relaxed);
            return false;
        }
        let mut next = head;
        for frame in frames {
            let offset = (next as usize & (CAPACITY - 1)) * CHANNELS;
            for (channel, sample) in frame.into_iter().enumerate() {
                self.samples[offset + channel].store(sample.to_bits(), Ordering::Relaxed);
            }
            next = next.wrapping_add(1);
        }
        self.write.store(next, Ordering::Release);
        true
    }
    pub fn push_silence(&self, frames: usize) -> bool {
        self.push_frames((0..frames).map(|_| [0.0; 2]))
    }
    pub fn pop_frames(&self, requested: usize, mut deliver: impl FnMut(usize, [f32; 2])) -> usize {
        let tail = self.read.load(Ordering::Relaxed);
        let head = self.write.load(Ordering::Acquire);
        let available = head.wrapping_sub(tail) as usize;
        if available > CAPACITY {
            return 0;
        }
        let count = available.min(requested);
        for frame in 0..count {
            let offset = (tail.wrapping_add(frame as u32) as usize & (CAPACITY - 1)) * CHANNELS;
            let samples = std::array::from_fn(|channel| {
                f32::from_bits(self.samples[offset + channel].load(Ordering::Relaxed))
            });
            deliver(frame, samples);
        }
        if count != 0 {
            self.read
                .store(tail.wrapping_add(count as u32), Ordering::Release);
        }
        count
    }
    pub fn pop(&self) -> Option<[f32; 2]> {
        let mut result = None;
        self.pop_frames(1, |_, frame| result = Some(frame));
        result
    }
    pub fn discard(&self) {
        let head = self.write.load(Ordering::Acquire);
        self.read.store(head, Ordering::Release);
    }
    pub fn available(&self) -> u32 {
        let tail = self.read.load(Ordering::Relaxed);
        let head = self.write.load(Ordering::Acquire);
        let available = head.wrapping_sub(tail);
        if available <= CAPACITY as u32 {
            available
        } else {
            0
        }
    }
}
#[repr(C)]
pub struct Shared {
    pub magic: AtomicU32,
    pub rate: AtomicU32,
    pub owner: AtomicU32,
    pub engine_owner: AtomicU32,
    pub heartbeat: AtomicU64,
    pub client_heartbeat: AtomicU64,
    pub to_engine: Ring,
    pub to_client: Ring,
}
impl Shared {
    pub fn engine_live(&self) -> bool {
        let t = self.heartbeat.load(Ordering::Acquire);
        t > 0 && clock_ms().saturating_sub(t) < 1500
    }
    pub fn client_live(&self) -> bool {
        self.owner.load(Ordering::Acquire) != 0
            && clock_ms().saturating_sub(self.client_heartbeat.load(Ordering::Acquire)) < 1500
    }
}
pub struct Mapping {
    handle: HANDLE,
    ptr: *mut Shared,
}
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}
impl Mapping {
    pub fn open(bus: usize) -> Result<Self, String> {
        let name: Vec<u16> = format!("Local\\PatchbayAsioBus{}_v1\0", bus + 1)
            .encode_utf16()
            .collect();
        unsafe {
            let handle = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                0,
                std::mem::size_of::<Shared>() as u32,
                name.as_ptr(),
            );
            if handle.is_null() {
                return Err(format!(
                    "Shared audio mapping: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let view = MapViewOfFile(
                handle,
                FILE_MAP_ALL_ACCESS,
                0,
                0,
                std::mem::size_of::<Shared>(),
            );
            if view.Value.is_null() {
                CloseHandle(handle);
                return Err("Cannot map shared audio".into());
            }
            let ptr = view.Value as *mut Shared;
            // New file mappings are zero-filled. Atomic magic publishes the protocol version.
            (*ptr)
                .rate
                .compare_exchange(0, DEFAULT_RATE, Ordering::AcqRel, Ordering::Acquire)
                .ok();
            (*ptr)
                .magic
                .compare_exchange(0, MAGIC, Ordering::AcqRel, Ordering::Acquire)
                .ok();
            if (*ptr).magic.load(Ordering::Acquire) != MAGIC {
                UnmapViewOfFile(view);
                CloseHandle(handle);
                return Err("Bridge protocol mismatch".into());
            }
            Ok(Self { handle, ptr })
        }
    }
    pub fn shared(&self) -> &Shared {
        unsafe { &*self.ptr }
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr as *mut c_void,
            });
            CloseHandle(self.handle);
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guid {
    pub d1: u32,
    pub d2: u16,
    pub d3: u16,
    pub d4: [u8; 8],
}
pub const IUNKNOWN: Guid = Guid {
    d1: 0,
    d2: 0,
    d3: 0,
    d4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
pub const ICLASSFACTORY: Guid = Guid { d1: 1, ..IUNKNOWN };
pub fn bus_guid(i: usize) -> Guid {
    Guid {
        d1: 0xa1258c01 + i as u32,
        d2: 0x6f49,
        d3: 0x48f8,
        d4: [0xa5, 1, 0x66, 0x30, 0x51, 0x29, 0, i as u8 + 1],
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BufferInfo {
    pub input: i32,
    pub channel: i32,
    pub buffers: [*mut c_void; 2],
}
unsafe impl Send for BufferInfo {}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Callbacks {
    pub switch: Option<unsafe extern "C" fn(i32, i32)>,
    pub rate_changed: Option<unsafe extern "C" fn(f64)>,
    pub message: Option<unsafe extern "C" fn(i32, i32, *mut c_void, *mut f64) -> i32>,
    pub time_switch: Option<unsafe extern "C" fn(*mut c_void, i32, i32) -> *mut c_void>,
}
#[repr(C)]
pub struct ChannelInfo {
    pub channel: i32,
    pub input: i32,
    pub active: i32,
    pub group: i32,
    pub sample_type: i32,
    pub name: [u8; 32],
}
#[repr(C)]
pub struct Samples {
    pub hi: u32,
    pub lo: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn empty_ring() -> Box<Ring> {
        unsafe {
            let layout = std::alloc::Layout::new::<Ring>();
            let pointer = std::alloc::alloc_zeroed(layout);
            if pointer.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            Box::from_raw(pointer as *mut Ring)
        }
    }
    #[test]
    fn logical_limit_refuses_whole_blocks_without_overwriting() {
        let ring = empty_ring();
        assert!(ring.push_frames_limited([[1.0; 2], [2.0; 2]].into_iter(), 3));
        assert!(!ring.push_frames_limited([[3.0; 2], [4.0; 2]].into_iter(), 3));
        assert_eq!(ring.available(), 2);
        assert_eq!(ring.pop(), Some([1.0; 2]));
        assert!(ring.push_frames_limited([[3.0; 2], [4.0; 2]].into_iter(), 3));
        let mut samples = Vec::new();
        assert_eq!(ring.pop_frames(9, |_, frame| samples.push(frame[0])), 3);
        assert_eq!(samples, vec![2.0, 3.0, 4.0]);
        assert!(!ring.push_limited([1.0; 2], 0));
        assert!(ring.push_frames_limited((0..CAPACITY).map(|_| [0.0; 2]), usize::MAX));
        assert!(!ring.push_limited([0.0; 2], usize::MAX));
    }
    #[test]
    fn bounded_batch_read_wraps_and_preserves_unavailable_tail() {
        let ring = empty_ring();
        ring.read.store((CAPACITY - 1) as u32, Ordering::Relaxed);
        ring.write.store((CAPACITY - 1) as u32, Ordering::Relaxed);
        ring.push_frames([[1.0, -1.0], [2.0, -2.0]].into_iter());
        let mut output = [[99.0; 2]; 4];
        assert_eq!(ring.pop_frames(4, |i, frame| output[i] = frame), 2);
        assert_eq!(output, [[1.0, -1.0], [2.0, -2.0], [99.0; 2], [99.0; 2]]);
        assert_eq!(ring.available(), 0);
        ring.read.store(u32::MAX - 1, Ordering::Relaxed);
        ring.write.store(u32::MAX - 1, Ordering::Relaxed);
        ring.push_frames([[3.0; 2], [4.0; 2], [5.0; 2]].into_iter());
        let mut output = [[0.0; 2]; 3];
        assert_eq!(ring.pop_frames(3, |i, frame| output[i] = frame), 3);
        assert_eq!(output, [[3.0; 2], [4.0; 2], [5.0; 2]]);
        assert_eq!(ring.available(), 0);
    }
    #[test]
    fn explicit_silence_is_a_whole_zero_block() {
        let ring = empty_ring();
        assert!(!ring.push_silence(0));
        assert!(ring.push_silence(3));
        assert_eq!(
            ring.pop_frames(5, |_, frame| assert_eq!(frame, [0.0; 2])),
            3
        );
        ring.push_frames((0..CAPACITY - 1).map(|_| [0.25; 2]));
        assert!(!ring.push_silence(2));
        assert_eq!(ring.available(), (CAPACITY - 1) as u32);
        assert_eq!(ring.pop(), Some([0.25; 2]));
    }
    #[test]
    fn invalid_shared_indices_do_not_expose_stale_samples() {
        let ring = empty_ring();
        ring.write.store(CAPACITY as u32 + 1, Ordering::Relaxed);
        assert_eq!(ring.available(), 0);
        assert_eq!(ring.pop(), None);
        assert_eq!(
            ring.pop_frames(4, |_, _| panic!("invalid queue must not deliver data")),
            0
        );
        ring.discard();
        assert!(ring.push([0.5, -0.5]));
        assert_eq!(ring.pop(), Some([0.5, -0.5]));
    }
    #[test]
    fn fader_law() {
        for (fader, expected, db) in [
            (0.25, 0.031622775, -30.0),
            (0.5, 0.1, -20.0),
            (0.75, 0.31622776, -10.0),
            (1.0, 1.0, 0.0),
        ] {
            assert!((gain(fader) - expected).abs() < 1e-6);
            assert!((fader_db(fader) - db).abs() < 1e-6);
        }
        assert_eq!(gain(-0.5), 0.0);
        assert_eq!(gain(0.0), 0.0);
        assert_eq!(gain(1.5), 1.0);
        assert_eq!(gain(f32::NAN), 0.0);
        assert_eq!(fader_label(0.0), "−∞ dB");
        assert_eq!(fader_label(0.5), "-20.0 dB");
    }
    #[test]
    fn writes_are_whole_blocks() {
        let ring = empty_ring();
        assert!(!ring.push_frames(std::iter::empty()));
        assert!(ring.push_frames((0..CAPACITY - 1).map(|i| [i as f32; 2])));
        assert!(!ring.push_frames([[99.0; 2], [100.0; 2]].into_iter()));
        assert_eq!(ring.available(), (CAPACITY - 1) as u32);
        assert_eq!(ring.dropped.load(Ordering::Relaxed), 2);
        for i in 0..CAPACITY - 1 {
            assert_eq!(ring.pop(), Some([i as f32; 2]));
        }
        assert_eq!(ring.pop(), None);
        assert!(ring.push_frames([[0.25, -0.25], [0.5, -0.5]].into_iter()));
        assert_eq!(ring.pop(), Some([0.25, -0.25]));
        assert_eq!(ring.pop(), Some([0.5, -0.5]));
    }
    #[test]
    fn ring_bounds_and_wrap() {
        let ring = empty_ring();
        for _ in 0..3 {
            for i in 0..CAPACITY {
                assert!(ring.push([i as f32, -(i as f32)]));
            }
            assert!(!ring.push([0.0; 2]));
            for i in 0..CAPACITY {
                assert_eq!(ring.pop(), Some([i as f32, -(i as f32)]));
            }
            assert_eq!(ring.pop(), None);
        }
    }
}

/// One consumer per ring. Small continuous rate adjustment follows the hardware
/// clock; bounded recovery drops old frames after a stalled consumer.
pub struct AdaptiveReader {
    ready: bool,
    phase: f64,
    step: f64,
    a: [f32; 2],
    b: [f32; 2],
    target: usize,
    rate: u32,
    pub underruns: u64,
    pub resyncs: u64,
}
impl AdaptiveReader {
    pub fn new(rate: u32, buffer: usize) -> Self {
        Self {
            ready: false,
            phase: 0.0,
            step: 1.0,
            a: [0.0; 2],
            b: [0.0; 2],
            target: buffer.max(rate as usize / 125).min(2048),
            rate,
            underruns: 0,
            resyncs: 0,
        }
    }
    pub fn reset(&mut self) {
        self.ready = false;
        self.phase = 0.0;
        self.step = 1.0;
    }
    pub fn next(&mut self, ring: &Ring) -> [f32; 2] {
        let available = ring.available() as usize;
        if available > self.target + self.rate as usize / 25 {
            for _ in self.target..available {
                let _ = ring.pop();
            }
            self.reset();
            self.resyncs += 1;
        }
        if !self.ready {
            if (ring.available() as usize) < self.target.max(2) {
                return [0.0; 2];
            }
            self.a = ring.pop().unwrap_or([0.0; 2]);
            self.b = ring.pop().unwrap_or([0.0; 2]);
            self.ready = true;
        }
        let error = ring.available() as f64 - self.target as f64;
        let desired = 1.0 + (error / self.rate as f64 * 0.1).clamp(-0.005, 0.005);
        self.step += (desired - self.step) * 0.0001;
        let out = std::array::from_fn(|i| self.a[i] + (self.b[i] - self.a[i]) * self.phase as f32);
        self.phase += self.step;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
            self.a = self.b;
            match ring.pop() {
                Some(v) => self.b = v,
                None => {
                    self.reset();
                    self.underruns += 1;
                    break;
                }
            }
        }
        out
    }
}

pub const UNIFIED_CLSID: &str = "{A1258C10-6F49-48F8-A501-663051290010}";
pub const UNIFIED_GUID: Guid = Guid {
    d1: 0xA1258C10,
    d2: 0x6F49,
    d3: 0x48F8,
    d4: [0xA5, 0x01, 0x66, 0x30, 0x51, 0x29, 0x00, 0x10],
};

mod hardware;
pub use hardware::*;
mod lifecycle;
pub use lifecycle::*;

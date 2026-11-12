//! Transport layout and application defaults.
//! Changes to layout constants require upgrading the mapping protocol on both ends.

pub const LEGACY_BUSES: usize = 4;
pub const MAX_APPLICATIONS: usize = 32;
pub const CHANNELS: usize = 2;
pub const CAPACITY: usize = 8192;
pub const MAX_HARDWARE_CHANNELS: usize = 128;
pub const HARDWARE_CAPACITY: usize = 4096;
pub const DEFAULT_RATE: u32 = 48000;
pub const DEFAULT_BUFFER: usize = 256;
pub const MIN_BUFFER: usize = 64;
pub const MAX_BUFFER: usize = 2048;
pub const PCM_I32_PEAK: f64 = i32::MAX as f64;

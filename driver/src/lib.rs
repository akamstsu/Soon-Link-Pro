#![allow(non_snake_case)]
use bridge::*;
use std::{
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use windows_sys::Win32::{
    Media::{timeBeginPeriod, timeEndPeriod},
    System::{
        LibraryLoader::{
            GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        },
        Threading::GetCurrentProcessId,
    },
};
const OK: i32 = 0;
const INVALID: i32 = -998;
const NOT_PRESENT: i32 = -1000;
const BAD_MODE: i32 = -997;
const NO_INTERFACE: i32 = 0x80004002u32 as i32;
static OBJECTS: AtomicU32 = AtomicU32::new(0);
static LOCKS: AtomicU32 = AtomicU32::new(0);
fn text(dst: *mut u8, s: &str, n: usize) {
    unsafe {
        ptr::write_bytes(dst, 0, n);
        let mut length = s.len().min(n - 1);
        while !s.is_char_boundary(length) {
            length -= 1;
        }
        ptr::copy_nonoverlapping(s.as_ptr(), dst, length);
    }
}
#[repr(C)]
struct Driver {
    vtable: *const usize,
    refs: AtomicU32,
    mappings: Vec<Mapping>,
    hardware: HardwareMapping,
    lifecycle: Option<Lifecycle>,
    virtual_buses: usize,
    input_channels: usize,
    output_channels: usize,
    generation: u64,
    rate: f64,
    frames: usize,
    buffers: Vec<(bool, usize, [Vec<f32>; 2])>,
    callbacks: Option<Callbacks>,
    running: Arc<AtomicBool>,
    position: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
    reset_cancel: Arc<AtomicBool>,
    reset_worker: Option<JoinHandle<()>>,
    error: String,
}
unsafe extern "system" fn qi(d: *mut Driver, id: *const Guid, out: *mut *mut c_void) -> i32 {
    if id.is_null() || out.is_null() {
        return 0x80004003u32 as i32;
    }
    *out = ptr::null_mut();
    if *id == IUNKNOWN || *id == UNIFIED_GUID {
        *out = d.cast();
        add(d);
        0
    } else {
        NO_INTERFACE
    }
}
unsafe extern "system" fn add(d: *mut Driver) -> u32 {
    (*d).refs.fetch_add(1, Ordering::Relaxed) + 1
}
unsafe extern "system" fn release(d: *mut Driver) -> u32 {
    let n = (*d).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n == 0 {
        stop(d);
        drop(Box::from_raw(d));
        OBJECTS.fetch_sub(1, Ordering::Relaxed);
    }
    n
}
unsafe extern "system" fn init(d: *mut Driver, _: *mut c_void) -> i32 {
    lifecycle_log(&format!(
        "ASIO R2.4.3 init: host_pid={}",
        GetCurrentProcessId()
    ));
    let started = std::time::Instant::now();
    let x = &mut *d;
    let session = match Lifecycle::open().and_then(|session| {
        session.register_host()?;
        Ok(session)
    }) {
        Ok(session) => session,
        Err(error) => {
            x.error = format!("ASIO host registration failed: {error}");
            lifecycle_log(&x.error);
            return 0;
        }
    };
    if !session.mixer_running() && !x.mappings[0].shared().engine_live() {
        if let Err(error) = launch_mixer() {
            x.error = error;
            lifecycle_log(&x.error);
            return 0;
        }
    }
    x.lifecycle = Some(session);
    // This is ASIO initialization, never an audio callback or DllMain. Give the
    // background engine time to open the configured physical device.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while (!x.mappings[0].shared().engine_live()
        || x.hardware.shared().generation.load(Ordering::Acquire) == 0)
        && std::time::Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(20));
    }
    let h = x.hardware.shared();
    let generation = h.generation.load(Ordering::Acquire);
    if !x.mappings[0].shared().engine_live() || generation == 0 {
        if let Some(session) = &x.lifecycle {
            session.request_show();
        }
        x.error = "Soon Link Pro did not become ready within 20 seconds; check its audio settings or resume audio".into();
        lifecycle_log(&format!(
            "ASIO init timeout: mixer_running={}, engine_live={}, generation={generation}",
            x.lifecycle.as_ref().is_some_and(Lifecycle::mixer_running),
            x.mappings[0].shared().engine_live()
        ));
        return 0;
    }
    x.input_channels = h.input_channels.load(Ordering::Acquire) as usize;
    x.output_channels = h.output_channels.load(Ordering::Acquire) as usize;
    if x.input_channels > MAX_HARDWARE_CHANNELS || x.output_channels > MAX_HARDWARE_CHANNELS {
        return 0;
    }
    x.virtual_buses = h.virtual_buses.load(Ordering::Acquire) as usize;
    if x.virtual_buses
        .checked_mul(2)
        .is_none_or(|n| n > (i32::MAX as usize).saturating_sub(MAX_HARDWARE_CHANNELS))
    {
        return 0;
    }
    x.mappings = match (0..x.virtual_buses.max(1))
        .map(Mapping::open)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(m) => m,
        Err(e) => {
            x.error = e;
            return 0;
        }
    };
    x.generation = generation;
    x.rate = x.mappings[0].shared().rate.load(Ordering::Acquire) as f64;
    lifecycle_log(&format!(
        "ASIO init ready: elapsed_ms={}, inputs={}, outputs={}, virtual_buses={}, rate={}",
        started.elapsed().as_millis(),
        x.input_channels,
        x.output_channels,
        x.virtual_buses,
        x.rate
    ));
    1
}
// Startup diagnostics only. Never called from an audio callback.
fn lifecycle_log(message: &str) {
    use std::io::Write;
    let Some(root) = std::env::var_os("LOCALAPPDATA") else {
        return;
    };
    let directory = std::path::PathBuf::from(root).join("SoonLinkPro");
    let _ = std::fs::create_dir_all(&directory);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("soon-link-pro-virtual-devices.log"))
    {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(file, "{timestamp} {message}");
    }
}
fn launch_mixer() -> Result<(), String> {
    use std::{os::windows::process::CommandExt, path::PathBuf, process::Command};
    let mut module = ptr::null_mut();
    let mut path = [0u16; 32768];
    let count = unsafe {
        if GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            init as *const u16,
            &mut module,
        ) == 0
        {
            return Err("Cannot locate ASIO module".into());
        }
        GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32)
    } as usize;
    if count == 0 || count >= path.len() {
        return Err("Cannot read ASIO module path".into());
    }
    let exe =
        PathBuf::from(String::from_utf16_lossy(&path[..count])).with_file_name("SoonLinkPro.exe");
    if !exe.is_file() {
        return Err(
            "SoonLinkPro.exe is missing beside the ASIO DLL; repair the installation".into(),
        );
    }
    lifecycle_log(&format!("ASIO launching mixer: {}", exe.display()));
    Command::new(exe)
        .arg("--asio-managed")
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| format!("Cannot start Soon Link Pro: {e}"))?;
    Ok(())
}
unsafe extern "system" fn name(d: *mut Driver, s: *mut u8) {
    let _ = d;
    text(s, "Soon Link Pro ASIO", 32);
}
unsafe extern "system" fn version(_: *mut Driver) -> i32 {
    1
}
unsafe extern "system" fn error(d: *mut Driver, s: *mut u8) {
    text(s, &(*d).error, 124);
}
unsafe extern "system" fn start(d: *mut Driver) -> i32 {
    let x = &mut *d;
    if x.running.load(Ordering::Acquire) {
        return OK;
    }
    if x.buffers.is_empty() || x.callbacks.and_then(|c| c.switch).is_none() {
        x.error = "Create buffers and provide bufferSwitch first".into();
        return INVALID;
    }
    if x.hardware.shared().generation.load(Ordering::Acquire) != x.generation || x.generation == 0 {
        x.error = "Mixer layout changed; reopen Soon Link Pro ASIO in the DAW".into();
        return NOT_PRESENT;
    }
    if let Some(worker) = x.worker.take() {
        let _ = worker.join();
    }
    cancel_reset(x);
    let pid = GetCurrentProcessId();
    let mut claimed = 0;
    for mapping in &x.mappings {
        let shared = mapping.shared();
        if !shared.engine_live() || shared.rate.load(Ordering::Acquire) as f64 != x.rate {
            for m in &x.mappings[..claimed] {
                m.shared().owner.store(0, Ordering::Release);
                m.shared().client_heartbeat.store(0, Ordering::Release);
            }
            x.error = "Start Soon Link Pro first and match its sample rate".into();
            return NOT_PRESENT;
        }
        if !shared.client_live() {
            shared.owner.store(0, Ordering::Release);
        }
        if shared
            .owner
            .compare_exchange(0, pid, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            for m in &x.mappings[..claimed] {
                m.shared().owner.store(0, Ordering::Release);
                m.shared().client_heartbeat.store(0, Ordering::Release);
            }
            x.error = "Soon Link Pro ASIO already has a client".into();
            return NOT_PRESENT;
        }
        shared.to_client.discard();
        shared.client_heartbeat.store(clock_ms(), Ordering::Release);
        claimed += 1;
    }
    let resources = (|| -> Result<_, String> {
        Ok((
            (0..x.virtual_buses.max(1))
                .map(Mapping::open)
                .collect::<Result<Vec<_>, _>>()?,
            HardwareMapping::open()?,
        ))
    })();
    let (mappings, hardware) = match resources {
        Ok(v) => v,
        Err(e) => {
            for m in &x.mappings {
                m.shared().owner.store(0, Ordering::Release);
                m.shared().client_heartbeat.store(0, Ordering::Release);
            }
            x.error = e;
            return NOT_PRESENT;
        }
    };
    hardware.shared().to_daw.discard();
    hardware
        .shared()
        .client_buffer
        .store(x.frames as u32, Ordering::Release);
    let start_frame = hardware.shared().engine_frames.load(Ordering::Acquire);
    x.position.store(0, Ordering::Release);
    x.running.store(true, Ordering::Release);
    let running = x.running.clone();
    let position = x.position.clone();
    let pointers: Vec<(bool, usize, [usize; 2])> = x
        .buffers
        .iter_mut()
        .map(|(a, c, b)| {
            (
                *a,
                *c,
                [b[0].as_mut_ptr() as usize, b[1].as_mut_ptr() as usize],
            )
        })
        .collect();
    let cb = x.callbacks.unwrap().switch.unwrap();
    let frames = x.frames;
    let rate = x.rate;
    let ni = x.input_channels;
    let no = x.output_channels;
    let generation = x.generation;
    let bus_count = x.virtual_buses;
    let worker = thread::Builder::new()
        .name("patchbay-asio".into())
        .spawn(move || {
            timeBeginPeriod(1);
            let mut side = 0usize;
            let mut due = start_frame + frames as u64;
            let mut readers: Vec<_> = (0..bus_count)
                .map(|_| AdaptiveReader::new(rate as u32, frames))
                .collect();
            let mut virtuals = vec![[0.0; 2]; bus_count];
            'audio: while running.load(Ordering::Acquire) {
                while hardware.shared().engine_frames.load(Ordering::Acquire) < due {
                    if !running.load(Ordering::Acquire)
                        || hardware.shared().generation.load(Ordering::Acquire) != generation
                        || !mappings[0].shared().engine_live()
                    {
                        break 'audio;
                    }
                    thread::sleep(Duration::from_micros(250));
                }
                if hardware.shared().generation.load(Ordering::Acquire) != generation
                    || !mappings.iter().all(|m| {
                        m.shared().engine_live()
                            && m.shared().rate.load(Ordering::Acquire) as f64 == rate
                            && m.shared().owner.load(Ordering::Acquire) == pid
                    })
                {
                    break;
                }
                for mapping in &mappings {
                    mapping
                        .shared()
                        .client_heartbeat
                        .store(clock_ms(), Ordering::Release);
                }
                for (_, _, p) in &pointers {
                    std::ptr::write_bytes(p[side] as *mut f32, 0, frames);
                }
                let mut raw = [0.0; MAX_HARDWARE_CHANNELS];
                if hardware.shared().to_daw.available() > frames.saturating_mul(4) {
                    while hardware.shared().to_daw.available() > frames.saturating_mul(2) {
                        hardware.shared().to_daw.pop(&mut raw[..ni]);
                    }
                }
                for f in 0..frames {
                    hardware.shared().to_daw.pop(&mut raw[..ni]);
                    for (input, ch, p) in &pointers {
                        if *input && *ch < ni {
                            *(p[side] as *mut f32).add(f) = raw[*ch];
                        }
                    }
                    for bus in 0..bus_count {
                        let sample = readers[bus].next(&mappings[bus].shared().to_client);
                        for (input, ch, p) in &pointers {
                            if *input && *ch >= ni && (*ch - ni) / 2 == bus {
                                *(p[side] as *mut f32).add(f) = sample[(*ch - ni) % 2];
                            }
                        }
                    }
                }
                cb(side as i32, 0);
                for f in 0..frames {
                    raw.fill(0.0);
                    virtuals.fill([0.0; 2]);
                    for (input, ch, p) in &pointers {
                        if !*input {
                            let v = *(p[side] as *const f32).add(f);
                            if *ch < no {
                                raw[*ch] = v;
                            } else {
                                virtuals[(*ch - no) / 2][(*ch - no) % 2] = v;
                            }
                        }
                    }
                    hardware.shared().from_daw.push(&raw[..no]);
                    for bus in 0..bus_count {
                        mappings[bus].shared().to_engine.push(virtuals[bus]);
                    }
                }
                position.fetch_add(frames as u64, Ordering::Release);
                side ^= 1;
                due += frames as u64;
                let current = hardware.shared().engine_frames.load(Ordering::Acquire);
                if current > due + frames as u64 * 4 {
                    due = current + frames as u64;
                }
            }
            running.store(false, Ordering::Release);
            for m in &mappings {
                if m.shared().owner.load(Ordering::Acquire) == pid {
                    m.shared().owner.store(0, Ordering::Release);
                    m.shared().client_heartbeat.store(0, Ordering::Release);
                }
            }
            timeEndPeriod(1);
        });
    match worker {
        Ok(t) => {
            x.worker = Some(t);
            watch_layout(x);
            OK
        }
        Err(e) => {
            x.running.store(false, Ordering::Release);
            for m in &x.mappings {
                m.shared().owner.store(0, Ordering::Release);
                m.shared().client_heartbeat.store(0, Ordering::Release);
            }
            x.error = e.to_string();
            NOT_PRESENT
        }
    }
}
fn cancel_reset(x: &mut Driver) {
    x.reset_cancel.store(true, Ordering::Release);
    if let Some(worker) = x.reset_worker.take() {
        // A host may synchronously stop during its reset callback.
        if worker.thread().id() != thread::current().id() {
            let _ = worker.join();
        }
    }
}
fn watch_layout(x: &mut Driver) {
    let Some(message) = x.callbacks.and_then(|c| c.message) else {
        return;
    };
    let (Ok(hardware), Ok(mapping)) = (HardwareMapping::open(), Mapping::open(0)) else {
        return;
    };
    let cancel = Arc::new(AtomicBool::new(false));
    x.reset_cancel = cancel.clone();
    let generation = x.generation;
    let running = x.running.clone();
    // The notification is separate from the buffer thread: ASIOStop can join that thread safely.
    LOCKS.fetch_add(1, Ordering::Relaxed);
    let result = thread::Builder::new()
        .name("soon-link-asio-reset".into())
        .spawn(move || {
            while !cancel.load(Ordering::Acquire) {
                let next = hardware.shared().generation.load(Ordering::Acquire);
                if next != 0
                    && next != generation
                    && mapping.shared().engine_live()
                    && !running.load(Ordering::Acquire)
                {
                    if !cancel.load(Ordering::Acquire) {
                        // ASIO SDK: kAsioResetRequest = 3. Host defers resetting to a safe time.
                        unsafe {
                            message(3, 0, ptr::null_mut(), ptr::null_mut());
                        }
                    }
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            LOCKS.fetch_sub(1, Ordering::Release);
        });
    match result {
        Ok(worker) => x.reset_worker = Some(worker),
        Err(_) => {
            LOCKS.fetch_sub(1, Ordering::Release);
        }
    }
}
unsafe fn refresh_layout(d: *mut Driver) {
    if !(*d).running.load(Ordering::Acquire)
        && (*d).hardware.shared().generation.load(Ordering::Acquire) != (*d).generation
    {
        stop(d);
        if init(d, ptr::null_mut()) != 0 {
            (*d).buffers.clear();
            (*d).callbacks = None;
        }
    }
}
unsafe extern "system" fn stop(d: *mut Driver) -> i32 {
    let x = &mut *d;
    x.running.store(false, Ordering::Release);
    if let Some(t) = x.worker.take() {
        if t.thread().id() != thread::current().id() {
            let _ = t.join();
        }
    }
    cancel_reset(x);
    OK
}
unsafe extern "system" fn channels(d: *mut Driver, i: *mut i32, o: *mut i32) -> i32 {
    refresh_layout(d);
    if i.is_null() || o.is_null() {
        return INVALID;
    }
    *i = ((*d).input_channels + (*d).virtual_buses * 2) as i32;
    *o = ((*d).output_channels + (*d).virtual_buses * 2) as i32;
    OK
}
unsafe extern "system" fn latencies(d: *mut Driver, i: *mut i32, o: *mut i32) -> i32 {
    if i.is_null() || o.is_null() {
        return INVALID;
    }
    let queue = (*d).frames.max((*d).rate as usize / 125);
    *i = ((*d).frames + queue) as i32;
    *o = ((*d).frames + queue) as i32;
    OK
}
unsafe extern "system" fn sizes(
    _: *mut Driver,
    min: *mut i32,
    max: *mut i32,
    pref: *mut i32,
    gran: *mut i32,
) -> i32 {
    if min.is_null() || max.is_null() || pref.is_null() || gran.is_null() {
        return INVALID;
    }
    *min = MIN_BUFFER as i32;
    *max = MAX_BUFFER as i32;
    *pref = DEFAULT_BUFFER as i32;
    *gran = -1;
    OK
}
unsafe extern "system" fn can_rate(_: *mut Driver, rate: f64) -> i32 {
    if [44100.0, 48000.0, 96000.0].contains(&rate) {
        OK
    } else {
        BAD_MODE
    }
}
unsafe extern "system" fn get_rate(d: *mut Driver, rate: *mut f64) -> i32 {
    refresh_layout(d);
    if rate.is_null() {
        return INVALID;
    }
    *rate = (*d).rate;
    OK
}
unsafe extern "system" fn set_rate(d: *mut Driver, rate: f64) -> i32 {
    if (*d).running.load(Ordering::Acquire) || can_rate(d, rate) != OK {
        return BAD_MODE;
    }
    if (&(*d).mappings)[0].shared().engine_live()
        && (&(*d).mappings)[0].shared().rate.load(Ordering::Acquire) as f64 != rate
    {
        (*d).error = "Set this rate in Soon Link Pro first".into();
        return BAD_MODE;
    }
    (*d).rate = rate;
    OK
}
#[repr(C)]
struct Clock {
    index: i32,
    channel: i32,
    group: i32,
    current: i32,
    name: [u8; 32],
}
unsafe extern "system" fn clocks(_: *mut Driver, c: *mut Clock, n: *mut i32) -> i32 {
    if c.is_null() || n.is_null() || *n < 1 {
        return INVALID;
    }
    *n = 1;
    ptr::write(
        c,
        Clock {
            index: 0,
            channel: -1,
            group: -1,
            current: 1,
            name: [0; 32],
        },
    );
    text((*c).name.as_mut_ptr(), "Soon Link Pro bridge clock", 32);
    OK
}
unsafe extern "system" fn clock(_: *mut Driver, i: i32) -> i32 {
    if i == 0 {
        OK
    } else {
        INVALID
    }
}
unsafe extern "system" fn position(d: *mut Driver, p: *mut Samples, t: *mut Samples) -> i32 {
    if p.is_null() || t.is_null() {
        return INVALID;
    }
    let n = (*d).position.load(Ordering::Acquire);
    *p = Samples {
        hi: (n >> 32) as u32,
        lo: n as u32,
    };
    let ns = clock_ms() * 1_000_000;
    *t = Samples {
        hi: (ns >> 32) as u32,
        lo: ns as u32,
    };
    OK
}
unsafe extern "system" fn info(d: *mut Driver, c: *mut ChannelInfo) -> i32 {
    refresh_layout(d);
    if c.is_null() {
        return INVALID;
    }
    let hw = if (*c).input != 0 {
        (*d).input_channels
    } else {
        (*d).output_channels
    };
    if !(0..(hw + (*d).virtual_buses * 2) as i32).contains(&(*c).channel) {
        return INVALID;
    }
    let ch = (*c).channel as usize;
    (*c).active = (*d)
        .buffers
        .iter()
        .any(|(input, channel, _)| *input == ((*c).input != 0) && *channel == ch)
        as i32;
    (*c).sample_type = 19;
    let label = if ch < hw {
        (*c).group = (ch / 2) as i32;
        format!(
            "Hardware {} {}",
            if (*c).input != 0 { "Input" } else { "Output" },
            ch + 1
        )
    } else {
        let v = ch - hw;
        (*c).group = (hw.div_ceil(2) + v / 2) as i32;
        // Card annotations are private UI data and never change host labels.
        let base = format!("Virtual {}", v / 2 + 1);
        let suffix = if v % 2 == 0 { " L" } else { " R" };
        let mut length = base.len().min(31 - suffix.len());
        while !base.is_char_boundary(length) {
            length -= 1;
        }
        format!("{}{suffix}", &base[..length])
    };
    text((*c).name.as_mut_ptr(), &label, 32);
    OK
}
unsafe extern "system" fn create(
    d: *mut Driver,
    b: *mut BufferInfo,
    count: i32,
    size: i32,
    c: *const Callbacks,
) -> i32 {
    refresh_layout(d);
    if b.is_null()
        || c.is_null()
        || !(1..=((*d).input_channels + (*d).output_channels + (*d).virtual_buses * 4) as i32)
            .contains(&count)
        || size < MIN_BUFFER as i32
        || size > MAX_BUFFER as i32
        || !(size as usize).is_power_of_two()
        || (*d).running.load(Ordering::Acquire)
    {
        return INVALID;
    }
    let x = &mut *d;
    x.buffers.clear();
    let mut seen = Vec::new();
    for i in 0..count as usize {
        let v = &mut *b.add(i);
        if !(0..(if v.input != 0 {
            x.input_channels
        } else {
            x.output_channels
        } + x.virtual_buses * 2) as i32)
            .contains(&v.channel)
            || seen.contains(&(v.input != 0, v.channel))
        {
            x.buffers.clear();
            return INVALID;
        }
        seen.push((v.input != 0, v.channel));
    }
    for i in 0..count as usize {
        let v = &mut *b.add(i);
        let mut pair = [vec![0.0; size as usize], vec![0.0; size as usize]];
        v.buffers = [pair[0].as_mut_ptr().cast(), pair[1].as_mut_ptr().cast()];
        x.buffers.push((v.input != 0, v.channel as usize, pair));
    }
    x.frames = size as usize;
    x.callbacks = Some(*c);
    OK
}
unsafe extern "system" fn dispose(d: *mut Driver) -> i32 {
    stop(d);
    (*d).buffers.clear();
    (*d).callbacks = None;
    OK
}
unsafe extern "system" fn panel(_: *mut Driver) -> i32 {
    OK
}
unsafe extern "system" fn future(_: *mut Driver, _: i32, _: *mut c_void) -> i32 {
    -999
}
unsafe extern "system" fn ready(_: *mut Driver) -> i32 {
    OK
}
// ASIO on x64 uses the standard Windows calling convention for its C++ vtable.
fn driver_vtable() -> *const usize {
    static TABLE: std::sync::OnceLock<[usize; 24]> = std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            [
                qi as *const () as usize,
                add as *const () as usize,
                release as *const () as usize,
                init as *const () as usize,
                name as *const () as usize,
                version as *const () as usize,
                error as *const () as usize,
                start as *const () as usize,
                stop as *const () as usize,
                channels as *const () as usize,
                latencies as *const () as usize,
                sizes as *const () as usize,
                can_rate as *const () as usize,
                get_rate as *const () as usize,
                set_rate as *const () as usize,
                clocks as *const () as usize,
                clock as *const () as usize,
                position as *const () as usize,
                info as *const () as usize,
                create as *const () as usize,
                dispose as *const () as usize,
                panel as *const () as usize,
                future as *const () as usize,
                ready as *const () as usize,
            ]
        })
        .as_ptr()
}
#[repr(C)]
struct Factory {
    vtable: *const usize,
    refs: AtomicU32,
}
unsafe extern "system" fn fqi(f: *mut Factory, i: *const Guid, o: *mut *mut c_void) -> i32 {
    if i.is_null() || o.is_null() {
        return INVALID;
    }
    *o = ptr::null_mut();
    if *i == IUNKNOWN || *i == ICLASSFACTORY {
        *o = f.cast();
        fadd(f);
        0
    } else {
        NO_INTERFACE
    }
}
unsafe extern "system" fn fadd(f: *mut Factory) -> u32 {
    (*f).refs.fetch_add(1, Ordering::Relaxed) + 1
}
unsafe extern "system" fn frel(f: *mut Factory) -> u32 {
    let n = (*f).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n == 0 {
        drop(Box::from_raw(f));
        OBJECTS.fetch_sub(1, Ordering::Relaxed);
    }
    n
}
unsafe extern "system" fn instance(
    f: *mut Factory,
    outer: *mut c_void,
    id: *const Guid,
    out: *mut *mut c_void,
) -> i32 {
    if out.is_null() || id.is_null() {
        return INVALID;
    }
    *out = ptr::null_mut();
    if !outer.is_null() {
        return 0x80040110u32 as i32;
    }
    let _ = f;
    let mappings = match (0..1).map(Mapping::open).collect::<Result<Vec<_>, _>>() {
        Ok(m) => m,
        Err(_) => return 0x80004005u32 as i32,
    };
    let d = Box::into_raw(Box::new(Driver {
        vtable: driver_vtable(),
        refs: AtomicU32::new(1),
        hardware: match HardwareMapping::open() {
            Ok(h) => h,
            Err(_) => return 0x80004005u32 as i32,
        },
        virtual_buses: 0,
        lifecycle: None,
        input_channels: 0,
        output_channels: 0,
        generation: 0,
        mappings,
        rate: DEFAULT_RATE as f64,
        frames: DEFAULT_BUFFER,
        buffers: Vec::new(),
        callbacks: None,
        running: Arc::new(AtomicBool::new(false)),
        position: Arc::new(AtomicU64::new(0)),
        worker: None,
        reset_cancel: Arc::new(AtomicBool::new(false)),
        reset_worker: None,
        error: "No error".into(),
    }));
    OBJECTS.fetch_add(1, Ordering::Relaxed);
    let result = qi(d, id, out);
    release(d);
    result
}
unsafe extern "system" fn lock(_: *mut Factory, on: i32) -> i32 {
    if on != 0 {
        LOCKS.fetch_add(1, Ordering::Relaxed);
    } else {
        let _ = LOCKS.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1));
    }
    0
}
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    clsid: *const Guid,
    id: *const Guid,
    out: *mut *mut c_void,
) -> i32 {
    if clsid.is_null() || id.is_null() || out.is_null() {
        return INVALID;
    }
    *out = ptr::null_mut();
    if *clsid != UNIFIED_GUID {
        return 0x80040111u32 as i32;
    }
    static TABLE: std::sync::OnceLock<[usize; 5]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        [
            fqi as *const () as usize,
            fadd as *const () as usize,
            frel as *const () as usize,
            instance as *const () as usize,
            lock as *const () as usize,
        ]
    });
    let f = Box::into_raw(Box::new(Factory {
        vtable: table.as_ptr(),
        refs: AtomicU32::new(1),
    }));
    OBJECTS.fetch_add(1, Ordering::Relaxed);
    let result = fqi(f, id, out);
    frel(f);
    result
}
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> i32 {
    if OBJECTS.load(Ordering::Acquire) == 0 && LOCKS.load(Ordering::Acquire) == 0 {
        0
    } else {
        1
    }
}
fn registration(remove: bool) -> Result<(), Box<dyn std::error::Error>> {
    use winreg::{enums::*, RegKey};
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let mut module = ptr::null_mut();
    let mut path = [0u16; 32768];
    unsafe {
        if GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            DllRegisterServer as *const u16,
            &mut module,
        ) == 0
        {
            return Err("Cannot locate driver module".into());
        }
        let n = GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32);
        if n == 0 {
            return Err("Cannot locate driver path".into());
        }
    }
    let path =
        String::from_utf16_lossy(&path[..path.iter().position(|c| *c == 0).unwrap_or(path.len())]);
    let asio = "SOFTWARE\\ASIO\\Soon Link Pro ASIO";
    let cls = format!("SOFTWARE\\Classes\\CLSID\\{}", UNIFIED_CLSID);
    let old = "SOFTWARE\\ASIO\\Patchbay ASIO";
    let old_owned = machine
        .open_subkey(old)
        .ok()
        .and_then(|k| k.get_value::<String, _>("CLSID").ok())
        .is_some_and(|id| id.eq_ignore_ascii_case(UNIFIED_CLSID));
    if old_owned {
        machine.delete_subkey_all(old)?;
    }
    if remove {
        for key in [asio, &cls] {
            match machine.delete_subkey_all(key) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            }
        }
    } else {
        let (k, _) = machine.create_subkey(asio)?;
        k.set_value("CLSID", &UNIFIED_CLSID)?;
        k.set_value(
            "Description",
            &"Soon Link Pro ASIO (Hardware + Dynamic Virtual Buses)",
        )?;
        let (k, _) = machine.create_subkey(format!("{}\\InprocServer32", cls))?;
        k.set_value("", &path)?;
        k.set_value("ThreadingModel", &"Both")?;
        // Migrate only old Patchbay keys whose CLSIDs exactly match ours.
        for i in 0..LEGACY_BUSES {
            let key = format!("SOFTWARE\\ASIO\\Patchbay ASIO {}", i + 1);
            let owned = machine
                .open_subkey(&key)
                .ok()
                .and_then(|k| k.get_value::<String, _>("CLSID").ok())
                .is_some_and(|v| v.eq_ignore_ascii_case(CLSIDS[i]));
            if owned {
                machine.delete_subkey_all(&key)?;
                let legacy = format!("SOFTWARE\\Classes\\CLSID\\{}", CLSIDS[i]);
                match machine.delete_subkey_all(legacy) {
                    Ok(()) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.into()),
                }
            }
        }
    }
    Ok(())
}
#[no_mangle]
pub extern "system" fn DllRegisterServer() -> i32 {
    match registration(false) {
        Ok(()) => 0,
        Err(_) => 0x80070005u32 as i32,
    }
}
#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> i32 {
    match registration(true) {
        Ok(()) => 0,
        Err(_) => 0x80070005u32 as i32,
    }
}

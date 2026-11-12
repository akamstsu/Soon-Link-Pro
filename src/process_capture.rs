#![allow(non_snake_case)]
use crate::audio::Processor;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use windows::{
    core::{implement, IUnknown, Interface, HRESULT, PCWSTR, PROPVARIANT},
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        Media::Audio::Endpoints::IAudioMeterInformation,
        Media::Audio::*,
        Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW},
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
                COINIT_MULTITHREADED,
            },
            Diagnostics::ToolHelp::*,
            Threading::{
                AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW,
                OpenProcess, QueryFullProcessImageNameW, SetEvent, WaitForSingleObject, INFINITE,
                PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    },
};

pub const MAX_APPLICATIONS: usize = bridge::MAX_APPLICATIONS;
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(4);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    pub pid: u32,
    pub exe: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(skip)]
    pub speaking: bool,
    #[serde(skip)]
    pub audio_active: bool,
}
fn processes() -> Vec<Target> {
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return Vec::new();
        };
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut out = Vec::new();
        let mut next = Process32FirstW(snapshot, &mut entry).is_ok();
        while next {
            if entry.th32ProcessID != 0 && entry.th32ProcessID != std::process::id() {
                if let Ok(handle) = OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION,
                    false,
                    entry.th32ProcessID,
                ) {
                    let mut buffer = [0u16; 32768];
                    let mut len = buffer.len() as u32;
                    if QueryFullProcessImageNameW(
                        handle,
                        PROCESS_NAME_WIN32,
                        windows::core::PWSTR(buffer.as_mut_ptr()),
                        &mut len,
                    )
                    .is_ok()
                    {
                        let exe = String::from_utf16_lossy(&buffer[..len as usize]);
                        out.push(Target {
                            pid: entry.th32ProcessID,
                            exe,
                            display_name: String::new(),
                            speaking: false,
                            audio_active: false,
                        });
                    }
                    let _ = CloseHandle(handle);
                }
            }
            next = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
        out
    }
}
impl Target {
    pub fn label(&self) -> String {
        if !self.display_name.trim().is_empty() {
            return self.display_name.clone();
        }
        std::path::Path::new(&self.exe)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("进程 {}", self.pid))
    }
    fn resolve(&self) -> Result<u32, String> {
        if self.pid == std::process::id() {
            return Ok(self.pid);
        }
        // Resolving a saved target must not enumerate sessions, read version resources,
        // or sample meters on the engine owner thread.
        let apps = processes();
        apps.iter()
            .find(|t| t.pid == self.pid && t.exe.eq_ignore_ascii_case(&self.exe))
            .or_else(|| apps.iter().find(|t| t.exe.eq_ignore_ascii_case(&self.exe)))
            .map(|t| t.pid)
            .ok_or_else(|| format!("应用未运行：{}", self.label()))
    }
}

pub struct Inventory {
    pub targets: Vec<Target>,
    pub audio_status: String,
}

/// Called only on the picker worker; names are cached, activity is read afresh.
pub fn applications() -> Inventory {
    static NAMES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let cache = NAMES.get_or_init(Default::default);
    let (activity, audio_status) = unsafe {
        match CoInitializeEx(None, COINIT_MULTITHREADED).ok() {
            Ok(()) => {
                let result = session_activity();
                CoUninitialize();
                match result {
                    Ok(activity) => (
                        activity,
                        "正在发声的应用优先，其次为有活动音频会话的应用；约每 2 秒刷新。".into(),
                    ),
                    Err(error) => (
                        HashMap::new(),
                        format!("音频活动暂不可用，仍可添加应用：{error}"),
                    ),
                }
            }
            Err(error) => (HashMap::new(), format!("音频活动暂不可用：{error}")),
        }
    };
    let mut grouped = HashMap::<String, Target>::new();
    for mut target in processes() {
        let key = target.exe.to_lowercase();
        let cached = cache.lock().ok().and_then(|names| names.get(&key).cloned());
        target.display_name = cached.unwrap_or_else(|| {
            let name = unsafe { executable_name(&target.exe) }.unwrap_or_default();
            if let Ok(mut names) = cache.lock() {
                names.insert(key.clone(), name.clone());
            }
            name
        });
        if let Some((speaking, active, session_name)) = activity.get(&target.pid) {
            target.speaking = *speaking;
            target.audio_active = *active;
            if target.display_name.is_empty() {
                target.display_name = session_name.clone();
            }
        }
        match grouped.get_mut(&key) {
            Some(existing)
                if (target.speaking, target.audio_active)
                    > (existing.speaking, existing.audio_active) =>
            {
                *existing = target
            }
            Some(_) => {}
            None => {
                grouped.insert(key, target);
            }
        }
    }
    let mut targets: Vec<_> = grouped.into_values().collect();
    targets.sort_by_cached_key(|target| {
        (
            !target.speaking,
            !target.audio_active,
            target.label().to_lowercase(),
        )
    });
    Inventory {
        targets,
        audio_status,
    }
}

unsafe fn executable_name(exe: &str) -> Option<String> {
    let wide: Vec<_> = exe.encode_utf16().chain(Some(0)).collect();
    let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
    if size == 0 || size > 4 * 1024 * 1024 {
        return None;
    }
    let mut bytes = vec![0u8; size as usize];
    GetFileVersionInfoW(PCWSTR(wide.as_ptr()), 0, size, bytes.as_mut_ptr().cast()).ok()?;
    let query = |path: &str| -> Option<(*mut c_void, usize)> {
        let path: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
        let mut pointer = std::ptr::null_mut();
        let mut length = 0;
        VerQueryValueW(
            bytes.as_ptr().cast(),
            PCWSTR(path.as_ptr()),
            &mut pointer,
            &mut length,
        )
        .as_bool()
        .then_some((pointer, length as usize))
        .filter(|(pointer, length)| !pointer.is_null() && *length > 0)
    };
    let mut translations = Vec::new();
    if let Some((pointer, length)) = query(r"\VarFileInfo\Translation") {
        for pair in std::slice::from_raw_parts(pointer.cast::<u8>(), length).chunks_exact(4) {
            translations.push((
                u16::from_le_bytes([pair[0], pair[1]]),
                u16::from_le_bytes([pair[2], pair[3]]),
            ));
        }
    }
    translations.sort_by_key(|(language, _)| {
        if *language == 0x0804 {
            0
        } else if language & 0x3ff == 4 {
            1
        } else if *language == 0x0409 {
            2
        } else {
            3
        }
    });
    for fallback in [(0x0804, 0x04b0), (0x0409, 0x04b0), (0x0409, 0x04e4)] {
        if !translations.contains(&fallback) {
            translations.push(fallback);
        }
    }
    for (language, codepage) in translations {
        for field in ["FileDescription", "ProductName"] {
            if let Some((pointer, length)) = query(&format!(
                r"\StringFileInfo\{language:04x}{codepage:04x}\{field}"
            )) {
                let name = String::from_utf16_lossy(std::slice::from_raw_parts(
                    pointer.cast::<u16>(),
                    length,
                ));
                let name: String = name
                    .trim_end_matches('\0')
                    .trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(100)
                    .collect();
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
    }
    None
}

unsafe fn session_activity() -> windows::core::Result<HashMap<u32, (bool, bool, String)>> {
    let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
    let mut meters = Vec::new();
    let mut activity = HashMap::<u32, (bool, bool, String)>::new();
    for i in 0..devices.GetCount()? {
        let Ok(device) = devices.Item(i) else {
            continue;
        };
        let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
            continue;
        };
        let Ok(sessions) = manager.GetSessionEnumerator() else {
            continue;
        };
        for i in 0..sessions.GetCount()? {
            let Ok(session) = sessions
                .GetSession(i)
                .and_then(|session| session.cast::<IAudioSessionControl2>())
            else {
                continue;
            };
            let Ok(pid) = session.GetProcessId() else {
                continue;
            };
            if pid == 0 {
                continue;
            }
            let state = session
                .GetState()
                .is_ok_and(|state| state == AudioSessionStateActive);
            let entry = activity.entry(pid).or_default();
            entry.1 |= state;
            if entry.2.is_empty() {
                if let Ok(pointer) = session.GetDisplayName() {
                    let name = pointer.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(pointer.0.cast()));
                    if !name.starts_with('@') {
                        entry.2 = name.chars().filter(|c| !c.is_control()).take(100).collect();
                    }
                }
            }
            if state {
                let volume = session.cast::<ISimpleAudioVolume>().ok();
                let muted = volume
                    .as_ref()
                    .is_some_and(|volume| volume.GetMute().is_ok_and(|mute| mute.as_bool()));
                let gain = volume
                    .as_ref()
                    .and_then(|volume| volume.GetMasterVolume().ok())
                    .unwrap_or(1.0);
                if !muted {
                    if let Ok(meter) = session.cast::<IAudioMeterInformation>() {
                        meters.push((pid, meter, gain));
                    }
                }
            }
        }
    }
    // A short peak window avoids treating a single zero crossing as silence.
    // This is picker-only background work, never on a render or capture callback.
    for sample in 0..3 {
        for (pid, meter, gain) in &meters {
            if meter
                .GetPeakValue()
                .is_ok_and(|peak| peak.is_finite() && peak * gain > 0.0001)
            {
                activity.entry(*pid).or_default().0 = true;
            }
        }
        if sample < 2 {
            thread::sleep(Duration::from_millis(40));
        }
    }
    Ok(activity)
}
use windows_sys::Win32::System::{
    Com::{
        StructuredStorage::{
            PROPVARIANT as RawPropVariant, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
        },
        BLOB,
    },
    Variant::VT_BLOB,
};

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Completion {
    result: mpsc::SyncSender<Result<IAudioClient, String>>,
    _storage: Arc<ActivationStorage>,
}
impl IActivateAudioInterfaceCompletionHandler_Impl for Completion {
    fn ActivateCompleted(
        &self,
        operation: Option<&IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let result = unsafe {
            (|| -> Result<IAudioClient, String> {
                let operation = operation.ok_or("Windows 未返回激活操作")?;
                let mut status = HRESULT(0);
                let mut interface: Option<IUnknown> = None;
                operation
                    .GetActivateResult(&mut status, &mut interface)
                    .map_err(|error| error.to_string())?;
                status.ok().map_err(|error| error.to_string())?;
                interface
                    .ok_or("Windows 未返回音频接口")?
                    .cast()
                    .map_err(|error| error.to_string())
            })()
        };
        let _ = self.result.try_send(result);
        Ok(())
    }
}

// Use the SDK layout. The BLOB borrows the boxed parameter block, so the raw
// variant must not be passed to PropVariantClear. The callback retains both
// allocations until Windows completes the asynchronous request.
struct ActivationStorage {
    _parameters: Box<AUDIOCLIENT_ACTIVATION_PARAMS>,
    variant: RawPropVariant,
}
unsafe impl Send for ActivationStorage {}
unsafe impl Sync for ActivationStorage {}

unsafe fn activate_process(pid: u32) -> Result<IAudioClient, String> {
    let mut parameters = Box::new(AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    });
    let blob = BLOB {
        cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
        pBlobData: (&mut *parameters as *mut AUDIOCLIENT_ACTIVATION_PARAMS).cast(),
    };
    let storage = Arc::new(ActivationStorage {
        _parameters: parameters,
        variant: RawPropVariant {
            Anonymous: PROPVARIANT_0 {
                Anonymous: PROPVARIANT_0_0 {
                    vt: VT_BLOB,
                    Anonymous: PROPVARIANT_0_0_0 { blob },
                    ..Default::default()
                },
            },
        },
    });
    let (sender, receiver) = mpsc::sync_channel(1);
    let callback: IActivateAudioInterfaceCompletionHandler = Completion {
        result: sender,
        _storage: storage.clone(),
    }
    .into();
    // Both Rust binding versions use the SDK PROPVARIANT ABI.
    let variant: &PROPVARIANT = &*(&storage.variant as *const RawPropVariant).cast();
    let _operation = ActivateAudioInterfaceAsync(
        VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
        &IAudioClient::IID,
        Some(variant),
        &callback,
    )
    .map_err(|error| format!("Windows 应用采集激活失败：{error}"))?;
    receiver
        .recv_timeout(ACTIVATION_TIMEOUT)
        .map_err(|_| "Windows 应用采集激活超时".to_string())?
}

struct CaptureEvent(HANDLE);
// The event can be signaled across threads; its last Arc owns and closes the handle.
unsafe impl Send for CaptureEvent {}
unsafe impl Sync for CaptureEvent {}
impl Drop for CaptureEvent {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
struct AudioPriority(Option<HANDLE>);
impl Drop for AudioPriority {
    fn drop(&mut self) {
        if let Some(handle) = self.0 {
            unsafe {
                let _ = AvRevertMmThreadCharacteristics(handle);
            }
        }
    }
}
pub struct ProcessCapture {
    stop: Arc<AtomicBool>,
    event: Arc<CaptureEvent>,
    thread: Option<JoinHandle<()>>,
}
impl Drop for ProcessCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        unsafe {
            let _ = SetEvent(self.event.0);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
impl ProcessCapture {
    pub fn start(target: &Target, processor: Arc<Processor>, slot: usize) -> Result<Self, String> {
        Self::start_format(target, processor, slot, false)
    }
    fn start_format(
        target: &Target,
        processor: Arc<Processor>,
        slot: usize,
        force_pcm: bool,
    ) -> Result<Self, String> {
        if slot >= processor.applications.len() {
            return Err("无效的应用采集通道".into());
        }
        let event = Arc::new(CaptureEvent(
            unsafe { CreateEventW(None, false, false, None) }.map_err(|e| e.to_string())?,
        ));
        let worker_event = event.clone();
        let pid = target.resolve()?;
        let label = target.label();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || unsafe {
            if let Err(e) = CoInitializeEx(None, COINIT_MULTITHREADED).ok() {
                let _ = tx.send(Err(e.to_string()));
                return;
            }
            let result = worker(pid, &processor, slot, &flag, &worker_event, force_pcm, &tx);
            if let Err(e) = result {
                let message = format!("应用采集 {label}：{e}");
                let _ = tx.try_send(Err(message.clone()));
                if let Ok(mut errors) = processor.errors.lock() {
                    if errors.len() < 8 {
                        errors.push(message);
                    }
                }
            }
            CoUninitialize();
        });
        let capture = Self {
            stop,
            event,
            thread: Some(thread),
        };
        match rx.recv_timeout(STARTUP_TIMEOUT) {
            Ok(Ok(())) => Ok(capture),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                capture.stop.store(true, Ordering::Release);
                Err("应用音频初始化超时，请确认 Windows 支持应用声音采集".into())
            }
        }
    }
}
fn pcm_format(rate: u32) -> WAVEFORMATEX {
    // Packed stereo signed 32-bit PCM at the engine clock, per WAVEFORMATEX.
    let channels = bridge::CHANNELS as u16;
    let bits = i32::BITS as u16;
    let alignment = channels * (bits / 8);
    WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: channels,
        nSamplesPerSec: rate,
        nAvgBytesPerSec: rate * u32::from(alignment),
        nBlockAlign: alignment,
        wBitsPerSample: bits,
        cbSize: 0,
    }
}
#[derive(Debug, PartialEq, Eq)]
enum PacketKind {
    Empty,
    Silent,
    Data,
}
fn packet_kind(data: *const u8, frames: u32, flags: u32) -> Result<PacketKind, &'static str> {
    if frames == 0 {
        return Ok(PacketKind::Empty);
    }
    frames
        .checked_mul((std::mem::size_of::<i32>() * bridge::CHANNELS) as u32)
        .ok_or("应用音频包的长度无效")?;
    // WASAPI permits a null pointer for SILENT packets; all other data needs a buffer.
    if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
        return Ok(PacketKind::Silent);
    }
    if data.is_null() {
        return Err("应用音频返回无效缓冲，请重新启动引擎");
    }
    Ok(PacketKind::Data)
}
struct CaptureBuffer<'a> {
    client: &'a IAudioCaptureClient,
    frames: u32,
    released: bool,
}
impl CaptureBuffer<'_> {
    fn release(mut self) -> Result<(), String> {
        self.released = true;
        unsafe { self.client.ReleaseBuffer(self.frames) }
            .map_err(|e| format!("归还应用音频缓冲失败：{e}"))
    }
}
impl Drop for CaptureBuffer<'_> {
    fn drop(&mut self) {
        if !self.released {
            unsafe {
                let _ = self.client.ReleaseBuffer(self.frames);
            }
        }
    }
}
unsafe fn packet_sample(data: *const u8, frame: usize, pcm: bool) -> [f32; 2] {
    if pcm {
        let p = data.cast::<i32>().add(frame * 2);
        let scale = -(i32::MIN as f64) as f32;
        [
            std::ptr::read_unaligned(p) as f32 / scale,
            std::ptr::read_unaligned(p.add(1)) as f32 / scale,
        ]
    } else {
        let p = data.cast::<f32>().add(frame * 2);
        [
            std::ptr::read_unaligned(p),
            std::ptr::read_unaligned(p.add(1)),
        ]
    }
}
/// Owns a started client until every exit path has stopped it.
struct RunningClient(IAudioClient);
impl Drop for RunningClient {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.Stop();
        }
    }
}

unsafe fn initialize_process(pid: u32, rate: u32, pcm: bool) -> Result<IAudioClient, String> {
    let client = activate_process(pid)?;
    let mut format = pcm_format(rate);
    if !pcm {
        format.wFormatTag = windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT as u16;
    }
    let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
        | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
        | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM;
    client
        .Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 0, 0, &format, None)
        .map_err(|error| format!("初始化应用音频格式失败：{error}"))?;
    Ok(client)
}

unsafe fn worker(
    pid: u32,
    processor: &Processor,
    slot: usize,
    stop: &AtomicBool,
    event: &CaptureEvent,
    force_pcm: bool,
    ready: &mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    // Format negotiation uses a fresh activation for each attempt. A client
    // whose Initialize failed is never reused for a different representation.
    let (client, pcm) = if force_pcm {
        (initialize_process(pid, processor.rate, true)?, true)
    } else {
        match initialize_process(pid, processor.rate, false) {
            Ok(client) => (client, false),
            Err(float_error) => {
                if stop.load(Ordering::Acquire) {
                    return Ok(());
                }
                let client = initialize_process(pid, processor.rate, true)
                    .map_err(|pcm_error| format!("{float_error}；PCM 回退失败：{pcm_error}"))?;
                (client, true)
            }
        }
    };
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    client
        .SetEventHandle(event.0)
        .map_err(|error| error.to_string())?;
    let packets: IAudioCaptureClient = client.GetService().map_err(|error| error.to_string())?;
    let task: Vec<u16> = "Pro Audio".encode_utf16().chain(Some(0)).collect();
    let mut task_index = 0;
    let _priority =
        AudioPriority(AvSetMmThreadCharacteristicsW(PCWSTR(task.as_ptr()), &mut task_index).ok());
    client.Start().map_err(|error| error.to_string())?;
    let _running = RunningClient(client);
    if ready.send(Ok(())).is_err() {
        return Ok(());
    }
    while !stop.load(Ordering::Acquire) {
        let wait = WaitForSingleObject(event.0, INFINITE);
        if wait != WAIT_OBJECT_0 {
            return Err(format!(
                "等待应用音频事件失败：{}",
                std::io::Error::last_os_error()
            ));
        }
        while !stop.load(Ordering::Acquire) {
            if packets
                .GetNextPacketSize()
                .map_err(|error| error.to_string())?
                == 0
            {
                break;
            }
            let mut data = std::ptr::null_mut();
            let mut frames = 0;
            let mut flags = 0;
            packets
                .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                .map_err(|error| error.to_string())?;
            // AUDCLNT_S_BUFFER_EMPTY has no buffer lease to release.
            if frames == 0 {
                break;
            }
            let lease = CaptureBuffer {
                client: &packets,
                frames,
                released: false,
            };
            let silent =
                packet_kind(data, frames, flags).map_err(str::to_string)? == PacketKind::Silent;
            processor.applications[slot].push_frames((0..frames as usize).map(|frame| {
                if silent {
                    [0.0; 2]
                } else {
                    packet_sample(data, frame, pcm)
                }
            }));
            lease.release()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    #[test]
    fn packet_admission_handles_empty_silent_and_invalid_data() {
        let null = std::ptr::null();
        let present = std::ptr::NonNull::<u8>::dangling().as_ptr();
        assert_eq!(packet_kind(null, 0, 0), Ok(PacketKind::Empty));
        assert_eq!(
            packet_kind(null, 64, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32),
            Ok(PacketKind::Silent)
        );
        assert!(packet_kind(null, 64, 0).is_err());
        assert_eq!(packet_kind(present, 1, 0), Ok(PacketKind::Data));
        assert!(packet_kind(present, u32::MAX, 0).is_err());
    }
    struct ReleaseState {
        calls: AtomicU32,
        frames: AtomicU32,
    }
    #[implement(IAudioCaptureClient)]
    struct CountingCapture {
        state: Arc<ReleaseState>,
        fail: bool,
    }
    impl IAudioCaptureClient_Impl for CountingCapture {
        fn GetBuffer(
            &self,
            _: *mut *mut u8,
            _: *mut u32,
            _: *mut u32,
            _: *mut u64,
            _: *mut u64,
        ) -> windows::core::Result<()> {
            HRESULT(0x80004001u32 as i32).ok()
        }
        fn ReleaseBuffer(&self, frames: u32) -> windows::core::Result<()> {
            self.state.calls.fetch_add(1, Ordering::Relaxed);
            self.state.frames.store(frames, Ordering::Relaxed);
            if self.fail {
                HRESULT(0x80004005u32 as i32).ok()
            } else {
                Ok(())
            }
        }
        fn GetNextPacketSize(&self) -> windows::core::Result<u32> {
            Ok(0)
        }
    }
    fn counting_client(fail: bool) -> (IAudioCaptureClient, Arc<ReleaseState>) {
        let state = Arc::new(ReleaseState {
            calls: AtomicU32::new(0),
            frames: AtomicU32::new(0),
        });
        let client = CountingCapture {
            state: state.clone(),
            fail,
        }
        .into();
        (client, state)
    }
    #[test]
    fn capture_buffers_release_after_delivery_and_validation_failure() {
        let (client, state) = counting_client(false);
        CaptureBuffer {
            client: &client,
            frames: 64,
            released: false,
        }
        .release()
        .unwrap();
        assert_eq!(state.calls.load(Ordering::Relaxed), 1);
        assert_eq!(state.frames.load(Ordering::Relaxed), 64);
        let result = {
            let _lease = CaptureBuffer {
                client: &client,
                frames: 96,
                released: false,
            };
            packet_kind(std::ptr::null(), 96, 0)
        };
        assert!(result.is_err());
        assert_eq!(state.calls.load(Ordering::Relaxed), 2);
        assert_eq!(state.frames.load(Ordering::Relaxed), 96);
    }
    #[test]
    fn failed_release_is_not_retried_by_drop() {
        let (client, state) = counting_client(true);
        assert!(CaptureBuffer {
            client: &client,
            frames: 64,
            released: false
        }
        .release()
        .is_err());
        assert_eq!(state.calls.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn pcm_and_float_packet_conversion() {
        let pcm = [i32::MIN, i32::MAX, 0, 1073741824];
        unsafe {
            assert_eq!(packet_sample(pcm.as_ptr().cast(), 0, true), [-1.0, 1.0]);
            assert_eq!(packet_sample(pcm.as_ptr().cast(), 1, true), [0.0, 0.5]);
        }
        let float = [-0.125_f32, 0.25];
        unsafe {
            assert_eq!(
                packet_sample(float.as_ptr().cast(), 0, false),
                [-0.125, 0.25]
            );
        }
    }
}

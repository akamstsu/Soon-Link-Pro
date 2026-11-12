//! Non-realtime session coordination; independent of the audio IPC layout.
use super::*;
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_ALREADY_EXISTS, FILETIME, WAIT_OBJECT_0},
    System::Threading::{
        CreateMutexW, GetCurrentProcessId, GetProcessTimes, OpenProcess, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, SYNCHRONIZATION_SYNCHRONIZE,
    },
};
const HOSTS: usize = 64;
#[repr(C)]
struct Host {
    pid: AtomicU32,
    created: AtomicU64,
}
#[repr(C)]
struct Session {
    mixer: AtomicU32,
    show: AtomicU32,
    keep: AtomicU32,
    hosts: [Host; HOSTS],
}
pub struct Lifecycle {
    handle: HANDLE,
    ptr: *mut Session,
}
unsafe impl Send for Lifecycle {}
unsafe impl Sync for Lifecycle {}
impl Lifecycle {
    pub fn open() -> Result<Self, String> {
        unsafe {
            let name: Vec<u16> = "Local\\SoonLinkProLifecycle_v1\0".encode_utf16().collect();
            let handle = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                0,
                std::mem::size_of::<Session>() as u32,
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
                std::mem::size_of::<Session>(),
            );
            if view.Value.is_null() {
                CloseHandle(handle);
                return Err("Cannot map lifecycle session".into());
            }
            Ok(Self {
                handle,
                ptr: view.Value.cast(),
            })
        }
    }
    fn session(&self) -> &Session {
        unsafe { &*self.ptr }
    }
    pub fn register_host(&self) -> Result<(), String> {
        let pid = unsafe { GetCurrentProcessId() };
        let created = process_identity(pid)?.ok_or("ASIO host has exited")?;
        // A process stays registered until it exits, including ASIO reset/reopen.
        for slot in &self.session().hosts {
            if slot.pid.load(Ordering::Acquire) == pid
                && slot.created.load(Ordering::Acquire) == created
            {
                return Ok(());
            }
        }
        self.live_hosts();
        for slot in &self.session().hosts {
            if slot
                .pid
                .compare_exchange(0, u32::MAX, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                slot.created.store(created, Ordering::Release);
                slot.pid.store(pid, Ordering::Release);
                return Ok(());
            }
        }
        Err("Too many registered ASIO host processes".into())
    }
    pub fn live_hosts(&self) -> usize {
        let mut count = 0;
        for slot in &self.session().hosts {
            let pid = slot.pid.load(Ordering::Acquire);
            if pid == 0 || pid == u32::MAX {
                continue;
            }
            let stamp = slot.created.load(Ordering::Acquire);
            match process_identity(pid) {
                Ok(Some(value)) if value == stamp => count += 1,
                Err(_) => count += 1, // Permission refusal is not proof of process exit.
                _ => {
                    let _ = slot
                        .pid
                        .compare_exchange(pid, 0, Ordering::AcqRel, Ordering::Acquire);
                }
            }
        }
        count
    }
    pub fn mixer_running(&self) -> bool {
        let pid = self.session().mixer.load(Ordering::Acquire);
        pid != 0 && process_identity(pid).is_ok_and(|v| v.is_some())
    }
    pub fn set_mixer(&self) {
        self.session()
            .mixer
            .store(unsafe { GetCurrentProcessId() }, Ordering::Release);
    }
    pub fn clear_mixer(&self) {
        let _ = self.session().mixer.compare_exchange(
            unsafe { GetCurrentProcessId() },
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
    pub fn request_show(&self) {
        self.session().show.store(1, Ordering::Release);
    }
    pub fn take_show(&self) -> bool {
        self.session().show.swap(0, Ordering::AcqRel) != 0
    }
    pub fn keep_running(&self) {
        self.session().keep.store(1, Ordering::Release);
    }
    pub fn manually_owned(&self) -> bool {
        self.session().keep.load(Ordering::Acquire) != 0
    }
    pub fn reset_ownership(&self, manual: bool) {
        self.session().keep.store(manual as u32, Ordering::Release);
        self.session().show.store(0, Ordering::Release);
    }
}
impl Drop for Lifecycle {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr.cast(),
            });
            CloseHandle(self.handle);
        }
    }
}
pub struct InstanceGuard(HANDLE);
impl InstanceGuard {
    pub fn claim() -> Result<Option<Self>, String> {
        unsafe {
            let name: Vec<u16> = "Local\\SoonLinkProMixerInstance_v1\0"
                .encode_utf16()
                .collect();
            let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(handle);
                Ok(None)
            } else {
                Ok(Some(Self(handle)))
            }
        }
    }
}
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn process_identity(pid: u32) -> Result<Option<u64>, String> {
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZATION_SYNCHRONIZE,
            0,
            pid,
        );
        if handle.is_null() {
            let e = std::io::Error::last_os_error();
            return if matches!(e.raw_os_error(), Some(87) | Some(1168)) {
                Ok(None)
            } else {
                Err(e.to_string())
            };
        }
        if WaitForSingleObject(handle, 0) == WAIT_OBJECT_0 {
            CloseHandle(handle);
            return Ok(None);
        }
        let mut created = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok = GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user);
        let error = std::io::Error::last_os_error();
        CloseHandle(handle);
        if ok == 0 {
            return Err(error.to_string());
        }
        Ok(Some(
            ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64,
        ))
    }
}

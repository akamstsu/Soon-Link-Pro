//! Dynamic Windows stereo playback/capture pairs, served locally through usbip-win2.
use crate::audio::Processor;
use bridge::clock_ms;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use usbip_audio::{
    uac2::{Uac2Config, Uac2Device, Uac2Streams},
    usbip_exe, MonotonicClock, Server,
};

pub fn log(message: &str) {
    use std::io::Write;
    let path = std::env::var_os("LOCALAPPDATA")
        .map(|root| {
            std::path::PathBuf::from(root)
                .join("SoonLinkPro")
                .join("soon-link-pro-virtual-devices.log")
        })
        .or_else(|| {
            std::env::current_exe().ok().and_then(|p| {
                p.parent()
                    .map(|p| p.join("soon-link-pro-virtual-devices.log"))
            })
        });
    let Some(path) = path else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(file, "{timestamp} {message}");
    }
}

pub fn name(bus: usize) -> String {
    format!("Soon Link Pro Virtual {}", bus + 1)
}
pub fn endpoint_name(base: &str, bus: usize, playback: bool) -> String {
    let role = if playback { "播放" } else { "录音" };
    let identity = name(bus);
    if base == identity {
        format!("{base} · {role}")
    } else {
        format!("{base} · {role} ({identity})")
    }
}
pub fn is_virtual(name: &str) -> bool {
    name.contains("Soon Link Pro Virtual ")
        || name.contains("Soon Link Pro Playback ")
        || name.contains("Soon Link Pro Recording ")
        || name.contains("Patchbay Virtual ")
        || name.contains("Patchbay Playback ")
        || name.contains("Patchbay Recording ")
}
pub fn has_endpoint(devices: &[String], bus: usize) -> bool {
    let product = name(bus);
    let matches = |s: &str, expected: &str| {
        s.match_indices(expected).any(|(at, _)| {
            !s[at + expected.len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        })
    };
    devices.iter().any(|s| {
        [
            product.clone(),
            format!("Soon Link Pro Playback {}", bus + 1),
            format!("Soon Link Pro Recording {}", bus + 1),
            format!("Patchbay Virtual {}", bus + 1),
            format!("Patchbay Playback {}", bus + 1),
            format!("Patchbay Recording {}", bus + 1),
        ]
        .iter()
        .any(|expected| matches(s, expected))
    })
}
pub fn driver_status() -> String {
    if usbip_exe::find().is_some_and(|p| p.is_file()) {
        "已找到 USB/IP 客户端；启动引擎后连接已添加的设备".into()
    } else {
        "未找到 usbip-win2：请先安装包内 drivers 目录中的安装程序".into()
    }
}

pub use bridge::AdaptiveReader as Reader;

struct Streams {
    processor: Arc<Processor>,
    bus: usize,
    reader: Mutex<Reader>,
}
impl Uac2Streams for Streams {
    fn playback(&self, _: u64, samples: &[i32]) {
        let p = &self.processor;
        let b = self.bus;
        p.windows_capture[b].push_frames(
            samples
                .chunks_exact(2)
                .map(|s| [s[0] as f32 / 2147483648.0, s[1] as f32 / 2147483648.0]),
        );
        p.windows_capture_frames[b].fetch_add((samples.len() / 2) as u64, Ordering::Relaxed);
    }
    fn capture(&self, _: u64, samples: &mut [i32]) {
        let p = &self.processor;
        let b = self.bus;
        let now = clock_ms();
        let last = p.windows_render_heartbeat[b].swap(now, Ordering::Relaxed);
        let Ok(mut reader) = self.reader.lock() else {
            samples.fill(0);
            return;
        };
        if now.saturating_sub(last) > 200 {
            p.windows_render[b].discard();
            reader.reset();
        }
        p.windows_render_enabled[b].store(true, Ordering::Release);
        for frame in samples.chunks_exact_mut(2) {
            let value = reader.next(&p.windows_render[b]);
            for c in 0..2 {
                frame[c] = ((value[c].clamp(-1.0, 1.0) as f64 * 2147483647.0) as i32) & !255;
            }
        }
        p.windows_render_frames[b].fetch_add((samples.len() / 2) as u64, Ordering::Relaxed);
    }
}

pub struct Transport {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    status: Arc<Mutex<String>>,
}
impl Transport {
    pub fn start(p: Arc<Processor>, buffer: usize) -> Result<Self, String> {
        let exe = usbip_exe::find()
            .filter(|x| x.is_file())
            .ok_or_else(driver_status)?;
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let status = Arc::new(Mutex::new("正在创建 Windows 虚拟设备…".into()));
        let report = status.clone();
        let worker=thread::Builder::new().name("patchbay-usb-audio".into()).spawn(move || {
            log(&format!("Starting dynamic Windows device pairs: {} Hz, buffer {} frames",p.rate,buffer));
            let mut ports=Vec::new();
            let result=(|| -> Result<(),String> {
                let server=Server::start(0,Arc::new(MonotonicClock::new())).map_err(|e|e.to_string())?;
                for bus in 0..p.virtual_buses {
                    let cfg=Uac2Config {vendor_id:0x1209,product_id:0x0001,bcd_device:0x0100,
                        manufacturer:"Soon Link Pro".into(), product:name(bus),
                        serial:format!("soon-link-pro-v1-{}-{}",bus+1,p.rate), sample_rate:p.rate,
                        playback_channels:2,capture_channels:2,
                        playback_name:Some(endpoint_name(&name(bus), bus, true)),
                        capture_name:Some(endpoint_name(&name(bus), bus, false)),capture_connector:false};
                    let streams=Arc::new(Streams{processor:p.clone(),bus,reader:Mutex::new(Reader::new(p.rate,buffer))});
                    server.add(&format!("1-{}",bus+1),Arc::new(Uac2Device::new(cfg,streams)?));
                }
                for bus in 0..p.virtual_buses {
                    if signal.load(Ordering::Acquire) { return Ok(()); }
                    *report.lock().unwrap()=format!("正在连接 Windows 设备 {}/{}…",bus+1,p.virtual_buses);
                    let port=usbip_exe::attach(&exe,server.port(),&format!("1-{}",bus+1))?;
                    ports.push(port);
                    log(&format!("{} attached on USB/IP port {}",name(bus),port));
                }
                p.system_driver_active.store(true,Ordering::Release);
                *report.lock().unwrap()=format!("{} 组 USB 设备已连接 · 通道组备注仅保存在本软件",p.virtual_buses);
                while !signal.load(Ordering::Acquire) {
                    thread::park_timeout(Duration::from_millis(100));
                }
                // Detach only our own ports while the local server is still alive.
                for bus in 0..p.virtual_buses {
                    log(&format!("{} playback_frames={} capture_frames={} playback_dropped={} capture_dropped={}",name(bus),
                        p.windows_capture_frames[bus].load(Ordering::Relaxed),p.windows_render_frames[bus].load(Ordering::Relaxed),
                        p.windows_capture[bus].dropped.load(Ordering::Relaxed),p.windows_render[bus].dropped.load(Ordering::Relaxed)));
                }
                let mut cleanup=Vec::new();
                for port in ports.drain(..).rev() { if let Err(e)=usbip_exe::detach(&exe,port) {cleanup.push(e);} }
                if !cleanup.is_empty() { return Err(cleanup.join("; ")); }
                drop(server); Ok(())
            })();
            // Partial startup failures also release every successfully attached port.
            for port in ports.into_iter().rev() { let _=usbip_exe::detach(&exe,port); }
            p.system_driver_active.store(false,Ordering::Release);
            for enabled in &p.windows_render_enabled { enabled.store(false,Ordering::Release); }
            if let Err(error)=result {
                let error=format!("虚拟设备连接失败：{error}");
                log(&error);
                *report.lock().unwrap()=error.clone();
                if let Ok(mut errors)=p.errors.lock() { errors.push(error); }
            }
            log("Windows virtual device worker stopped");
        }).map_err(|e|e.to_string())?;
        Ok(Self {
            stop,
            worker: Some(worker),
            status,
        })
    }
    pub fn status(&self) -> String {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| "设备状态暂不可用".into())
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

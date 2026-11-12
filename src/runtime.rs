use eframe::egui::{Context, ViewportCommand};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Shell::*, WindowsAndMessaging::*},
};

pub struct Runtime {
    pub hidden: Arc<AtomicBool>,
    pub quit: Arc<AtomicBool>,
    pub tray_error: Option<String>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    tray: Option<Tray>,
    native: Option<Arc<NativeWindow>>,
    context: Context,
}
impl Runtime {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        session: Arc<bridge::Lifecycle>,
        automatic: bool,
    ) -> Self {
        let ctx = cc.egui_ctx.clone();
        let hidden = Arc::new(AtomicBool::new(false));
        let quit = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let native = match NativeWindow::new(cc) {
            Ok(native) => Arc::new(native),
            Err(error) => {
                crate::usb_audio::log(&format!("Runtime native window unavailable: {error}"));
                return Self {
                    hidden,
                    quit,
                    stop,
                    tray_error: Some(error),
                    worker: None,
                    tray: None,
                    native: None,
                    context: ctx,
                };
            }
        };
        let (tray, mut tray_error) = match Tray::new(
            ctx.clone(),
            hidden.clone(),
            quit.clone(),
            session.clone(),
            native.clone(),
        ) {
            Ok(tray) => (Some(tray), None),
            Err(error) => (None, Some(error)),
        };
        if automatic && tray.is_some() {
            hidden.store(true, Ordering::Release);
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
        let watch_stop = stop.clone();
        let watch_quit = quit.clone();
        let watch_hidden = hidden.clone();
        let watch_native = native.clone();
        let context = ctx.clone();
        let worker = thread::Builder::new()
            .name("soon-link-host-watch".into())
            .spawn(move || {
                let mut empty_since = None;
                let mut last_hosts = Instant::now() - Duration::from_secs(1);
                while !watch_stop.load(Ordering::Acquire) {
                    if session.take_show() {
                        crate::usb_audio::log("Runtime: IPC show window requested");
                        show(&ctx, &watch_hidden, &watch_native);
                    }
                    if last_hosts.elapsed() >= Duration::from_secs(1) {
                        if automatic && !session.manually_owned() && session.live_hosts() == 0 {
                            let empty = empty_since.get_or_insert_with(Instant::now);
                            if empty.elapsed() >= Duration::from_secs(2) {
                                watch_quit.store(true, Ordering::Release);
                                crate::usb_audio::log(
                                    "Runtime: last ASIO host exited; closing managed instance",
                                );
                                ctx.send_viewport_cmd(ViewportCommand::Close);
                                watch_native.wake();
                                break;
                            }
                        } else {
                            empty_since = None;
                        }
                        last_hosts = Instant::now();
                    }
                    // winit 0.30's Windows redraw request uses RedrawWindow, which does
                    // not deliver normal paint messages to hidden windows. Post a
                    // coalesced native paint so eframe drains commands and polls the
                    // engine even while hidden. Never wait on the UI thread here.
                    if watch_hidden.load(Ordering::Acquire) || watch_quit.load(Ordering::Acquire) {
                        watch_native.wake();
                    }
                    thread::park_timeout(Duration::from_millis(250));
                }
                session.clear_mixer();
            });
        let worker = match worker {
            Ok(worker) => Some(worker),
            Err(error) => {
                let error = format!("无法创建后台窗口与宿主监视线程：{error}");
                crate::usb_audio::log(&error);
                hidden.store(false, Ordering::Release);
                context.send_viewport_cmd(ViewportCommand::Visible(true));
                native.restore();
                tray_error = Some(error);
                None
            }
        };
        Self {
            hidden,
            quit,
            tray_error,
            stop,
            worker,
            tray,
            native: Some(native),
            context,
        }
    }
    pub fn on_update(&self, ctx: &Context) {
        if let Some(native) = &self.native {
            native.paint_pending.store(false, Ordering::Release);
        }
        if self.quit.load(Ordering::Acquire) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
    pub fn show_window(&self) {
        if let Some(native) = &self.native {
            show(&self.context, &self.hidden, native);
        }
    }
    pub fn close_to_tray(&self, ctx: &Context) -> bool {
        if self.tray.is_none() || self.worker.is_none() || self.quit.load(Ordering::Acquire) {
            return false;
        }
        self.hidden.store(true, Ordering::Release);
        ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        true
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
        self.tray.take();
    }
}
// Borrowed HWND: Runtime joins its workers before eframe destroys the window.
// usize is only transported between threads; no thread owns or destroys it.
struct NativeWindow {
    hwnd: usize,
    paint_pending: AtomicBool,
}
impl NativeWindow {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        match cc.window_handle().map_err(|e| e.to_string())?.as_raw() {
            RawWindowHandle::Win32(handle) => Ok(Self {
                hwnd: handle.hwnd.get() as usize,
                paint_pending: AtomicBool::new(false),
            }),
            _ => Err("无法获取 Windows 主窗口，已禁用隐藏到托盘".into()),
        }
    }
    fn wake(&self) {
        if !self.paint_pending.swap(true, Ordering::AcqRel) {
            unsafe {
                if PostMessageW(self.hwnd as HWND, WM_PAINT, 0, 0) == 0 {
                    self.paint_pending.store(false, Ordering::Release);
                }
            }
        }
    }
    fn restore(&self) {
        unsafe {
            let hwnd = self.hwnd as HWND;
            let command = if IsIconic(hwnd) != 0 {
                SW_RESTORE
            } else {
                SW_SHOW
            };
            ShowWindowAsync(hwnd, command);
        }
        self.wake();
    }
}
fn show(ctx: &Context, hidden: &AtomicBool, native: &NativeWindow) {
    hidden.store(false, Ordering::Release);
    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(ViewportCommand::Focus);
    native.restore();
    ctx.request_repaint();
}
struct Tray {
    hwnd: usize,
    worker: Option<JoinHandle<()>>,
}
struct TrayState {
    ctx: Context,
    hidden: Arc<AtomicBool>,
    quit: Arc<AtomicBool>,
    session: Arc<bridge::Lifecycle>,
    icon: HICON,
    taskbar_created: u32,
    native: Arc<NativeWindow>,
}
const TRAY_MESSAGE: u32 = WM_APP + 73;
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
impl Tray {
    fn new(
        ctx: Context,
        hidden: Arc<AtomicBool>,
        quit: Arc<AtomicBool>,
        session: Arc<bridge::Lifecycle>,
        native: Arc<NativeWindow>,
    ) -> Result<Self, String> {
        let (send, receive) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("soon-link-tray".into())
            .spawn(move || unsafe {
                let instance = GetModuleHandleW(std::ptr::null());
                let class: Vec<u16> = "SoonLinkProTrayWindow_v1\0".encode_utf16().collect();
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(tray_proc),
                    hInstance: instance,
                    lpszClassName: class.as_ptr(),
                    ..std::mem::zeroed()
                };
                RegisterClassW(&wc);
                let hwnd = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null(),
                );
                if hwnd.is_null() {
                    let _ = send.send(Err(std::io::Error::last_os_error().to_string()));
                    return;
                }
                let icon = LoadImageW(
                    instance,
                    1usize as *const u16,
                    IMAGE_ICON,
                    32,
                    32,
                    LR_DEFAULTCOLOR,
                ) as HICON;
                let message: Vec<u16> = "TaskbarCreated\0".encode_utf16().collect();
                let mut state = Box::new(TrayState {
                    ctx,
                    hidden,
                    quit,
                    session,
                    native,
                    icon,
                    taskbar_created: RegisterWindowMessageW(message.as_ptr()),
                });
                SetWindowLongPtrW(
                    hwnd,
                    GWLP_USERDATA,
                    (&mut *state as *mut TrayState) as isize,
                );
                if icon.is_null() || !add_icon(hwnd, icon) {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    DestroyWindow(hwnd);
                    if !icon.is_null() {
                        DestroyIcon(icon);
                    }
                    let _ = send.send(Err("Windows 通知区域图标创建失败".into()));
                    return;
                }
                if send.send(Ok(hwnd as usize)).is_err() {
                    DestroyWindow(hwnd);
                }
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let mut data: NOTIFYICONDATAW = std::mem::zeroed();
                data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                data.hWnd = hwnd;
                data.uID = 1;
                Shell_NotifyIconW(NIM_DELETE, &data);
                DestroyIcon(icon);
                drop(state);
            })
            .map_err(|e| e.to_string())?;
        match receive.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(hwnd)) => Ok(Self {
                hwnd,
                worker: Some(worker),
            }),
            Ok(Err(e)) => {
                let _ = worker.join();
                Err(e)
            }
            Err(_) => Err("Windows 通知区域初始化超时".into()),
        }
    }
}
impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
unsafe fn add_icon(hwnd: HWND, icon: HICON) -> bool {
    let mut data: NOTIFYICONDATAW = std::mem::zeroed();
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = 1;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    data.uCallbackMessage = TRAY_MESSAGE;
    data.hIcon = icon;
    for (out, value) in data
        .szTip
        .iter_mut()
        .zip("Soon Link Pro · 音频路由\0".encode_utf16())
    {
        *out = value;
    }
    if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
        return false;
    }
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    Shell_NotifyIconW(NIM_SETVERSION, &data);
    true
}
unsafe extern "system" fn tray_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TrayState;
    if !state.is_null() {
        let state = &*state;
        if message == state.taskbar_created {
            add_icon(hwnd, state.icon);
            return 0;
        }
        if message == TRAY_MESSAGE {
            match l as u32 & 0xffff {
                NIN_SELECT | NIN_KEYSELECT | WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                    crate::usb_audio::log("Tray: show window (icon click)");
                    show(&state.ctx, &state.hidden, &state.native);
                }
                WM_CONTEXTMENU | WM_RBUTTONUP => {
                    let menu = CreatePopupMenu();
                    for (id, title) in [
                        (1, "显示窗口"),
                        (2, "保持独立运行"),
                        (3, "退出 Soon Link Pro"),
                    ] {
                        let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
                        let flags = if id == 2 && state.session.manually_owned() {
                            MF_STRING | MF_CHECKED
                        } else {
                            MF_STRING
                        };
                        AppendMenuW(menu, flags, id, title.as_ptr());
                    }
                    let mut point: POINT = std::mem::zeroed();
                    GetCursorPos(&mut point);
                    SetForegroundWindow(hwnd);
                    let command = TrackPopupMenu(
                        menu,
                        TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                        point.x,
                        point.y,
                        0,
                        hwnd,
                        std::ptr::null(),
                    );
                    DestroyMenu(menu);
                    PostMessageW(hwnd, WM_NULL, 0, 0);
                    match command {
                        1 => {
                            crate::usb_audio::log("Tray: show window (menu)");
                            show(&state.ctx, &state.hidden, &state.native);
                        }
                        2 => {
                            state.session.keep_running();
                            crate::usb_audio::log("Tray: independent ownership enabled");
                            show(&state.ctx, &state.hidden, &state.native);
                        }
                        3 => {
                            state.quit.store(true, Ordering::Release);
                            crate::usb_audio::log("Tray: quit requested");
                            state.ctx.send_viewport_cmd(ViewportCommand::Close);
                            state.native.wake();
                            state.ctx.request_repaint();
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            return 0;
        }
    }
    if message == WM_CLOSE {
        DestroyWindow(hwnd);
        return 0;
    }
    if message == WM_DESTROY {
        PostQuitMessage(0);
        return 0;
    }
    DefWindowProcW(hwnd, message, w, l)
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod about;
mod app_updates;
mod asio;
mod audio;
mod driver_tools;
mod engine_control;
mod mcp;
mod process_capture;
mod runtime;
mod ui_shell;
mod usb_audio;
mod widgets;
use audio::{Config, Graph};
use eframe::egui::*;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use widgets::{action, icon_at, Icon};
const BG: Color32 = Color32::from_rgb(14, 16, 20);
const PANEL: Color32 = Color32::from_rgb(20, 23, 29);
const CARD: Color32 = Color32::from_rgb(27, 30, 38);
const LINE: Color32 = Color32::from_rgb(45, 49, 60);
const TEXT: Color32 = Color32::from_rgb(234, 237, 245);
const MUTED: Color32 = Color32::from_rgb(133, 142, 161);
const ACCENT: Color32 = Color32::from_rgb(172, 143, 255);
const COLORS: [Color32; 5] = [
    Color32::from_rgb(99, 212, 198),
    Color32::from_rgb(172, 143, 255),
    Color32::from_rgb(100, 173, 249),
    Color32::from_rgb(238, 182, 111),
    Color32::from_rgb(226, 134, 181),
];
#[derive(Clone, Serialize, Deserialize)]
struct Scene {
    name: String,
    graph: Graph,
}
#[derive(Serialize, Deserialize)]
struct Saved {
    config: Config,
    graph: Graph,
    scenes: Vec<Scene>,
    scene: usize,
    #[serde(default)]
    unlinked_stereo: Vec<String>,
    #[serde(default = "enabled_by_default")]
    motion_enabled: bool,
    #[serde(default)]
    low_power_animation: bool,
    #[serde(default)]
    mcp_enabled: bool,
}
fn enabled_by_default() -> bool {
    true
}
struct ChannelCard {
    group: String,
    stereo: bool,
    all_muted: bool,
    any_muted: bool,
    title: String,
    original_title: String,
    indices: Vec<usize>,
    color: Color32,
}
enum CardAction {
    Hide { input: bool, group: String },
    RemoveBus(usize),
    RemoveApp(String),
}
struct CardRename {
    input: bool,
    group: String,
    draft: String,
    focus: bool,
}
struct App {
    mcp_server: Option<soon_link_pro::mcp_transport::LocalServer>,
    mcp_enabled: bool,
    mcp_status: String,
    runtime: Option<runtime::Runtime>,
    hardware_available: bool,
    channel_manager: bool,
    channel_tab: usize,
    layout_signature: String,
    notices: ui_shell::Toasts,
    last_notice: String,
    config: Config,
    graph: Graph,
    scenes: Vec<Scene>,
    scene: usize,
    engine: Option<engine_control::Snapshot>,
    engine_controller: engine_control::Controller,
    engine_revision: u64,
    engine_wanted: bool,
    engine_busy: bool,
    restart_at: Option<Instant>,
    card_action: Option<CardAction>,
    inputs: Vec<String>,
    outputs: Vec<String>,
    asios: Vec<asio::Device>,
    unified_asio: bool,
    tab: usize,
    settings: bool,
    settings_tab: usize,
    about: about::About,
    driver_tools: driver_tools::Tools,
    selected: Option<usize>,
    route: Option<(usize, usize)>,
    rename: Option<(bool, usize)>,
    card_rename: Option<CardRename>,
    panic: bool,
    levels: Vec<f32>,
    last_levels: Instant,
    last_devices: Instant,
    status: String,
    system_status: String,
    app_picker: bool,
    app_candidates: Vec<process_capture::Target>,
    app_scan: Option<std::sync::mpsc::Receiver<process_capture::Inventory>>,
    last_app_scan: Instant,
    app_audio_status: String,
    app_filter: String,
    frames: u32,
    screenshot: Option<PathBuf>,
    requested: bool,
    last_save: Instant,
    dirty: bool,
    audio_sync_pending: bool,
    unlinked_stereo: Vec<String>,
    device_refresh: Option<std::sync::mpsc::Receiver<(Vec<String>, Vec<String>)>>,
    motion_enabled: bool,
    low_power_animation: bool,
}
fn data_path() -> PathBuf {
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(root)
            .join("SoonLinkPro")
            .join("soon-link-pro-settings.json");
    }
    legacy_data_path()
}
fn legacy_data_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("soon-link-pro-settings.json")))
        .unwrap_or_else(|| PathBuf::from("soon-link-pro-settings.json"))
}
fn button(ui: &mut Ui, label: impl Into<WidgetText>, active: bool) -> Response {
    widgets::text_button(ui, label, active)
}
fn small(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(MUTED));
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>) -> Self {
        let mut fonts = FontDefinitions::default();
        if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/msyh.ttc") {
            fonts
                .font_data
                .insert("chinese".into(), FontData::from_owned(bytes).into());
            fonts
                .families
                .get_mut(&FontFamily::Proportional)
                .unwrap()
                .push("chinese".into());
        }
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_theme(ThemePreference::Dark);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = Visuals::dark();
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = PANEL;
        style.visuals.extreme_bg_color = BG;
        style.visuals.faint_bg_color = CARD;
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        style.visuals.widgets.inactive.bg_fill = CARD;
        style.visuals.widgets.inactive.weak_bg_fill = CARD;
        style.visuals.widgets.active.bg_fill = ACCENT;
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(48, 43, 65);
        style.visuals.selection.bg_fill = Color32::from_rgb(69, 53, 107);
        style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        style.spacing.item_spacing = vec2(10.0, 8.0);
        style.spacing.button_padding = vec2(13.0, 7.0);
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(13.0));
        ui_shell::refine_style(&mut style);
        cc.egui_ctx.set_style(style);
        let saved = std::fs::read(data_path())
            .or_else(|_| std::fs::read(legacy_data_path()))
            .or_else(|_| std::fs::read(legacy_data_path().with_file_name("patchbay-settings.json")))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        let config = saved
            .as_ref()
            .and_then(|s| serde_json::from_value::<Config>(s["config"].clone()).ok())
            .unwrap_or_default();
        let graph = saved
            .as_ref()
            .and_then(|s| Graph::from_value(s["graph"].clone()))
            .unwrap_or_default();
        let mut scenes = saved
            .as_ref()
            .and_then(|s| s["scenes"].as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| {
                        Some(Scene {
                            name: s["name"].as_str()?.into(),
                            graph: Graph::from_value(s["graph"].clone())?,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if scenes.is_empty() {
            scenes.push(Scene {
                name: "直播工作台".into(),
                graph: graph.clone(),
            });
        }
        let scene = saved
            .as_ref()
            .and_then(|s| s["scene"].as_u64())
            .map(|v| v as usize)
            .unwrap_or(0)
            .min(scenes.len() - 1);
        let unlinked_stereo = saved
            .as_ref()
            .and_then(|s| serde_json::from_value(s["unlinked_stereo"].clone()).ok())
            .unwrap_or_default();
        let motion_enabled = saved
            .as_ref()
            .and_then(|s| s["motion_enabled"].as_bool())
            .unwrap_or(true);
        let low_power_animation = saved
            .as_ref()
            .and_then(|s| s["low_power_animation"].as_bool())
            .unwrap_or(false);
        let mcp_enabled = saved
            .as_ref()
            .and_then(|s| s["mcp_enabled"].as_bool())
            .unwrap_or(false);
        let (inputs, outputs) = audio::wasapi_devices();
        let asios = asio::devices();
        ui_shell::apply_system_corners(cc);
        let automatic_audio = screenshot.is_none();
        let mut app = Self {
            mcp_server: None,
            mcp_enabled,
            mcp_status: String::new(),
            runtime: None,
            hardware_available: false,
            channel_manager: false,
            channel_tab: 0,
            layout_signature: String::new(),
            notices: ui_shell::Toasts::default(),
            last_notice: "正在自动启动音频引擎…".into(),
            config,
            graph,
            scenes,
            scene,
            engine: None,
            engine_controller: engine_control::Controller::new(),
            engine_revision: 0,
            engine_wanted: automatic_audio,
            engine_busy: automatic_audio,
            restart_at: automatic_audio.then(Instant::now),
            card_action: None,
            inputs,
            outputs,
            asios,
            unified_asio: asio::unified_installed(),
            tab: 0,
            settings: false,
            settings_tab: 0,
            about: about::About::default(),
            driver_tools: driver_tools::Tools::default(),
            selected: None,
            route: None,
            rename: None,
            card_rename: None,
            panic: false,
            levels: Vec::new(),
            last_levels: Instant::now(),
            last_devices: Instant::now(),
            status: "正在自动启动音频引擎…".into(),
            system_status: usb_audio::driver_status(),
            app_picker: false,
            app_candidates: Vec::new(),
            app_scan: None,
            last_app_scan: Instant::now(),
            app_audio_status: String::new(),
            app_filter: String::new(),
            frames: 0,
            screenshot,
            requested: false,
            last_save: Instant::now(),
            dirty: false,
            audio_sync_pending: false,
            unlinked_stereo,
            device_refresh: None,
            motion_enabled,
            low_power_animation,
        };
        if automatic_audio && mcp_enabled {
            app.start_mcp(&cc.egui_ctx);
        }
        app
    }
    fn start_mcp(&mut self, ctx: &Context) {
        let context = ctx.clone();
        match soon_link_pro::mcp_transport::LocalServer::start(move || context.request_repaint()) {
            Ok(server) => {
                self.mcp_server = Some(server);
                self.mcp_status = "已启用，等待 Agent 连接".into();
            }
            Err(error) => {
                self.mcp_enabled = false;
                self.mcp_status = format!("无法启用 Agent 控制：{error}");
            }
        }
    }
    fn poll_mcp(&mut self) {
        if let Some(mut server) = self.mcp_server.take() {
            server.poll(|name, arguments| match name {
                "get_routing_state" => {
                    mcp::validate_empty(arguments)?;
                    Ok(mcp::state(
                        &self.graph,
                        self.panic,
                        self.engine.is_some(),
                        self.engine_busy,
                        self.hardware_available,
                    ))
                }
                "patch_routing" => {
                    let (result, changed) =
                        mcp::patch(&mut self.graph, &mut self.panic, arguments)?;
                    if changed {
                        self.dirty = true;
                        self.audio_sync_pending = true;
                        self.status = "Agent 已更新跳线／音量／静音".into();
                        self.mcp_status = "已启用，最近一次 Agent 修改已应用".into();
                    }
                    Ok(result)
                }
                _ => Err("Unknown routing tool".into()),
            });
            self.mcp_server = Some(server);
        }
    }
    fn mcp_panel(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Agent 跳线控制 · MCP").size(15.0).strong());
        small(ui, "让本机的 Agent 查看通道、批量跳线、调节音量与静音。");
        ui.add_space(8.0);
        if ui
            .checkbox(&mut self.mcp_enabled, "允许本机 Agent 控制跳线")
            .changed()
        {
            if self.mcp_enabled {
                self.start_mcp(ui.ctx());
            } else {
                self.mcp_server = None;
                self.mcp_status = "已关闭，Agent 无法读取或修改跳线".into();
            }
            self.dirty = true;
        }
        if !self.mcp_status.is_empty() {
            ui.label(&self.mcp_status);
        }
        ui.add_space(10.0);
        small(
            ui,
            "启用后可直接改变正在播放的声音；关闭此开关即可停止后续控制。仅支持本机连接。",
        );
        small(
            ui,
            "先打开本软件，再把下方配置加入支持 stdio MCP 的 Agent。窗口收进托盘后仍可使用。",
        );
        let config = serde_json::to_string_pretty(&soon_link_pro::mcp_transport::client_config())
            .unwrap_or_default();
        ui.add_space(8.0);
        if button(ui, "复制 MCP 配置", true).clicked() {
            ui.ctx().copy_text(config.clone());
        }
        ui.add_space(6.0);
        ui.label(RichText::new(config).monospace().size(11.0));
        ui.add_space(8.0);
        small(
            ui,
            "按单声道精确操作；立体声需要分别指定 L／R。不更换设备、不启动引擎、不安装驱动。",
        );
        small(
            ui,
            "接线、音量和通道静音沿用自动保存；全部静音与界面一致，仅在本次运行有效。",
        );
    }
    fn save(&mut self) {
        let saved = Saved {
            config: self.config.clone(),
            graph: self.graph.clone(),
            scenes: self.scenes.clone(),
            scene: self.scene,
            unlinked_stereo: self.unlinked_stereo.clone(),
            motion_enabled: self.motion_enabled,
            low_power_animation: self.low_power_animation,
            mcp_enabled: self.mcp_enabled,
        };
        if let Ok(bytes) = serde_json::to_vec_pretty(&saved) {
            let p = data_path();
            let tmp = p.with_extension("json.tmp");
            let prepared = p.parent().map(std::fs::create_dir_all).unwrap_or(Ok(()));
            match prepared
                .and_then(|_| std::fs::write(&tmp, bytes))
                .and_then(|_| std::fs::rename(&tmp, p))
            {
                Ok(()) => {
                    self.dirty = false;
                    self.last_save = Instant::now();
                }
                Err(e) => self.status = format!("保存失败：{e}"),
            }
        }
    }
    fn queue_restart(&mut self) {
        if !self.engine_wanted || self.driver_tools.busy() {
            return;
        }
        self.engine_revision = self.engine_revision.wrapping_add(1);
        self.engine = None;
        self.engine_busy = true;
        self.restart_at = Some(Instant::now() + Duration::from_millis(250));
        self.selected = None;
        self.route = None;
        self.rename = None;
        self.levels.fill(0.0);
        self.status = "正在自动更新音频布局，声音会短暂中断…".into();
    }
    fn poll_engine(&mut self) {
        while let Some(event) = self.engine_controller.poll() {
            match event {
                engine_control::Event::Started {
                    revision,
                    graph,
                    snapshot,
                } if revision == self.engine_revision
                    && self.engine_wanted
                    && self.restart_at.is_none() =>
                {
                    let (inputs, outputs) = graph.counts();
                    // Keep edits made while opening the device; only reconcile hardware metadata.
                    self.graph.reconfigure(inputs, outputs);
                    for (current, received) in [
                        (&mut self.graph.inputs, &graph.inputs),
                        (&mut self.graph.outputs, &graph.outputs),
                    ] {
                        for node in current
                            .iter_mut()
                            .filter(|n| n.key.starts_with("hardware:"))
                        {
                            if let Some(actual) = received.iter().find(|n| n.key == node.key) {
                                if node.name == node.device_name {
                                    node.name = actual.device_name.clone();
                                }
                                node.device_name = actual.device_name.clone();
                            }
                        }
                    }
                    self.hardware_available = true;
                    self.status = snapshot.description.clone();
                    usb_audio::log(&format!("Audio engine ready: {}", snapshot.description));
                    self.engine = Some(snapshot);
                    self.engine_busy = false;
                    self.audio_sync_pending = true;
                    self.queue_device_refresh();
                }
                engine_control::Event::Failed { revision, error }
                    if revision == self.engine_revision =>
                {
                    self.engine = None;
                    self.engine_busy = false;
                    self.status = format!("音频启动失败：{error}，请检查设置后重试。");
                    usb_audio::log(&format!("Audio engine failed: {error}"));
                    if let Some(runtime) = &self.runtime {
                        runtime.show_window();
                    }
                }
                engine_control::Event::Stopped { revision } if revision == self.engine_revision => {
                    self.engine_busy = false;
                    if self.driver_tools.pending_install {
                        self.driver_tools.pending_install = false;
                        self.driver_tools.install = Some(driver_tools::install());
                        self.status = "音频已暂停，正在打开驱动安装器…".into();
                    }
                }
                _ => (),
            }
        }
        if self
            .restart_at
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.restart_at = None;
            usb_audio::log(&format!(
                "Audio engine restart requested: revision={}",
                self.engine_revision
            ));
            if let Err(error) = self.engine_controller.restart(
                self.engine_revision,
                self.config.clone(),
                self.graph.clone(),
            ) {
                self.engine_busy = false;
                self.status = error;
            }
        }
    }
    fn start_stop(&mut self) {
        if self.driver_tools.busy() {
            self.notices
                .push("驱动安装结束后会恢复音频，请先完成或取消安装器。");
            return;
        }
        if self.engine.is_some() || self.engine_busy {
            self.engine_wanted = false;
            self.engine_busy = false;
            self.restart_at = None;
            self.engine_revision = self.engine_revision.wrapping_add(1);
            self.engine = None;
            self.engine_controller.stop(self.engine_revision);
            self.levels.fill(0.0);
            self.status = "音频引擎已暂停；点击启动可恢复".into();
        } else {
            self.engine_wanted = true;
            self.queue_restart();
        }
    }
    fn apply_card_action(&mut self) {
        let Some(action) = self.card_action.take() else {
            return;
        };
        match action {
            CardAction::Hide { input, group } => {
                let nodes = if input {
                    &mut self.graph.inputs
                } else {
                    &mut self.graph.outputs
                };
                for node in nodes {
                    if Self::stereo_group(&node.key) == group {
                        node.visible = false;
                    }
                }
                self.dirty = true;
                self.status = "通道组已移出面板，接线和声音仍保留；可在通道管理中恢复。".into();
            }
            CardAction::RemoveBus(bus) => match self.graph.remove_bus(bus) {
                Ok(()) => {
                    self.unlinked_stereo.retain_mut(|key| {
                        let Some((prefix, index)) = key.rsplit_once(':') else {
                            return true;
                        };
                        if !prefix.ends_with(":virtual") {
                            return true;
                        }
                        let Ok(index) = index.parse::<usize>() else {
                            return true;
                        };
                        if index == bus {
                            return false;
                        }
                        if index > bus {
                            *key = format!("{prefix}:{}", index - 1);
                        }
                        true
                    });
                    self.dirty = true;
                    self.audio_sync_pending = true;
                    self.queue_restart();
                    self.status =
                        "虚拟总线已删除，其他接线已保留；正在自动更新设备与 ASIO。".into();
                }
                Err(error) => self.notices.push(error),
            },
            CardAction::RemoveApp(exe) => {
                self.graph.remove_application(&exe);
                self.unlinked_stereo
                    .retain(|key| !key.ends_with(&format!(":app:{}", exe.to_lowercase())));
                self.dirty = true;
                self.audio_sync_pending = true;
                self.queue_restart();
                self.status = "应用输入及其接线已移除，正在自动更新音频。".into();
            }
        }
        self.selected = None;
        self.route = None;
        self.rename = None;
    }
    fn queue_device_refresh(&mut self) {
        if self.device_refresh.is_some() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        if std::thread::Builder::new()
            .name("audio-device-list".into())
            .spawn(move || {
                let _ = sender.send(audio::wasapi_devices());
            })
            .is_ok()
        {
            self.device_refresh = Some(receiver);
        }
    }
    fn refresh(&mut self) {
        self.queue_device_refresh();
        self.asios = asio::devices();
        self.unified_asio = asio::unified_installed();
        self.system_status = self
            .engine
            .as_ref()
            .map(|e| e.system_status())
            .unwrap_or_else(usb_audio::driver_status);
        self.layout_signature.clear();
        self.status = "设备列表已刷新".into();
    }
    fn header(&mut self, ctx: &Context) {
        ui_shell::titlebar(ctx);
        TopBottomPanel::top("header")
            .exact_height(66.0)
            .frame(Frame::new().inner_margin(Margin::symmetric(27, 12)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (i, (icon, label)) in
                        [(Icon::Routes, "跳线视图"), (Icon::Matrix, "路由矩阵")]
                            .iter()
                            .enumerate()
                    {
                        if action(ui, *icon, label, self.tab == i).clicked() {
                            self.tab = i;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let about_label = if self.about.updates.available() {
                            "发现新版本"
                        } else {
                            "关于与帮助"
                        };
                        if action(
                            ui,
                            Icon::Info,
                            about_label,
                            self.about.open || self.about.updates.available(),
                        )
                        .clicked()
                        {
                            self.about.open = true;
                        }
                        if action(ui, Icon::Settings, "设置", self.settings).clicked() {
                            self.settings = true;
                        }
                        ui.add_space(8.0);
                        let live = self.engine.is_some() || self.engine_busy;
                        if widgets::action_tinted(
                            ui,
                            if live { Icon::Stop } else { Icon::Play },
                            if self.engine_busy {
                                "更新音频…"
                            } else if live {
                                "暂停音频"
                            } else {
                                "启动音频"
                            },
                            true,
                            if live { COLORS[3] } else { COLORS[0] },
                        )
                        .clicked()
                        {
                            self.start_stop();
                        }
                        let (indicator, label) = if self.engine_busy {
                            (widgets::StatusIndicator::Busy, "自动更新中")
                        } else if live {
                            (widgets::StatusIndicator::Active, "引擎运行中")
                        } else {
                            (widgets::StatusIndicator::Idle, "引擎待机")
                        };
                        widgets::status_label(
                            ui,
                            indicator,
                            label,
                            if live { COLORS[0] } else { MUTED },
                        );
                    });
                });
            });
    }
    fn footer(&mut self, ctx: &Context) {
        TopBottomPanel::bottom("footer")
            .resizable(false)
            .frame(Frame::new().inner_margin(Margin::symmetric(27, 10)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                Frame::new()
                    .fill(Color32::from_rgb(21, 26, 35))
                    .corner_radius(10.0)
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(&self.status).size(12.0).color(
                            if self.status.contains("失败") || self.status.contains("不支持") {
                                COLORS[3]
                            } else {
                                TEXT
                            },
                        ));
                        self.rack(ui);
                    });
            });
    }

    fn scene_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().interact_size.y = 36.0;
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.set_min_height(36.0);
            small(ui, "场景");
            let mut selected = self.scene;
            ComboBox::from_id_salt("scenes")
                .width(205.0)
                .selected_text(&self.scenes[self.scene].name)
                .show_ui(ui, |ui| {
                    for (i, scene) in self.scenes.iter().enumerate() {
                        ui.selectable_value(&mut selected, i, &scene.name);
                    }
                });
            if selected != self.scene {
                self.engine = None;
                self.selected = None;
                self.route = None;
                self.rename = None;
                self.levels.fill(0.0);
                self.scene = selected;
                self.graph = self.scenes[selected].graph.clone();
                self.dirty = true;
                self.audio_sync_pending = true;
                self.queue_restart();
                self.status = "场景已切换，正在自动更新音频".into();
            }
            ui.menu_button("＋ 新建", |ui| {
                if ui.button("新建空白场景").clicked() {
                    let (ni, no) = self.graph.counts();
                    let mut g = Graph::new(ni, no);
                    for row in &mut g.routes {
                        for route in row {
                            route.on = false;
                        }
                    }
                    self.engine = None;
                    self.selected = None;
                    self.route = None;
                    self.rename = None;
                    self.levels.fill(0.0);
                    self.scene = self.scenes.len();
                    self.graph = g.clone();
                    self.scenes.push(Scene {
                        name: format!("自定义场景 {}", self.scene + 1),
                        graph: g,
                    });
                    self.dirty = true;
                    self.audio_sync_pending = true;
                    self.queue_restart();
                    self.status = "空白场景已创建，按需添加通道即可".into();
                    ui.close_menu();
                }
                if ui.button("复制当前场景").clicked() {
                    self.scene = self.scenes.len();
                    self.scenes.push(Scene {
                        name: format!("场景副本 {}", self.scene + 1),
                        graph: self.graph.clone(),
                    });
                    self.dirty = true;
                    self.audio_sync_pending = true;
                    self.status = "场景副本已创建".into();
                    ui.close_menu();
                }
            });
            if action(ui, Icon::Save, "保存场景", false).clicked() {
                if let Some(scene) = self.scenes.get_mut(self.scene) {
                    scene.graph = self.graph.clone();
                }
                self.save();
                if !self.dirty {
                    self.status = "当前场景已保存".into();
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::action_tinted(
                    ui,
                    if self.panic {
                        Icon::Muted
                    } else {
                        Icon::Volume
                    },
                    if self.panic {
                        "恢复输出"
                    } else {
                        "全部静音"
                    },
                    self.panic,
                    COLORS[3],
                )
                .on_hover_text("紧急关闭全部输出；再次点击恢复")
                .clicked()
                {
                    self.panic = !self.panic;
                    self.dirty = true;
                    self.audio_sync_pending = true;
                    self.status = if self.panic {
                        "全部输出已静音，点击恢复输出可继续"
                    } else {
                        "输出已恢复"
                    }
                    .into();
                }
            });
        });
        ui.add_space(4.0);
        Frame::new()
            .inner_margin(Margin::symmetric(2, 2))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
                ui.horizontal_wrapped(|ui| {
                    ui.set_min_height(22.0);
                    small(
                        ui,
                        if self.config.backend == 1 {
                            "ASIO"
                        } else {
                            "WASAPI"
                        },
                    );
                    let dev = if self.config.backend == 1 {
                        if self.config.asio_name.is_empty() {
                            "未选择声卡"
                        } else {
                            &self.config.asio_name
                        }
                    } else if self.config.output.is_empty() {
                        "Windows 默认音频设备"
                    } else {
                        &self.config.output
                    };
                    ui.add(Label::new(RichText::new(dev).size(12.0)).truncate())
                        .on_hover_text(dev);
                    small(
                        ui,
                        &format!("· {:.1} kHz", self.config.rate as f32 / 1000.0),
                    );
                    let buffer = if self.config.backend == 1 {
                        format!(
                            "· {} samples · {:.2} ms / block",
                            self.config.buffer,
                            self.config.buffer as f64 / self.config.rate.max(1) as f64 * 1000.0
                        )
                    } else {
                        "· 系统缓冲 · 共享模式".into()
                    };
                    ui.label(RichText::new(buffer).size(11.0).color(MUTED))
                        .on_hover_text("每块时长不代表端到端或往返音频延迟。");
                    small(
                        ui,
                        &format!(
                            "· {} 条连接",
                            self.graph.routes.iter().flatten().filter(|r| r.on).count()
                        ),
                    );
                });
            });
        ui.add_space(4.0);
    }
    fn channel_color(key: &str) -> Color32 {
        let index = if let Some(ch) = key
            .strip_prefix("hardware:")
            .and_then(|s| s.parse::<usize>().ok())
        {
            ch / 2
        } else if key.starts_with("virtual:") {
            key.split(':')
                .nth(1)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0)
                + 1
        } else if key.starts_with("system:") {
            3
        } else {
            key.rsplit_once(':')
                .map(|(root, _)| {
                    root.bytes().fold(0usize, |hash, c| {
                        hash.wrapping_mul(31).wrapping_add(c as usize)
                    })
                })
                .unwrap_or(4)
        };
        COLORS[index % COLORS.len()]
    }
    fn stereo_group(key: &str) -> String {
        Graph::card_group(key)
    }
    fn open_card_rename(&mut self, input: bool, group: &str) {
        self.rename = None;
        self.card_rename = Some(CardRename {
            input,
            group: group.into(),
            draft: self.graph.card_name(input, group).unwrap_or("").into(),
            focus: true,
        });
    }

    fn toggle_card_mute(&mut self, input: bool, group: &str) {
        let nodes = if input {
            &mut self.graph.inputs
        } else {
            &mut self.graph.outputs
        };
        let mute = !nodes
            .iter()
            .filter(|node| Graph::card_group(&node.key) == group)
            .all(|node| node.mute);
        for node in nodes
            .iter_mut()
            .filter(|node| Graph::card_group(&node.key) == group)
        {
            node.mute = mute;
        }
        // Includes hidden companions; individual fader values and L/R linking stay intact.
        self.dirty = true;
        self.audio_sync_pending = true;
    }
    fn set_channel_fader(&mut self, input: bool, index: usize, value: f32) {
        let nodes = if input {
            &mut self.graph.inputs
        } else {
            &mut self.graph.outputs
        };
        let group = Self::stereo_group(&nodes[index].key);
        let linked = !self
            .unlinked_stereo
            .contains(&format!("{}:{}", input, group));
        let delta = value - nodes[index].fader;
        for (i, node) in nodes.iter_mut().enumerate() {
            if i == index {
                node.fader = value;
            } else if linked && Self::stereo_group(&node.key) == group {
                node.fader = (node.fader + delta).clamp(0.0, 1.0);
            }
        }
        self.dirty = true;
        self.audio_sync_pending = true;
    }
    fn channel_cards(&self, input: bool) -> Vec<ChannelCard> {
        self.channel_groups(input, false)
    }
    fn channel_groups(&self, input: bool, include_hidden: bool) -> Vec<ChannelCard> {
        let nodes = if input {
            &self.graph.inputs
        } else {
            &self.graph.outputs
        };
        let mut group_counts = std::collections::HashMap::<String, (usize, usize)>::new();
        for node in nodes {
            let (count, muted) = group_counts
                .entry(Self::stereo_group(&node.key))
                .or_default();
            *count += 1;
            *muted += usize::from(node.mute);
        }
        let mut cards: Vec<ChannelCard> = Vec::new();
        let mut keys: Vec<String> = Vec::new();
        for (index, node) in nodes.iter().enumerate().filter(|(_, n)| {
            (include_hidden || n.visible)
                && (include_hidden || !n.key.starts_with("hardware:") || self.hardware_available)
        }) {
            let (group, title) = if let Some(ch) = node
                .key
                .strip_prefix("hardware:")
                .and_then(|s| s.parse::<usize>().ok())
            {
                let title = if self.config.backend == 1 {
                    format!(
                        "{} · {}–{}",
                        self.config.asio_name,
                        ch / 2 * 2 + 1,
                        (ch / 2 * 2 + 2).min(if input {
                            self.graph.counts().0
                        } else {
                            self.graph.counts().1
                        })
                    )
                } else if (if input {
                    self.graph.counts().0
                } else {
                    self.graph.counts().1
                }) == 1
                {
                    node.device_name.clone()
                } else {
                    node.device_name
                        .rsplit_once(" · ")
                        .map(|(base, _)| base.to_string())
                        .unwrap_or_else(|| node.device_name.clone())
                };
                (format!("hardware:{}", ch / 2), title)
            } else if node.key.starts_with("virtual:") {
                let bus = node
                    .key
                    .split(':')
                    .nth(1)
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                let title = crate::usb_audio::name(bus);
                (format!("virtual:{bus}"), title)
            } else if node.key.starts_with("system:") {
                (String::from("system"), String::from("系统声音"))
            } else {
                let root = node
                    .key
                    .rsplit_once(':')
                    .map(|(root, _)| root)
                    .unwrap_or(&node.key);
                let title = self
                    .graph
                    .applications
                    .iter()
                    .find(|app| root == format!("app:{}", app.exe.to_lowercase()))
                    .map(|app| app.label())
                    .unwrap_or_else(|| node.name.clone());
                (root.to_string(), title)
            };
            let original_title = title;
            let title = self
                .graph
                .group_display_name(input, &group, &original_title);
            if let Some(slot) = keys.iter().position(|key| key == &group) {
                cards[slot].indices.push(index);
            } else {
                keys.push(group.clone());
                let (count, muted) = group_counts.get(&group).copied().unwrap_or_default();
                cards.push(ChannelCard {
                    stereo: count > 1,
                    all_muted: count > 0 && muted == count,
                    any_muted: muted > 0,
                    group,
                    title,
                    original_title,
                    indices: vec![index],
                    color: Self::channel_color(&node.key),
                });
            }
        }
        cards
    }
    fn add_virtual_bus(&mut self) {
        match self.graph.add_bus() {
            Ok(()) => {
                self.selected = None;
                self.route = None;
                self.rename = None;
                self.dirty = true;
                self.audio_sync_pending = true;
                self.queue_restart();
                self.status = format!(
                    "已添加虚拟总线 {}，正在自动更新设备与 ASIO",
                    self.graph.bus_count
                );
            }
            Err(error) => self.notices.push(error),
        }
    }
    fn channel_toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(40.0);
            ui.label(
                RichText::new(if self.tab == 0 {
                    "跳线面板"
                } else {
                    "路由矩阵"
                })
                .size(15.0)
                .strong(),
            );
            let visible = self
                .graph
                .inputs
                .iter()
                .chain(&self.graph.outputs)
                .filter(|n| n.visible)
                .count();
            small(ui, &format!("{} 个显示通道", visible));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if action(ui, Icon::Channels, "通道管理", false)
                    .on_hover_text("按实际设备名称选择显示或隐藏，不改变已有接线")
                    .clicked()
                {
                    self.channel_manager = true;
                }
                if action(ui, Icon::Plus, "虚拟总线", false).clicked() {
                    self.add_virtual_bus();
                }
                if action(ui, Icon::Plus, "添加应用", false).clicked() {
                    self.refresh_app_candidates();
                    self.app_picker = true;
                }
            });
        });
        ui.add_space(8.0);
    }
    fn board(&mut self, ui: &mut Ui, height: f32) {
        let inputs = self.channel_cards(true);
        let outputs = self.channel_cards(false);
        let total_height = |cards: &[ChannelCard]| {
            cards
                .iter()
                .map(|card| 48.0 + card.indices.len() as f32 * 44.0)
                .sum::<f32>()
        };
        let height = height.max(total_height(&inputs).max(total_height(&outputs)) + 65.0);
        let width = ui.available_width();
        let card_width = (width * 0.30).clamp(245.0, 370.0);
        let (rect, background) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        if background.clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
            self.selected = None;
        }
        let painter = ui.painter_at(rect);
        ui_shell::patchboard(&painter, rect);
        let left = rect.left() + 18.0;
        let right = rect.right() - 18.0 - card_width;
        painter.text(
            pos2(left, rect.top() + 17.0),
            Align2::LEFT_TOP,
            "输入来源",
            FontId::proportional(13.0),
            MUTED,
        );
        painter.text(
            pos2(right, rect.top() + 17.0),
            Align2::LEFT_TOP,
            "输出目标",
            FontId::proportional(13.0),
            MUTED,
        );
        let mut input_ports = vec![None; self.graph.inputs.len()];
        let mut output_ports = vec![None; self.graph.outputs.len()];
        let mut input_cards = Vec::new();
        let mut output_cards = Vec::new();
        for (cards, x, ports, rectangles) in [
            (&inputs, left, &mut input_ports, &mut input_cards),
            (&outputs, right, &mut output_ports, &mut output_cards),
        ] {
            let mut y = rect.top() + 46.0;
            for card in cards {
                let r = Rect::from_min_size(
                    pos2(x, y),
                    vec2(card_width, 38.0 + card.indices.len() as f32 * 44.0),
                );
                for (row, &index) in card.indices.iter().enumerate() {
                    ports[index] = Some(pos2(
                        if x == left { r.right() } else { r.left() },
                        r.top() + 49.0 + row as f32 * 44.0,
                    ));
                }
                rectangles.push(r);
                y = r.bottom() + 10.0;
            }
        }
        let mid = rect.center().x;
        let bezier = |points: [Pos2; 4], t: f32| {
            let u = 1.0 - t;
            (points[0].to_vec2() * u * u * u
                + points[1].to_vec2() * 3.0 * u * u * t
                + points[2].to_vec2() * 3.0 * u * t * t
                + points[3].to_vec2() * t * t * t)
                .to_pos2()
        };
        let time = ui.input(|i| i.time);
        for (i, start) in input_ports.iter().enumerate() {
            let Some(start) = *start else {
                continue;
            };
            for (o, end) in output_ports.iter().enumerate() {
                let Some(end) = *end else {
                    continue;
                };
                let route = self.graph.routes[i][o];
                if !route.on {
                    continue;
                }
                let muted = route.mute
                    || self.graph.inputs[i].mute
                    || self.graph.outputs[o].mute
                    || self.panic;
                let color = Self::channel_color(&self.graph.inputs[i].key);
                let selected = self.selected == Some(i) || self.route == Some((i, o));
                let points = [
                    start,
                    pos2(mid - 55.0, start.y),
                    pos2(mid + 55.0, end.y),
                    end,
                ];
                let cable_rect = Rect::from_two_pos(start, end).expand(12.0);
                if !painter.clip_rect().intersects(cable_rect) {
                    continue;
                }
                let target_color = Self::channel_color(&self.graph.outputs[o].key);
                ui_shell::route_cable(
                    &painter,
                    points,
                    color,
                    target_color,
                    if selected { 2.1 } else { 1.4 },
                    if muted {
                        0.10
                    } else if selected {
                        0.85
                    } else {
                        0.35
                    },
                );
                let signal = self.levels.get(i).copied().unwrap_or(0.0)
                    * bridge::gain(self.graph.inputs[i].fader)
                    * bridge::gain(route.fader)
                    * bridge::gain(self.graph.outputs[o].fader);
                if self.motion_enabled && self.engine.is_some() && !muted && signal > 0.0003 {
                    let phase = ((time * 0.42 + (i * 7 + o * 11) as f64 * 0.071) % 1.0) as f32;
                    let strength =
                        ((20.0 * signal.max(0.0001).log10() + 65.0) / 65.0).clamp(0.15, 1.0);
                    // One gradient mesh rather than dozens of separate glowing strokes.
                    // Its final sample and the head use precisely the same curve parameter.
                    ui_shell::signal_beam(&painter, points, phase, color, target_color, strength);
                    // The renderer's vsync paces active animation; a timer here can miss
                    // every other refresh once drawing time is added to its delay.
                    if self.low_power_animation || !ui.input(|i| i.focused) {
                        ui.ctx().request_repaint_after(Duration::from_millis(33));
                    } else {
                        ui.ctx().request_repaint();
                    }
                }
                let center = bezier(points, 0.5);
                let response = ui
                    .interact(
                        Rect::from_center_size(center, vec2(22.0, 22.0)),
                        Id::new(("cable", i, o)),
                        Sense::click(),
                    )
                    .on_hover_text("调整连接音量或移除");
                if response.hovered() || selected {
                    painter.circle_stroke(center, 4.0, Stroke::new(1.0, color));
                }
                if response.clicked() {
                    self.route = Some((i, o));
                }
            }
        }
        for ((card, rect), input) in inputs
            .iter()
            .zip(input_cards)
            .map(|pair| (pair, true))
            .chain(outputs.iter().zip(output_cards).map(|pair| (pair, false)))
        {
            self.channel_card(ui, rect, input, card);
        }
        for (i, point) in input_ports.iter().enumerate() {
            if let Some(point) = *point {
                let response = ui
                    .interact(
                        Rect::from_center_size(point, vec2(24.0, 30.0)),
                        Id::new(("inport", i)),
                        Sense::click_and_drag(),
                    )
                    .on_hover_text("选择或拖动此声道接线");
                let color = Self::channel_color(&self.graph.inputs[i].key);
                painter.circle_filled(point, 5.0, color);
                if self.selected == Some(i) {
                    painter.circle_stroke(point, 9.0, Stroke::new(1.0, color));
                }
                if response.clicked() || response.drag_started() {
                    self.selected = Some(i);
                }
                if response.dragged() {
                    if let Some(mouse) = ui.input(|i| i.pointer.interact_pos()) {
                        painter.line_segment([point, mouse], Stroke::new(1.5, color));
                    }
                }
            }
        }
        for (o, point) in output_ports.iter().enumerate() {
            if let Some(point) = *point {
                let response = ui
                    .interact(
                        Rect::from_center_size(point, vec2(24.0, 30.0)),
                        Id::new(("outport", o)),
                        Sense::click(),
                    )
                    .on_hover_text("将选中的输入接到此声道");
                painter.circle_filled(point, 5.0, Self::channel_color(&self.graph.outputs[o].key));
                if response.clicked()
                    || (response.hovered() && ui.input(|i| i.pointer.any_released()))
                {
                    if let Some(i) = self.selected.take() {
                        self.graph.routes[i][o].on = !self.graph.routes[i][o].on;
                        self.dirty = true;
                        self.audio_sync_pending = true;
                    } else if response.clicked() {
                        self.notices
                            .push("先选择左侧输入声道，再点击右侧输出声道。");
                    }
                }
            }
        }
        if inputs.is_empty() && outputs.is_empty() {
            let center = rect.center();
            painter.text(
                center - vec2(0.0, 18.0),
                Align2::CENTER_CENTER,
                "从需要的通道开始",
                FontId::proportional(20.0),
                TEXT,
            );
            painter.text(
                center + vec2(0.0, 18.0),
                Align2::CENTER_CENTER,
                "添加虚拟总线或应用，或在「通道管理」中显示硬件通道",
                FontId::proportional(13.0),
                MUTED,
            );
        }
    }
    fn app_capture_state(&self, group: &str) -> Option<process_capture::CaptureState> {
        let engine = self.engine.as_ref()?;
        let path = group.strip_prefix("app:")?;
        let slot = self
            .graph
            .applications
            .iter()
            .position(|app| app.exe.to_lowercase() == path)?;
        engine.app_capture_state(slot)
    }

    fn app_capture_color(state: &process_capture::CaptureState) -> Color32 {
        use process_capture::CaptureState::*;
        match state {
            Capturing => COLORS[0],
            Connecting | Waiting => MUTED,
            Retrying(_) | Unavailable(_) => COLORS[3],
        }
    }

    fn channel_card(&mut self, ui: &mut Ui, r: Rect, input: bool, card: &ChannelCard) {
        if !ui.is_rect_visible(r.expand(10.0)) {
            return;
        }
        let painter = ui.painter().clone();
        let link_key = format!("{}:{}", input, card.group);
        let mut linked = !self.unlinked_stereo.contains(&link_key);
        // The companion may be hidden: linking still adjusts the complete stereo pair.
        let stereo = card.stereo;
        painter.rect_filled(r, 12.0, Color32::from_rgb(28, 33, 44));
        let all_muted = card.all_muted;
        let any_muted = card.any_muted;
        if icon_at(
            ui,
            Rect::from_min_size(pos2(r.right() - 66.0, r.top() + 6.0), vec2(24.0, 24.0)),
            Id::new(("card_mute", input, &card.group)),
            if all_muted { Icon::Muted } else { Icon::Volume },
            any_muted,
            if any_muted { COLORS[3] } else { MUTED },
            if all_muted {
                "整个通道组已静音 · 点击恢复全部声道（含隐藏声道）"
            } else if any_muted {
                "部分声道沿用旧配置静音 · 点击静音整个通道组，再点恢复全部；各声道音量不变"
            } else {
                "静音整个通道组（含隐藏声道），保留各声道音量"
            },
        )
        .clicked()
        {
            self.toggle_card_mute(input, &card.group);
        }
        if stereo {
            let response = icon_at(
                ui,
                Rect::from_min_size(pos2(r.right() - 96.0, r.top() + 6.0), vec2(24.0, 24.0)),
                Id::new(("stereo_link", input, &card.group)),
                if linked { Icon::Link } else { Icon::Unlink },
                linked,
                if linked { card.color } else { MUTED },
                if linked {
                    "L/R 音量已联动 · 点击独立调整"
                } else {
                    "L/R 音量独立 · 点击联动调整"
                },
            );
            if response.clicked() {
                linked = !linked;
                self.unlinked_stereo.retain(|key| key != &link_key);
                if !linked {
                    self.unlinked_stereo.push(link_key);
                }
                self.dirty = true;
            }
        }
        let mut menu_ui = ui.new_child(
            UiBuilder::new()
                .id_salt(("card_actions", input, &card.group))
                .max_rect(Rect::from_min_size(
                    pos2(r.right() - 36.0, r.top() + 6.0),
                    vec2(26.0, 24.0),
                )),
        );
        menu_ui.spacing_mut().interact_size = vec2(26.0, 24.0);
        menu_ui.spacing_mut().button_padding = vec2(5.0, 2.0);
        menu_ui.menu_button(RichText::new("···").size(18.0), |ui| {
            // egui resets popup padding to (2, 0); restore it inside the menu.
            ui.spacing_mut().button_padding = vec2(12.0, 6.0);
            ui.spacing_mut().interact_size.y = 32.0;
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(RichText::new(&card.title).size(12.0).color(MUTED));
            if ui.button("设置通道组备注…").clicked() {
                self.open_card_rename(input, &card.group);
                ui.close_menu();
            }
            if ui.button("移出面板（保留接线）").clicked() {
                self.card_action = Some(CardAction::Hide {
                    input,
                    group: card.group.clone(),
                });
                ui.close_menu();
            }
            if let Some(bus) = card
                .group
                .strip_prefix("virtual:")
                .and_then(|s| s.parse::<usize>().ok())
            {
                if ui
                    .button(RichText::new("删除整组虚拟总线").color(COLORS[4]))
                    .on_hover_text("移除这组输入、输出及其接线；保留其他总线。音频布局会自动更新。")
                    .clicked()
                {
                    self.card_action = Some(CardAction::RemoveBus(bus));
                    ui.close_menu();
                }
            } else if let Some(exe) = card.group.strip_prefix("app:") {
                if ui
                    .button(RichText::new("移除该应用输入").color(COLORS[4]))
                    .on_hover_text("移除应用 L/R 和相关接线，自动更新音频。")
                    .clicked()
                {
                    self.card_action = Some(CardAction::RemoveApp(exe.into()));
                    ui.close_menu();
                }
            }
        });
        painter.rect_filled(
            Rect::from_min_size(r.min + vec2(12.0, 13.0), vec2(3.0, 12.0)),
            2.0,
            card.color,
        );
        let mut title_rect = Rect::from_min_max(
            r.min + vec2(24.0, 5.0),
            pos2(
                r.right() - if stereo { 104.0 } else { 74.0 },
                r.top() + 32.0,
            ),
        );
        if input {
            if let Some(state) = self.app_capture_state(&card.group) {
                let color = Self::app_capture_color(&state);
                let badge =
                    painter.layout_no_wrap(state.label().into(), FontId::proportional(11.0), color);
                let badge_rect = Rect::from_min_max(
                    pos2(title_rect.right() - badge.size().x, title_rect.top()),
                    title_rect.right_bottom(),
                );
                title_rect.max.x = (badge_rect.left() - 6.0).max(title_rect.left());
                ui.interact(
                    badge_rect,
                    Id::new(("app_capture", &card.group)),
                    Sense::hover(),
                )
                .on_hover_text(format!(
                    "{}：{}\n{}",
                    card.title,
                    state.label(),
                    state.detail()
                ));
                painter.galley(
                    badge_rect.left_center() - vec2(0.0, badge.size().y * 0.5),
                    badge,
                    color,
                );
            }
        }
        if ui
            .interact(
                title_rect,
                Id::new(("device_title", input, &card.group)),
                Sense::click(),
            )
            .on_hover_text(format!(
                "{}\n点击设置通道组备注，显示为 原名 (备注)",
                card.title
            ))
            .clicked()
        {
            self.open_card_rename(input, &card.group);
        }
        painter.with_clip_rect(title_rect).text(
            title_rect.left_center(),
            Align2::LEFT_CENTER,
            &card.title,
            FontId::proportional(13.0),
            TEXT,
        );
        for (row, &index) in card.indices.iter().enumerate() {
            let node = if input {
                &self.graph.inputs[index]
            } else {
                &self.graph.outputs[index]
            };
            let key = node.key.clone();
            let name = self.graph.channel_name(input, index);
            let muted = node.mute;
            let fader = node.fader;
            let y = r.top() + 49.0 + row as f32 * 44.0;
            let row_rect = Rect::from_min_max(
                pos2(r.left() + 12.0, y - 14.0),
                pos2(r.right() - 12.0, y + 14.0),
            );
            let label = if self.graph.card_name(input, &card.group).is_some() {
                self.graph.channel_name(input, index)
            } else if key.starts_with("hardware:") && self.config.backend == 1 {
                node.device_name
                    .strip_prefix(&format!("{} · ", self.config.asio_name))
                    .unwrap_or(&node.device_name)
                    .to_string()
            } else if key.starts_with("hardware:") {
                let count = if input {
                    self.graph.counts().0
                } else {
                    self.graph.counts().1
                };
                let ch = key
                    .strip_prefix("hardware:")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                if count == 1 {
                    "MONO".into()
                } else if count == 2 {
                    if ch == 0 {
                        "L".into()
                    } else {
                        "R".into()
                    }
                } else {
                    format!("CH {}", ch + 1)
                }
            } else if key.rsplit(':').next() == Some("0") {
                "L".into()
            } else {
                "R".into()
            };
            let name_rect =
                Rect::from_min_max(row_rect.min, pos2(r.right() - 90.0, row_rect.bottom()));
            if let Some(base) = self.graph.card_name(input, &card.group) {
                let suffix = if !stereo {
                    "MONO"
                } else if key
                    .rsplit(':')
                    .next()
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0)
                    % 2
                    == 0
                {
                    "L"
                } else {
                    "R"
                };
                let suffix_width = if stereo { 20.0 } else { 40.0 };
                let base_rect =
                    Rect::from_min_max(name_rect.min, name_rect.max - vec2(suffix_width, 0.0));
                painter.with_clip_rect(base_rect).text(
                    base_rect.left_center(),
                    Align2::LEFT_CENTER,
                    base,
                    FontId::proportional(12.0),
                    if muted { MUTED } else { card.color },
                );
                painter.with_clip_rect(name_rect).text(
                    name_rect.right_center(),
                    Align2::RIGHT_CENTER,
                    suffix,
                    FontId::proportional(11.0),
                    if muted { MUTED } else { card.color },
                );
            } else {
                painter.with_clip_rect(name_rect).text(
                    name_rect.left_center(),
                    Align2::LEFT_CENTER,
                    &label,
                    FontId::proportional(12.0),
                    if muted { MUTED } else { card.color },
                );
            }
            if ui
                .interact(
                    name_rect,
                    Id::new(("channel_name", input, index)),
                    Sense::click(),
                )
                .on_hover_text(format!(
                    "{}\n原始通道：{}\n点击调整声道；备注请点击通道组标题。",
                    name, node.device_name
                ))
                .clicked()
            {
                self.rename = Some((input, index));
            }
            let level = self
                .levels
                .get(if input {
                    index
                } else {
                    self.graph.inputs.len() + index
                })
                .copied()
                .unwrap_or(0.0);
            let gain_rect = Rect::from_min_size(pos2(r.right() - 84.0, y - 13.0), vec2(72.0, 26.0));
            painter.text(
                gain_rect.center(),
                Align2::CENTER_CENTER,
                bridge::fader_label(fader as f64),
                FontId::proportional(10.0),
                MUTED,
            );
            let mut value = fader;
            let fader_rect = Rect::from_min_max(
                pos2(r.left() + 12.0, y + 11.0),
                pos2(r.right() - 12.0, y + 31.0),
            );
            let response = widgets::metered_fader(
                ui,
                fader_rect,
                Id::new(("channel_fader", input, index)),
                &mut value,
                level,
                card.color,
                muted,
                &label,
            );
            if response.changed() {
                self.set_channel_fader(input, index, value);
            }
        }
    }
    fn matrix(&mut self, ui: &mut Ui, height: f32) {
        let inputs = self
            .graph
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                n.visible && (!n.key.starts_with("hardware:") || self.hardware_available)
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let outputs = self
            .graph
            .outputs
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                n.visible && (!n.key.starts_with("hardware:") || self.hardware_available)
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let panel_width = ui.available_width();
        Frame::new()
            .fill(PANEL)
            .corner_radius(14.0)
            .inner_margin(18.0)
            .show(ui, |ui| {
                ui.set_min_size(vec2(
                    (panel_width - 36.0).max(0.0),
                    (height - 36.0).max(0.0),
                ));
                if inputs.is_empty() || outputs.is_empty() {
                    small(ui, "请先添加通道，或在通道管理中选择显示的输入和输出。");
                    return;
                }
                // Fixed square cells prevent both long labels and sparse layouts stretching columns.
                const CELL: f32 = 34.0;
                const LABEL: f32 = 184.0;
                const HEADER: f32 = 100.0;
                let size = vec2(
                    LABEL + CELL * outputs.len() as f32,
                    HEADER + CELL * inputs.len() as f32,
                );
                small(
                    ui,
                    "输入 ↓ / 输出 →    点击切换接线 · 右键或双击调整连接 · 悬停查看完整名称",
                );
                ScrollArea::horizontal()
                    .id_salt("compact_matrix_scroll")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                        let p = ui.painter_at(rect);
                        let single_line = |name: &str, font: FontId, color: Color32, width: f32| {
                            let mut job = text::LayoutJob::simple(
                                name.replace(['\r', '\n'], " "),
                                font,
                                color,
                                width,
                            );
                            job.wrap.max_rows = 1;
                            job.wrap.break_anywhere = true;
                            p.layout_job(job)
                        };
                        for (column, &o) in outputs.iter().enumerate() {
                            let cell = Rect::from_min_size(
                                rect.min + vec2(LABEL + column as f32 * CELL, 0.0),
                                vec2(CELL, HEADER),
                            );
                            let name = self.graph.channel_name(false, o);
                            let color = Self::channel_color(&self.graph.outputs[o].key);
                            let galley = single_line(
                                &name,
                                FontId::proportional(11.0),
                                color,
                                HEADER - 16.0,
                            );
                            let text_height = galley.size().y;
                            p.with_clip_rect(cell.shrink(5.0)).add(
                                epaint::TextShape::new(
                                    pos2(cell.center().x - text_height * 0.5, cell.bottom() - 8.0),
                                    galley,
                                    color,
                                )
                                .with_angle(-std::f32::consts::FRAC_PI_2),
                            );
                            ui.interact(cell, Id::new(("matrix_output", o)), Sense::hover())
                                .on_hover_text(name);
                        }
                        for (row, &i) in inputs.iter().enumerate() {
                            let y = rect.top() + HEADER + row as f32 * CELL;
                            let label =
                                Rect::from_min_size(pos2(rect.left(), y), vec2(LABEL, CELL));
                            if !ui.is_rect_visible(Rect::from_min_size(
                                pos2(rect.left(), y),
                                vec2(size.x, CELL),
                            )) {
                                continue;
                            }
                            let name = self.graph.channel_name(true, i);
                            let capture = self.graph.inputs[i]
                                .key
                                .rsplit_once(':')
                                .and_then(|(group, _)| self.app_capture_state(group));
                            let name_width = LABEL - if capture.is_some() { 30.0 } else { 16.0 };
                            let galley =
                                single_line(&name, FontId::proportional(12.0), TEXT, name_width);
                            let text_height = galley.size().y;
                            p.with_clip_rect(label.shrink(4.0)).galley(
                                label.min + vec2(8.0, (CELL - text_height) * 0.5),
                                galley,
                                TEXT,
                            );
                            let response =
                                ui.interact(label, Id::new(("matrix_input", i)), Sense::hover());
                            if let Some(state) = capture {
                                p.circle_filled(
                                    pos2(label.right() - 10.0, label.center().y),
                                    3.0,
                                    Self::app_capture_color(&state),
                                );
                                response.on_hover_text(format!(
                                    "{name}\n{}：{}",
                                    state.label(),
                                    state.detail()
                                ));
                            } else {
                                response.on_hover_text(&name);
                            }
                            for (column, &o) in outputs.iter().enumerate() {
                                let cell = Rect::from_min_size(
                                    pos2(rect.left() + LABEL + column as f32 * CELL, y),
                                    vec2(CELL, CELL),
                                );
                                if !ui.is_rect_visible(cell) {
                                    continue;
                                }
                                let route = self.graph.routes[i][o];
                                let response = ui.interact(
                                    cell,
                                    Id::new(("matrix_route", i, o)),
                                    Sense::click(),
                                );
                                let color = Self::channel_color(&self.graph.outputs[o].key);
                                p.rect_filled(
                                    cell.shrink(2.0),
                                    5.0,
                                    if response.hovered() {
                                        Color32::from_rgb(48, 54, 70)
                                    } else if route.on {
                                        color.gamma_multiply(0.20)
                                    } else {
                                        CARD
                                    },
                                );
                                if route.on {
                                    let muted = route.mute
                                        || self.graph.inputs[i].mute
                                        || self.graph.outputs[o].mute
                                        || self.panic;
                                    p.circle_filled(
                                        cell.center(),
                                        4.5,
                                        if muted { MUTED } else { color },
                                    );
                                } else if response.hovered() {
                                    p.text(
                                        cell.center(),
                                        Align2::CENTER_CENTER,
                                        "+",
                                        FontId::proportional(16.0),
                                        TEXT,
                                    );
                                }
                                let response = response.on_hover_text(format!(
                                    "{} → {}\n{} · 右键或双击调整",
                                    name,
                                    self.graph.channel_name(false, o),
                                    if route.on { "已连接" } else { "未连接" }
                                ));
                                if response.secondary_clicked() || response.double_clicked() {
                                    self.route = Some((i, o));
                                } else if response.clicked() {
                                    self.graph.routes[i][o].on = !route.on;
                                    self.dirty = true;
                                    self.audio_sync_pending = true;
                                }
                            }
                        }
                    });
            });
    }
    fn rack(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            small(
                ui,
                &format!("{} 组虚拟总线 · 一个 DAW ASIO 入口", self.graph.bus_count),
            );
            if self.graph.bus_count > 0 {
                small(ui, &self.system_status);
            }
            let hidden = self
                .graph
                .inputs
                .iter()
                .chain(&self.graph.outputs)
                .filter(|n| !n.visible)
                .count();
            if hidden > 0 {
                small(ui, &format!("{} 个通道已隐藏 · 接线仍保留", hidden));
            }
        });
    }
    fn channel_manager_window(&mut self, ctx: &Context) {
        if !self.channel_manager {
            return;
        }
        let mut open = true;
        ui_shell::dialog("通道管理").open(&mut open).default_size([680.0,520.0]).collapsible(false).show(ctx,|ui| {
            ui_shell::intro(ui,"自定义通道","只显示需要的通道，让工作台保持清晰。");
            ui.horizontal(|ui| {for (index,title) in ["虚拟总线","硬件输入","硬件输出","系统声音"].iter().enumerate() {
                if button(ui,*title,self.channel_tab==index).clicked() {self.channel_tab=index;}
            }});
            ui.add_space(8.0);small(ui,"勾选表示在跳线面板和矩阵中显示；取消勾选不会删除接线或停止音频。");ui.add_space(8.0);
            if self.channel_tab==0 {
                ui.horizontal(|ui| {if button(ui,"＋ 添加虚拟总线",true).clicked() {self.add_virtual_bus();}small(ui,"按需添加，不设固定组数上限");});
                ScrollArea::vertical().id_salt("virtual_bus_manager").max_height((ctx.screen_rect().height()-310.0).clamp(140.0,350.0)).show(ui,|ui| {
                    for bus in 0..self.graph.bus_count {
                        ui_shell::section().show(ui,|ui| {
                            ui.horizontal(|ui| {ui.label(RichText::new(format!("虚拟总线 {}", bus + 1)).color(COLORS[(bus+1)%COLORS.len()]).strong());
                                let mut visible=self.graph.inputs.iter().chain(&self.graph.outputs).any(|n|n.key.starts_with(&format!("virtual:{bus}:"))&&n.visible);
                                if button(ui,RichText::new("删除总线").color(COLORS[4]),false).on_hover_text("删除该组输入／输出及接线；至少保留一组兼容 DAW，最后一组可隐藏。").clicked() {self.card_action=Some(CardAction::RemoveBus(bus));}
                                if ui.checkbox(&mut visible,"在面板中显示").changed() {for node in self.graph.inputs.iter_mut().chain(&mut self.graph.outputs) {if node.key.starts_with(&format!("virtual:{bus}:")) {node.visible=visible;}}self.dirty=true;}
                            });
                            small(ui,"Windows 播放与 DAW 输出 → 同一输入通道组；输出通道组 → Windows 录音与 DAW 输入");
                            for input in [true, false] {
                                ui.horizontal(|ui| {
                                    ui.label(format!("{}：{}", if input { "输入通道组" } else { "输出通道组" }, self.graph.virtual_name(input, bus)));
                                    if button(ui, "设置备注…", false).clicked() {
                                        let group = format!("virtual:{bus}");
                                        self.card_rename = Some(CardRename { input, draft: self.graph.card_name(input, &group).unwrap_or("").to_owned(), group, focus: true });
                                    }
                                });
                            }
                            ui.horizontal(|ui| {for input in [true,false] {let nodes=if input {&mut self.graph.inputs} else {&mut self.graph.outputs};
                                for ch in 0..2 {if let Some(node)=nodes.iter_mut().find(|n|n.key==audio::virtual_key(bus,ch)) {
                                    if ui.checkbox(&mut node.visible,format!("{} {}",if input {"输入"} else {"输出"},if ch==0 {"L"} else {"R"})).changed() {self.dirty=true;}
                                }}
                            }});
                        });ui.add_space(4.0);
                    }
                });
                if self.graph.bus_count==0 {ui.add_space(15.0);ui.label("还没有虚拟总线，需要时再添加第一组。");}
            } else {
                let input=self.channel_tab!=2;let system=self.channel_tab==3;
                if !system && !self.hardware_available {ui.label("暂时无法读取所选声卡，请检查设备连接并在设置里刷新。");small(ui,&self.status);return;}
                if system && !self.config.loopback {small(ui,"系统回录尚未启用，可在声卡设置中启用；显示通道组不会自动启用回录。");}
                let display_names: std::collections::HashMap<String, String> = (if input {&self.graph.inputs} else {&self.graph.outputs}).iter().enumerate().map(|(i,n)|(n.key.clone(),self.graph.channel_name(input,i))).collect();
                let nodes=if input {&mut self.graph.inputs} else {&mut self.graph.outputs};
                let prefix=if system {"system:"} else {"hardware:"};
                ui.horizontal(|ui| {for (label,value) in [("全部显示",true),("全部隐藏",false)] {if ui.button(label).clicked() {for node in nodes.iter_mut().filter(|n|n.key.starts_with(prefix)) {node.visible=value;}self.dirty=true;}}});
                ScrollArea::vertical().id_salt("physical_channel_manager").max_height((ctx.screen_rect().height()-310.0).clamp(140.0,355.0)).show(ui,|ui| {
                    let mut count=0;
                    for node in nodes.iter_mut().filter(|n|n.key.starts_with(prefix)) {count+=1;
                        let label=display_names.get(&node.key).unwrap_or(&node.device_name);
                        ui_shell::section().show(ui,|ui| {ui.set_min_width(ui.available_width());if ui.checkbox(&mut node.visible,label).on_hover_text(format!("原始通道：{}",node.device_name)).changed() {self.dirty=true;}});
                        ui.add_space(4.0);
                    }
                    if count==0 {ui.label("没有可用通道，请在设置中选择已连接的音频设备并刷新。");}
                });
            }
        });
        self.channel_manager = open;
    }
    fn settings_window(&mut self, ctx: &Context) {
        let previous_config = self.config.clone();
        let mut open = self.settings;
        ui_shell::dialog("工作台设置")
            .open(&mut open)
            .default_width(660.0)
            .min_width(540.0)
            .collapsible(false)
            .show(ctx, |ui| {
                ui_shell::intro(ui, "音频工作台", "选择音频后端、管理虚拟设备及 DAW 接入。");
                ui.horizontal(|ui| {
                    for (i, t) in ["声卡与音频", "虚拟设备与 DAW", "界面与性能", "Agent / MCP"].iter().enumerate() {
                        if button(ui, *t, self.settings_tab == i).clicked() {
                            self.settings_tab = i;
                        }
                    }
                });
                ui.add_space(12.0);
                ScrollArea::vertical()
                    .id_salt("settings_content")
                    .max_height((ctx.screen_rect().height() - 235.0).max(180.0))
                    .show(ui, |ui| {
                        ui_shell::section().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                if self.settings_tab == 3 {
                    self.mcp_panel(ui);
                } else if self.settings_tab == 2 {
                    ui.label(RichText::new("连线动效").size(15.0).strong());
                    small(ui, "动效只用于指示信号流向，关闭后音频和电平表仍正常工作。");
                    ui.add_space(8.0);
                    if ui.checkbox(&mut self.motion_enabled, "显示信号流动光束").changed() { self.dirty = true; }
                    if ui.checkbox(&mut self.low_power_animation, "使用低开销动效（约 30 帧／秒）").changed() { self.dirty = true; }
                    ui.add_space(14.0);
                    small(ui, "窗口失去焦点时自动降低动效刷新频率；最小化后停止绘制面板，音频引擎继续工作。");
                    small(ui, "复杂工程或直播时可关闭动效。实际 CPU 占用和音频稳定性取决于声卡、缓冲及接线数量。");
                    ui.add_space(14.0);
                    if button(ui,"保存偏好",true).clicked() { self.save(); self.status = "界面偏好已保存".into(); }
                } else if self.settings_tab == 1 {
                    self.windows_panel(ui);
                            } else if self.settings_tab == 0 {
                                if self.engine.is_some() {
                                    ui.label(
                                        RichText::new("更改设备设置会自动重启音频，声音将短暂中断。")
                                            .color(COLORS[3]),
                                    );
                                }
                                ui.add_enabled_ui(true, |ui| {
                                    ui.add_space(8.0);
                                    ui.horizontal(|ui| {
                                        ui.selectable_value(
                                            &mut self.config.backend,
                                            0,
                                            "Windows 音频 · 板载 / USB",
                                        );
                                        ui.selectable_value(
                                            &mut self.config.backend,
                                            1,
                                            "原生 ASIO · 专业声卡",
                                        );
                                    });
                                    ui.add_space(12.0);
                                    Grid::new("audio_settings")
                            .num_columns(2)
                            .spacing(vec2(20.0, 15.0))
                            .show(ui, |ui| {
                                if self.config.backend == 1 {
                                    ui.label("ASIO 声卡");
                                    ComboBox::from_id_salt("asio_device")
                                        .width(340.0)
                                        .selected_text(if self.config.asio_name.is_empty() {
                                            "选择 ASIO 驱动"
                                        } else {
                                            &self.config.asio_name
                                        })
                                        .show_ui(ui, |ui| {
                                            for d in &self.asios {
                                                if d.clsid != bridge::UNIFIED_GUID
                                                    && !d.name.starts_with("Patchbay ASIO")
                                                {
                                                    ui.selectable_value(
                                                        &mut self.config.asio_name,
                                                        d.name.clone(),
                                                        &d.name,
                                                    );
                                                }
                                            }
                                        });
                                    ui.end_row();
                                    ui.label("通道");
                                    ui.label("启动后读取全部硬件输入／输出，逐通道转发给 DAW");
                                    ui.end_row();
                                } else {
                                    ui.label("输入设备");
                                    ComboBox::from_id_salt("input_device")
                                        .width(340.0)
                                        .selected_text(if self.config.input.is_empty() {
                                            "默认输入设备"
                                        } else {
                                            &self.config.input
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut self.config.input,
                                                String::new(),
                                                "默认输入设备",
                                            );
                                            ui.selectable_value(
                                                &mut self.config.input,
                                                "（不使用输入）".into(),
                                                "（不使用输入）",
                                            );
                                            for d in self
                                                .inputs
                                                .iter()
                                                .filter(|d| !usb_audio::is_virtual(d))
                                            {
                                                ui.selectable_value(
                                                    &mut self.config.input,
                                                    d.clone(),
                                                    d,
                                                );
                                            }
                                        });
                                    ui.end_row();
                                    ui.label("输出设备");
                                    ComboBox::from_id_salt("output_device")
                                        .width(340.0)
                                        .selected_text(if self.config.output.is_empty() {
                                            "默认输出设备"
                                        } else {
                                            &self.config.output
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut self.config.output,
                                                String::new(),
                                                "默认输出设备",
                                            );
                                            for d in self
                                                .outputs
                                                .iter()
                                                .filter(|d| !usb_audio::is_virtual(d))
                                            {
                                                ui.selectable_value(
                                                    &mut self.config.output,
                                                    d.clone(),
                                                    d,
                                                );
                                            }
                                        });
                                    ui.end_row();
                                }
                                ui.label("采样率");
                                ComboBox::from_id_salt("rate")
                                    .selected_text(format!("{} Hz", self.config.rate))
                                    .show_ui(ui, |ui| {
                                        for r in [44100, 48000, 96000] {
                                            ui.selectable_value(
                                                &mut self.config.rate,
                                                r,
                                                format!("{r} Hz"),
                                            );
                                        }
                                    });
                                ui.end_row();
                                if self.config.backend == 1 {
                                    ui.label("缓冲区");
                                    ComboBox::from_id_salt("buffer")
                                        .selected_text(format!("{} 帧", self.config.buffer))
                                        .show_ui(ui, |ui| {
                                            for b in [64, 128, 256, 512, 1024, 2048] {
                                                ui.selectable_value(
                                                    &mut self.config.buffer,
                                                    b,
                                                    format!("{b} 帧"),
                                                );
                                            }
                                        });
                                    ui.end_row();
                                }
                            });
                                    if self.config.backend == 0 {
                                        ui.add_space(8.0);
                                        ui.checkbox(
                                            &mut self.config.loopback,
                                            "启用系统声音回录（包含所选输出设备的全部声音）",
                                        );
                                        small(
                                ui,
                                "系统回录接回同一监听输出可能产生回声，请仅连接到虚拟直播总线。",
                            );
                                    } else {
                                        self.config.loopback = false;
                                        small(
                                ui,
                                "ASIO 模式按所选声卡的通道与缓冲区能力启动，设备不匹配会显示错误。",
                            );
                                    }
                                });
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    if button(ui, "刷新设备", false).clicked() {
                                        self.refresh();
                                    }
                                    if self.config.backend == 1
                                        && ui.add(Button::new("声卡控制面板")).clicked()
                                    {
                                        if self.engine.is_some() || self.engine_busy || self.driver_tools.busy() {
                                            self.notices
                                                .push("请先停止音频引擎，再打开声卡控制面板。");
                                        } else if let Some(d) = self
                                            .asios
                                            .iter()
                                            .find(|d| d.name == self.config.asio_name)
                                        {
                                            match AsioPanel::open(d) {
                                                Ok(()) => (),
                                                Err(e) => self.status = e,
                                            }
                                        } else {
                                            self.notices.push("请先选择一个 ASIO 声卡驱动。");
                                        }
                                    }
                                    if button(ui, "保存设置", true).clicked() {
                                        self.dirty = true;
                                        self.audio_sync_pending = true;
                                        self.save();
                                        if !self.dirty {
                                            self.status = "设置已保存，下次启动引擎生效".into();
                                        }
                                    }
                                });
                            }
                        });
                    });
            });
        self.settings = open;
        if self.config != previous_config {
            self.dirty = true;
            self.queue_restart();
        }
    }
    fn windows_panel(&mut self, ui: &mut Ui) {
        ui.add_space(8.0);
        ui.label(RichText::new("DAW：一个统一 ASIO 设备").strong());
        ui.label(if self.unified_asio {
            "Soon Link Pro ASIO 已注册 · 硬件通道 + 按需添加的虚拟通道"
        } else {
            "Soon Link Pro ASIO 未注册 · 注册后可在 64 位 DAW 中选择"
        });
        small(ui,"ASIO 前面是物理通道，后面按创建数量追加虚拟通道。Windows 与 DAW 共用虚拟总线，每个通道组内的 L/R 可独立接线。");
        ui.horizontal(|ui| {
            if button(ui, "注册统一 ASIO…", true).clicked() {
                if self.engine.is_some() || self.engine_busy || self.driver_tools.busy() {
                    self.notices
                        .push("请停止引擎并关闭 DAW，再注册 ASIO 设备。");
                } else {
                    self.register_asio(false);
                }
            }
            if button(ui, "卸载统一 ASIO…", false).clicked() {
                if self.engine.is_some() || self.engine_busy || self.driver_tools.busy() {
                    self.notices
                        .push("请停止引擎并关闭 DAW，再卸载 ASIO 设备。");
                } else {
                    self.register_asio(true);
                }
            }
        });
        let (ni, no) = self.graph.counts();
        let virtual_channels = self.graph.bus_count * 2;
        if virtual_channels == 0 {
            ui.label(format!(
                "当前 DAW：{} 个物理输入 / {} 个物理输出；尚未添加虚拟通道。",
                ni, no
            ));
        } else {
            ui.label(format!(
                "当前 DAW：{} 输入 / {} 输出。虚拟输入 {}–{}，虚拟输出 {}–{}。",
                ni + virtual_channels,
                no + virtual_channels,
                ni + 1,
                ni + virtual_channels,
                no + 1,
                no + virtual_channels
            ));
        }
        small(ui,"物理 ASIO 输入／输出直接转发真实声卡；硬件通道组的音量、静音以及全部静音同样作用于直通。");
        small(
            ui,
            "先关闭 DAW 再注册。注册会将本软件旧的 Patchbay ASIO 迁移为新名称，需管理员确认。",
        );
        ui.separator();
        ui.add_space(8.0);
        ui.label(RichText::new("Windows 播放／录音设备").strong());
        small(
            ui,
            "通道组备注仅保存在本软件；Windows 设备名与 DAW 通道名不会随备注改变。",
        );
        ui.add_enabled_ui(true, |ui| {
            if ui
                .checkbox(
                    &mut self.config.virtual_devices,
                    "启动引擎时连接已添加的虚拟设备",
                )
                .changed()
            {
                self.dirty = true;
                self.audio_sync_pending = true;
            }
        });
        ui.label(&self.system_status);
        small(
            ui,
            "每组包含一个立体声播放设备和一个立体声录音设备。停止引擎后设备断开。",
        );
        small(
            ui,
            "声卡只选择一个 ASIO 或 Windows 后端；OBS、通话软件直接选择 Windows 录音设备。",
        );
        ui.add_space(10.0);
        ScrollArea::vertical()
            .id_salt("endpoint_status_scroll")
            .max_height(190.0)
            .show(ui, |ui| {
                Grid::new("system_endpoints")
                    .num_columns(3)
                    .spacing(vec2(18.0, 13.0))
                    .show(ui, |ui| {
                        ui.label("设备组");
                        ui.label("播放 → 混音器输入");
                        ui.label("混音器输出 → 录音");
                        ui.end_row();
                        for bus in 0..self.graph.bus_count {
                            ui.label(
                                RichText::new(format!("虚拟总线 {}", bus + 1))
                                    .color(COLORS[(bus + 1) % COLORS.len()]),
                            );
                            ui.vertical(|ui| {
                                ui.label(self.graph.virtual_name(true, bus));
                                ui.label(if usb_audio::has_endpoint(&self.outputs, bus) {
                                    "Windows 已检测到"
                                } else {
                                    "未检测到"
                                });
                            });
                            ui.vertical(|ui| {
                                ui.label(self.graph.virtual_name(false, bus));
                                ui.label(if usb_audio::has_endpoint(&self.inputs, bus) {
                                    "Windows 已检测到"
                                } else {
                                    "未检测到"
                                });
                            });
                            ui.end_row();
                        }
                    });
            });
        ui.add_space(12.0);
        ui.label("使用 Windows 虚拟设备需要 usbip-win2，已安装无需重复安装。仅使用物理 ASIO 无需此组件。");
        ui.add_space(8.0);
        ui_shell::section().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("Windows 虚拟设备驱动").strong());
            small(
                ui,
                &format!(
                    "随包 usbip-win2 {} · 原厂安装器；安装时自动暂停音频，结束后恢复。",
                    driver_tools::BUNDLED_VERSION
                ),
            );
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled_ui(!self.driver_tools.busy(), |ui| {
                        button(ui, "安装驱动…", false)
                    })
                    .inner
                    .clicked()
                {
                    self.request_driver_install();
                }
                if ui
                    .add_enabled_ui(self.driver_tools.update.is_none(), |ui| {
                        button(
                            ui,
                            if self.driver_tools.update.is_some() {
                                "检查中…"
                            } else {
                                "检查驱动更新"
                            },
                            false,
                        )
                    })
                    .inner
                    .clicked()
                {
                    self.driver_tools.update = Some(driver_tools::check_updates());
                    self.driver_tools.update_status = "正在读取官方稳定版信息…".into();
                }
                ui.hyperlink_to(
                    "官方更新页面",
                    self.driver_tools
                        .release
                        .as_ref()
                        .map(|r| r.url.as_str())
                        .unwrap_or(driver_tools::RELEASES_URL),
                );
            });
            if self.driver_tools.busy() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("等待驱动安装完成…");
                });
            }
            if !self.driver_tools.update_status.is_empty() {
                ui.label(RichText::new(&self.driver_tools.update_status).size(12.0));
            }
        });
        ui.horizontal(|ui| {
            if button(ui, "刷新设备", false).clicked() {
                self.refresh();
            }
            if button(ui, "Windows 声音设置", false).clicked() {
                let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
                unsafe {
                    let result = windows_sys::Win32::UI::Shell::ShellExecuteW(
                        std::ptr::null_mut(),
                        wide("open").as_ptr(),
                        wide("ms-settings:sound").as_ptr(),
                        std::ptr::null(),
                        std::ptr::null(),
                        1,
                    );
                    if result as isize <= 32 {
                        self.status = "无法打开 Windows 声音设置".into();
                    }
                }
            }
        });
    }
    fn request_driver_install(&mut self) {
        if !driver_tools::bundled_path().is_file() {
            self.notices
                .push("未找到随包驱动安装器，请完整解压软件包后再试。");
            return;
        }
        self.driver_tools.resume_audio = self.engine_wanted;
        self.driver_tools.pending_install = true;
        self.engine_wanted = false;
        self.engine = None;
        self.engine_busy = true;
        self.restart_at = None;
        self.engine_revision = self.engine_revision.wrapping_add(1);
        self.engine_controller.stop(self.engine_revision);
        self.status = "正在暂停音频，以打开驱动安装器…".into();
    }
    fn poll_driver_tools(&mut self) {
        let install = self
            .driver_tools
            .install
            .as_ref()
            .and_then(|r| match r.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("驱动安装任务意外结束".into()))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = install {
            self.driver_tools.install = None;
            let message = result.unwrap_or_else(|error| error);
            self.queue_device_refresh();
            if self.driver_tools.resume_audio {
                self.engine_wanted = true;
                self.queue_restart();
            }
            self.driver_tools.resume_audio = false;
            self.status = message;
        }
        let update = self
            .driver_tools
            .update
            .as_ref()
            .and_then(|r| match r.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("驱动更新检查意外结束，请重试".into()))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = update {
            self.driver_tools.update = None;
            match result {
                Ok(release) => {
                    self.driver_tools.update_status = release.summary();
                    self.driver_tools.release = Some(release);
                }
                Err(error) => self.driver_tools.update_status = error,
            }
            self.notices.push(self.driver_tools.update_status.clone());
        }
    }
    fn register_asio(&mut self, remove: bool) {
        let dll = std::env::current_exe()
            .unwrap_or_default()
            .with_file_name("patchbay_asio.dll");
        if !dll.is_file() {
            self.status = "未找到 patchbay_asio.dll，请使用完整测试包".into();
            return;
        }
        let params = format!("{}\"{}\"", if remove { "/u " } else { "" }, dll.display());
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        unsafe {
            let result = windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                wide("runas").as_ptr(),
                wide("C:\\Windows\\System32\\regsvr32.exe").as_ptr(),
                wide(&params).as_ptr(),
                std::ptr::null(),
                1,
            );
            self.status = if result as isize > 32 {
                "已请求注册操作，完成后请刷新设备并重新打开 DAW".into()
            } else {
                "ASIO 注册操作未完成或已取消".into()
            };
        }
    }
    fn refresh_app_candidates(&mut self) {
        if self.app_scan.is_some() {
            return;
        }
        let (send, receive) = std::sync::mpsc::channel();
        self.app_scan = Some(receive);
        self.last_app_scan = Instant::now();
        std::thread::spawn(move || {
            let _ = send.send(process_capture::applications());
        });
    }
    fn application_picker(&mut self, ctx: &Context) {
        if !self.app_picker {
            return;
        }
        if !ctx.input(|input| input.pointer.any_down()) {
            let result = self
                .app_scan
                .as_ref()
                .and_then(|receiver| match receiver.try_recv() {
                    Ok(inventory) => Some(Some(inventory)),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(None),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                });
            if let Some(result) = result {
                self.app_scan = None;
                if let Some(inventory) = result {
                    self.app_candidates = inventory.targets;
                    self.app_audio_status = inventory.audio_status;
                } else {
                    self.app_audio_status = "应用扫描已结束，点击刷新重试。".into();
                }
            }
        }
        if self.app_scan.is_none() && self.last_app_scan.elapsed() >= Duration::from_secs(2) {
            self.refresh_app_candidates();
        }
        let mut open = true;
        let mut chosen = None;
        let mut blocked = None;
        widgets::application_picker_window(ctx)
            .open(&mut open)
            .show(ctx, |ui| {
                ui_shell::intro(ui, "应用声音", "将正在运行的应用添加为独立的立体声来源。");
                if widgets::search_with_refresh(ui, &mut self.app_filter, "搜索中文应用名或进程路径", self.app_scan.is_some()) {
                    self.refresh_app_candidates();
                }
                if self.engine.is_some() {
                    ui.label(
                        RichText::new("添加或移除应用会自动更新音频布局，声音将短暂中断。")
                            .color(COLORS[3]),
                    );
                }
                small(ui, "选择正在运行的应用；L/R 放在同一个通道组中，可分别接线。");
                ui.add(Label::new(RichText::new(&self.app_audio_status).size(11.0).color(MUTED)).truncate()).on_hover_text(&self.app_audio_status);
                ui.separator();
                let filter = self.app_filter.to_lowercase();
                ScrollArea::vertical().max_height(ui.available_height().min(330.0)).show(ui, |ui| {
                    for target in &self.app_candidates {
                        if !target.exe.to_lowercase().contains(&filter) && !target.label().to_lowercase().contains(&filter) {
                            continue;
                        }
                        let added = self
                            .graph
                            .applications
                            .iter()
                            .any(|a| a.exe.eq_ignore_ascii_case(&target.exe));
                        let state = if target.speaking { "   ● 正在发声" } else if target.audio_active { "   音频会话" } else { "" };
                        let label = format!("{}{}{}", target.label(), state, if added { "   已添加" } else { "" });
                        if widgets::sized_text_button(
                            ui,
                            label,
                            added || target.speaking,
                            if target.speaking { COLORS[0] } else { COLORS[1] },
                            vec2(ui.available_width(), 48.0),
                        )
                        .on_hover_text(format!("{}\n进程：{} · PID {}\n{}", target.label(), target.exe, target.pid, if target.speaking { "Windows 音频会话检测到非零信号" } else if target.audio_active { "音频会话活动，但本次未检测到信号" } else { "本次未检测到活动音频会话；独占 ASIO 等声音可能无法由 Windows 会话检测" }))
                        .clicked()
                        {
                            if added {
                                blocked = Some("这个应用已经添加，可在输入来源中找到它。");
                            } else {
                                chosen = Some(target.clone());
                            }
                        }
                    }
                });
            });
        if let Some(message) = blocked {
            self.notices.push(message);
        }
        self.app_picker = open;
        if let Some(target) = chosen {
            match self.graph.add_application(target) {
                Ok(()) => {
                    self.dirty = true;
                    self.audio_sync_pending = true;
                    self.selected = Some(self.graph.inputs.len() - 1);
                    self.queue_restart();
                    self.status = "应用输入已添加，音频布局正在自动更新".into();
                    self.app_picker = false;
                }
                Err(e) => self.status = e,
            }
        }
    }
    fn dialogs(&mut self, ctx: &Context) {
        if let Some((i, o)) = self.route {
            let mut open = true;
            let mut removed = false;
            ui_shell::dialog("连接控制")
                .open(&mut open)
                .default_width(360.0)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{}  →  {}",
                            self.graph.channel_name(true, i),
                            self.graph.channel_name(false, o)
                        ))
                        .color(COLORS[o % COLORS.len()]),
                    );
                    small(ui, "调整这条连接的音量，不改变声道本身的音量。");
                    ui.add_space(10.0);
                    ui.spacing_mut().slider_width = 220.0;
                    let r = &mut self.graph.routes[i][o];
                    ui.add_space(10.0);
                    if ui.checkbox(&mut r.on, "启用连接").changed() {
                        self.dirty = true;
                        self.audio_sync_pending = true;
                    }
                    if ui.checkbox(&mut r.mute, "静音此连接").changed() {
                        self.dirty = true;
                        self.audio_sync_pending = true;
                    }
                    if ui
                        .add(
                            Slider::new(&mut r.fader, 0.0..=1.0)
                                .text("音量")
                                .custom_formatter(|n, _| bridge::fader_label(n)),
                        )
                        .changed()
                    {
                        self.dirty = true;
                        self.audio_sync_pending = true;
                    }
                    ui.add_space(8.0);
                    if button(ui, RichText::new("移除连接").color(COLORS[4]), false).clicked() {
                        r.on = false;
                        self.dirty = true;
                        self.audio_sync_pending = true;
                        removed = true;
                    }
                });
            if !open || removed {
                self.route = None;
            }
        }
        if let Some((input, i)) = self.rename {
            let mut open = true;
            if let Some(node) = (if input {
                self.graph.inputs.get(i)
            } else {
                self.graph.outputs.get(i)
            })
            .cloned()
            {
                ui_shell::dialog("声道控制")
                    .open(&mut open)
                    .default_width(390.0)
                    .collapsible(false)
                    .show(ctx, |ui| {
                        ui.label(RichText::new(self.graph.channel_name(input, i)).strong());
                        small(ui, &format!("原始通道：{}", node.device_name));
                        if button(ui, "设置通道组备注…", false).clicked() {
                            self.open_card_rename(input, &Graph::card_group(&node.key));
                        }
                        ui.add_space(8.0);
                        let mut value = node.fader;
                        if ui
                            .add(
                                Slider::new(&mut value, 0.0..=1.0)
                                    .text("音量")
                                    .custom_formatter(|n, _| bridge::fader_label(n)),
                            )
                            .changed()
                        {
                            self.set_channel_fader(input, i, value);
                        }
                        small(ui, "静音请用通道组标题栏的扬声器图标，一次控制整个通道组。");
                        small(ui, "需要独立调整时，解除 L/R 联动后拖动各声道电平条。");
                    });
            } else {
                open = false;
            }
            if !open {
                self.rename = None;
            }
        }
        if let Some(mut rename) = self.card_rename.take() {
            let mut open = true;
            let mut apply = false;
            let mut cancel = false;
            ui_shell::dialog("通道组备注").open(&mut open).default_width(460.0).collapsible(false).show(ctx, |ui| {
                small(ui, "整个通道组备注 · 留空显示原始名称");
                let edit = ui.add(TextEdit::singleline(&mut rename.draft).char_limit(40).desired_width(f32::INFINITY).margin(Margin::symmetric(12, 9)));
                if rename.focus { edit.request_focus(); rename.focus = false; }
                apply |= edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let original = self.channel_groups(rename.input, true).into_iter()
                    .find(|group| group.group == rename.group).map(|group| group.original_title)
                    .unwrap_or_else(|| "原始名称".into());
                let preview = if rename.draft.trim().is_empty() { original } else { format!("{} ({})", original, rename.draft.trim()) };
                ui.label(format!("软件内显示：{preview}"));
                small(ui, "备注仅用于本软件显示；不修改 Windows、DAW、硬件或应用的原始名称，不重启音频。");
                small(ui, "同组输入、输出通道组的备注各自保存，互不覆盖。");
                let nodes = if rename.input { &self.graph.inputs } else { &self.graph.outputs };
                for node in nodes.iter().filter(|n| Graph::card_group(&n.key) == rename.group) { small(ui, &format!("原始通道：{}", node.device_name)); }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    apply |= button(ui, "保存备注", false).clicked();
                    if button(ui, "恢复默认", false).clicked() { rename.draft.clear(); apply = true; }
                    if button(ui, "取消", false).clicked() { cancel = true; }
                });
            });
            if apply {
                self.graph
                    .set_card_name(rename.input, &rename.group, &rename.draft);
                self.dirty = true;
                self.status = "通道组备注已保存，仅改变软件内显示；音频继续运行。".into();
            } else if open && !cancel {
                self.card_rename = Some(rename);
            }
        }
    }
}
struct AsioPanel;
impl AsioPanel {
    fn open(d: &asio::Device) -> Result<(), String> {
        let a = asio::Asio::open(d)?;
        a.control_panel();
        Ok(())
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _: &mut eframe::Frame) {
        if self
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.on_update(ctx))
        {
            // Let the window integration finish closing; on_exit saves and stops audio.
            return;
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            if let Some(runtime) = &self.runtime {
                runtime.close_to_tray(ctx);
            }
        }
        self.frames = self.frames.wrapping_add(1);
        if self.screenshot.is_none() {
            self.about.updates.poll(ctx);
        }
        self.poll_engine();
        self.poll_driver_tools();
        self.poll_mcp();
        if self.audio_sync_pending {
            if let Some(e) = &self.engine {
                e.processor.sync(&self.graph, self.panic);
            }
            self.audio_sync_pending = false;
        }
        if self.screenshot.is_none()
            && (ctx.input(|i| i.viewport().minimized == Some(true))
                || self
                    .runtime
                    .as_ref()
                    .is_some_and(|r| r.hidden.load(std::sync::atomic::Ordering::Acquire)))
        {
            // Audio callbacks keep running independently. No canvas work while minimized.
            if self.dirty && self.last_save.elapsed() > Duration::from_secs(3) {
                self.save();
            }
            ctx.request_repaint_after(Duration::from_millis(250));
            return;
        }
        if self.engine.is_none() && !self.engine_busy && !self.driver_tools.busy() {
            let signature = format!(
                "{}:{}:{}:{}:{}:{}",
                self.config.backend,
                self.config.asio_name,
                self.config.input,
                self.config.output,
                self.config.rate,
                self.scene
            );
            if signature != self.layout_signature {
                self.layout_signature = signature;
                self.selected = None;
                self.route = None;
                self.rename = None;
                match audio::refresh_hardware(&self.config, &mut self.graph) {
                    Ok(()) => self.hardware_available = true,
                    Err(error) => {
                        self.hardware_available = false;
                        self.status = error;
                    }
                }
            }
        }
        self.levels
            .resize(self.graph.inputs.len() + self.graph.outputs.len(), 0.0);
        ctx.request_repaint_after(Duration::from_millis(if self.engine.is_some() {
            33
        } else {
            125
        }));
        if self.last_levels.elapsed() > Duration::from_millis(60) {
            if let Some(e) = &self.engine {
                let levels = e.processor.take_peaks();
                for (a, b) in self.levels.iter_mut().zip(levels) {
                    *a = b.max(*a * 0.75);
                }
                if let Ok(mut errors) = e.processor.errors.try_lock() {
                    if let Some(e) = errors.pop() {
                        self.status = format!("音频提示：{e}");
                    }
                }
            }
            self.last_levels = Instant::now();
        }
        if self.last_devices.elapsed() > Duration::from_secs(2) {
            self.system_status = self
                .engine
                .as_ref()
                .map(|e| e.system_status())
                .unwrap_or_else(|| {
                    if self.config.virtual_devices {
                        usb_audio::driver_status()
                    } else {
                        "Windows 虚拟设备已禁用".into()
                    }
                });
            if self.engine.is_some() {
                self.queue_device_refresh();
            }
            self.last_devices = Instant::now();
        }
        if let Some(receiver) = &self.device_refresh {
            match receiver.try_recv() {
                Ok((inputs, outputs)) => {
                    self.inputs = inputs;
                    self.outputs = outputs;
                    self.device_refresh = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.device_refresh = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => (),
            }
        }
        ui_shell::background(ctx);
        self.header(ctx);
        self.footer(ctx);
        CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(Margin::symmetric(27, 21)),
            )
            .show(ctx, |ui| {
                self.scene_bar(ui);
                self.channel_toolbar(ui);
                let board_height = ui.available_height().max(180.0);
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.tab == 0 {
                            self.board(ui, board_height);
                        } else {
                            self.matrix(ui, board_height);
                        }
                    });
            });
        if self.settings {
            self.settings_window(ctx);
        }
        self.channel_manager_window(ctx);
        self.about.show(ctx);
        self.application_picker(ctx);
        self.dialogs(ctx);
        if self.selected.is_some_and(|index| {
            self.graph
                .inputs
                .get(index)
                .is_none_or(|node| !node.visible)
        }) {
            self.selected = None;
        }
        self.apply_card_action();
        if self.audio_sync_pending {
            if let Some(e) = &self.engine {
                e.processor.sync(&self.graph, self.panic);
            }
            self.audio_sync_pending = false;
        }
        if self.dirty && self.last_save.elapsed() > Duration::from_secs(3) {
            self.save();
        }
        if self.status != self.last_notice {
            self.notices.push(self.status.clone());
            self.last_notice = self.status.clone();
        }
        ui_shell::resize_edges(ctx);
        self.notices.show(ctx);
        if let Some(path) = &self.screenshot {
            if self.frames >= 120 && !self.requested {
                ctx.send_viewport_cmd(ViewportCommand::Screenshot(UserData::default()));
                self.requested = true;
            }
            for event in ctx.input(|i| i.events.clone()) {
                if let Event::Screenshot { image, .. } = event {
                    let pixels: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    match image::save_buffer(
                        path,
                        &pixels,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    ) {
                        Ok(()) => ctx.send_viewport_cmd(ViewportCommand::Close),
                        Err(e) => {
                            eprintln!("Screenshot: {e}");
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                    }
                }
            }
        }
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        usb_audio::log("Runtime: graceful shutdown");
        self.mcp_server = None;
        self.engine = None;
        self.engine_controller
            .stop(self.engine_revision.wrapping_add(1));
        if self.screenshot.is_none() {
            self.save();
        }
    }
}
fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args().collect();
    let screenshot = args
        .iter()
        .position(|a| a == "--screenshot")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);
    let automatic = args.iter().any(|arg| arg == "--asio-managed");
    if screenshot.is_none() {
        usb_audio::log(&format!(
            "Runtime R2.4.3 startup: pid={}, asio_managed={automatic}",
            std::process::id()
        ));
    }
    if let Some(index) = args
        .iter()
        .position(|arg| arg == "--import-settings-if-missing")
    {
        let result = (|| -> Result<(), String> {
            let source = args.get(index + 1).ok_or("缺少旧配置文件路径")?;
            if data_path().exists() {
                return Ok(());
            }
            let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            serde_json::from_value::<Config>(value["config"].clone()).map_err(|e| e.to_string())?;
            Graph::from_value(value["graph"].clone()).ok_or("旧配置中的通道路由无效")?;
            let target = data_path();
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            use std::io::Write;
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)
            {
                Ok(mut file) => file.write_all(&bytes).map_err(|e| e.to_string())?,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.to_string()),
            }
            Ok(())
        })();
        if let Err(error) = result {
            usb_audio::log(&format!("Profile import failed: {error}"));
            std::process::exit(6);
        }
        return Ok(());
    }
    let mut session = None;
    let mut _instance = None;
    if screenshot.is_none() {
        let error = |e: String| eframe::Error::AppCreation(Box::new(std::io::Error::other(e)));
        let shared = std::sync::Arc::new(bridge::Lifecycle::open().map_err(error)?);
        match bridge::InstanceGuard::claim().map_err(error)? {
            Some(guard) => {
                _instance = Some(guard);
                shared.reset_ownership(!automatic);
                shared.set_mixer();
                session = Some(shared);
            }
            None => {
                if !automatic {
                    shared.keep_running();
                    shared.request_show();
                }
                return Ok(());
            }
        }
    }
    let size = args
        .iter()
        .position(|a| a == "--size")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.split_once('x'))
        .and_then(|(w, h)| Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?]))
        .unwrap_or([1440.0, 960.0]);
    let view = args
        .iter()
        .position(|a| a == "--view")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let mut viewport = ViewportBuilder::default()
        .with_title("Soon Link Pro · 音频路由工作台")
        .with_icon(
            eframe::icon_data::from_png_bytes(include_bytes!(
                "../assets/soon-link-pro-icon-256-v2.png"
            ))
            .expect("The embedded Soon Link Pro icon is valid"),
        )
        .with_decorations(false)
        .with_resizable(true)
        .with_inner_size(size)
        .with_min_inner_size([960.0, 640.0]);
    if screenshot.is_some() {
        viewport = viewport
            .with_active(false)
            .with_window_level(WindowLevel::AlwaysOnBottom);
    }
    let options = eframe::NativeOptions {
        viewport,
        persist_window: screenshot.is_none(),
        renderer: eframe::Renderer::Glow,
        vsync: true,
        ..Default::default()
    };
    eframe::run_native(
        "Soon Link Pro",
        options,
        Box::new(move |cc| {
            let mut app = App::new(cc, screenshot);
            // Submit audio startup before a managed instance hides its window.
            // Opening hardware remains on the audio controller thread.
            app.poll_engine();
            if let Some(session) = session {
                let runtime = runtime::Runtime::new(cc, session, automatic);
                if let Some(error) = &runtime.tray_error {
                    app.status = format!("托盘不可用，关闭窗口将退出：{error}");
                }
                app.runtime = Some(runtime);
            }
            if app.screenshot.is_some() {
                cc.egui_ctx
                    .send_viewport_cmd(ViewportCommand::Maximized(false));
                cc.egui_ctx
                    .send_viewport_cmd(ViewportCommand::InnerSize(vec2(size[0], size[1])));
            }
            match view.as_str() {
                "matrix" => app.tab = 1,
                "applications" => {
                    app.app_picker = true;
                    app.refresh_app_candidates();
                }
                "drivers" => {
                    app.settings = true;
                    app.settings_tab = 1;
                }
                "windows" => {
                    app.settings = true;
                    app.settings_tab = 1;
                }
                _ => (),
            }
            Ok(Box::new(app))
        }),
    )
}

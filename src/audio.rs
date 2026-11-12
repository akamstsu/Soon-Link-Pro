use crate::asio::{self, Asio};
use bridge::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use std::{
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
#[derive(Clone, Serialize, Deserialize)]
pub struct Node {
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub device_name: String,
    pub key: String,
    pub name: String,
    pub fader: f32,
    pub mute: bool,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct Route {
    pub on: bool,
    pub fader: f32,
    pub mute: bool,
}
impl Default for Route {
    fn default() -> Self {
        Self {
            on: false,
            fader: 1.0,
            mute: false,
        }
    }
}
fn legacy_bus_count() -> usize {
    4
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Graph {
    #[serde(default)]
    pub card_names: std::collections::BTreeMap<String, String>,
    #[serde(default = "legacy_bus_count")]
    pub bus_count: usize,
    pub inputs: Vec<Node>,
    pub outputs: Vec<Node>,
    pub routes: Vec<Vec<Route>>,
    #[serde(default)]
    pub applications: Vec<crate::process_capture::Target>,
}
fn node(key: String, name: String) -> Node {
    Node {
        visible: key.starts_with("hardware:"),
        device_name: name.clone(),
        key,
        name,
        fader: 1.0,
        mute: false,
    }
}
pub fn virtual_key(bus: usize, ch: usize) -> String {
    format!("virtual:{bus}:{ch}")
}
pub fn hardware_key(ch: usize) -> String {
    format!("hardware:{ch}")
}
fn app_key(exe: &str, ch: usize) -> String {
    format!("app:{}:{ch}", exe.to_lowercase())
}
impl Graph {
    pub fn new(ni: usize, no: usize) -> Self {
        let mut g = Self {
            card_names: Default::default(),
            bus_count: 1,
            inputs: Vec::new(),
            outputs: Vec::new(),
            routes: Vec::new(),
            applications: Vec::new(),
        };
        g.reconfigure(ni, no);
        g
    }
    pub fn valid(&self) -> bool {
        let valid_key = |n: &Node, input: bool| {
            let parts = n.key.split(':').collect::<Vec<_>>();
            match parts.first().copied().unwrap_or("") {
                "hardware" | "dawhardware" => {
                    parts.len() == 2
                        && parts[1]
                            .parse::<usize>()
                            .is_ok_and(|c| c < MAX_HARDWARE_CHANNELS)
                }
                "system" => {
                    input && parts.len() == 2 && parts[1].parse::<usize>().is_ok_and(|c| c < 2)
                }
                "virtual" | "dawvirtual" => {
                    parts.len() == 3
                        && parts[1].parse::<usize>().is_ok_and(|b| b < self.bus_count)
                        && parts[2].parse::<usize>().is_ok_and(|c| c < 2)
                }
                "app" => {
                    input
                        && self
                            .applications
                            .iter()
                            .any(|a| (0..2).any(|c| app_key(&a.exe, c) == n.key))
                }
                _ => false,
            }
        };
        self.bus_count
            .checked_mul(2)
            .is_some_and(|n| n <= (i32::MAX as usize).saturating_sub(MAX_HARDWARE_CHANNELS))
            && self.routes.len() == self.inputs.len()
            && self.routes.iter().all(|r| r.len() == self.outputs.len())
            && self.applications.len() <= crate::process_capture::MAX_APPLICATIONS
            && self
                .inputs
                .iter()
                .all(|n| n.fader.is_finite() && valid_key(n, true))
            && self
                .outputs
                .iter()
                .all(|n| n.fader.is_finite() && valid_key(n, false))
            && self.routes.iter().flatten().all(|r| r.fader.is_finite())
            && self
                .inputs
                .iter()
                .map(|n| &n.key)
                .collect::<std::collections::HashSet<_>>()
                .len()
                == self.inputs.len()
            && self
                .outputs
                .iter()
                .map(|n| &n.key)
                .collect::<std::collections::HashSet<_>>()
                .len()
                == self.outputs.len()
    }

    pub fn card_group(key: &str) -> String {
        if let Some(ch) = key
            .strip_prefix("hardware:")
            .and_then(|s| s.parse::<usize>().ok())
        {
            format!("hardware:{}", ch / 2)
        } else {
            key.rsplit_once(':')
                .map(|(root, _)| root)
                .unwrap_or(key)
                .into()
        }
    }
    fn alias_key(input: bool, group: &str) -> String {
        format!("{input}:{group}")
    }
    pub fn card_name(&self, input: bool, group: &str) -> Option<&str> {
        self.card_names
            .get(&Self::alias_key(input, group))
            .map(String::as_str)
    }
    pub fn set_card_name(&mut self, input: bool, group: &str, name: &str) {
        let name: String = name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(40)
            .collect();
        let key = Self::alias_key(input, group);
        if name.is_empty() {
            self.card_names.remove(&key);
        } else {
            self.card_names.insert(key, name);
        }
    }
    pub fn virtual_name(&self, input: bool, bus: usize) -> String {
        self.card_name(input, &format!("virtual:{bus}"))
            .map(str::to_owned)
            .unwrap_or_else(|| crate::usb_audio::name(bus))
    }
    pub fn channel_name(&self, input: bool, index: usize) -> String {
        let node = if input {
            &self.inputs[index]
        } else {
            &self.outputs[index]
        };
        let group = Self::card_group(&node.key);
        if let Some(name) = self.card_name(input, &group) {
            let ch = node
                .key
                .rsplit(':')
                .next()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let other = format!("hardware:{}", ch ^ 1);
            let mono = node.key.starts_with("hardware:")
                && !(if input { &self.inputs } else { &self.outputs })
                    .iter()
                    .any(|n| n.key == other);
            format!(
                "{name} · {}",
                if mono {
                    "MONO"
                } else if ch % 2 == 0 {
                    "L"
                } else {
                    "R"
                }
            )
        } else {
            node.name.clone()
        }
    }
    fn migrate_card_names(&mut self) {
        for value in self.card_names.values_mut() {
            *value = value
                .trim()
                .chars()
                .filter(|c| !c.is_control())
                .take(40)
                .collect();
        }
        self.card_names.retain(|_, value| !value.is_empty());
        // Older profiles used one alias for both directions. Seed each side once,
        // preserving directional aliases already saved by a newer version.
        let shared: Vec<_> = self
            .card_names
            .keys()
            .filter(|key| key.starts_with("virtual:"))
            .cloned()
            .collect();
        for key in shared {
            if let Some(name) = self.card_names.remove(&key) {
                for input in [true, false] {
                    self.card_names
                        .entry(Self::alias_key(input, &key))
                        .or_insert_with(|| name.clone());
                }
            }
        }
        for (input, nodes) in [(true, &self.inputs), (false, &self.outputs)] {
            for node in nodes {
                if node.device_name.is_empty() || node.name == node.device_name {
                    continue;
                }
                let group = Self::card_group(&node.key);
                let key = Self::alias_key(input, &group);
                let mut base = node.name.as_str();
                for suffix in [" · L", " · R", " · MONO"] {
                    if let Some(value) = base.strip_suffix(suffix) {
                        base = value;
                        break;
                    }
                }
                let base: String = base
                    .trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(40)
                    .collect();
                if !base.is_empty() {
                    self.card_names.entry(key).or_insert(base);
                }
            }
        }
        // Node metadata continues to hold actual upstream names, independently of aliases.
        for node in self.inputs.iter_mut().chain(&mut self.outputs) {
            if !node.device_name.is_empty() {
                node.name = node.device_name.clone();
            }
        }
    }

    pub fn counts(&self) -> (usize, usize) {
        (
            self.inputs
                .iter()
                .filter(|n| n.key.starts_with("hardware:"))
                .count(),
            self.outputs
                .iter()
                .filter(|n| n.key.starts_with("hardware:"))
                .count(),
        )
    }
    pub fn reconfigure(&mut self, ni: usize, no: usize) {
        let mut inputs = (0..ni)
            .map(|ch| node(hardware_key(ch), format!("输入通道 {}", ch + 1)))
            .collect::<Vec<_>>();
        let mut outputs = (0..no)
            .map(|ch| node(hardware_key(ch), format!("输出通道 {}", ch + 1)))
            .collect::<Vec<_>>();
        for bus in 0..self.bus_count {
            for ch in 0..2 {
                let name = format!(
                    "{} · {}",
                    crate::usb_audio::name(bus),
                    if ch == 0 { "L" } else { "R" }
                );
                let mut input = node(virtual_key(bus, ch), name.clone());
                input.visible = true;
                let mut output = node(virtual_key(bus, ch), name);
                output.visible = true;
                inputs.push(input);
                outputs.push(output);
            }
        }
        inputs.extend((0..2).map(|ch| {
            node(
                format!("system:{ch}"),
                format!("系统声音 {}", if ch == 0 { "L" } else { "R" }),
            )
        }));
        for app in &self.applications {
            for ch in 0..2 {
                let mut n = node(
                    app_key(&app.exe, ch),
                    format!("{} · {}", app.label(), if ch == 0 { "L" } else { "R" }),
                );
                n.visible = true;
                inputs.push(n);
            }
        }
        let canonical = |key: &str| -> Option<String> {
            if key.starts_with("dawhardware:") {
                None
            } else {
                Some(key.replacen("dawvirtual:", "virtual:", 1))
            }
        };
        for (new, old) in [(&mut inputs, &self.inputs), (&mut outputs, &self.outputs)] {
            for n in new {
                if let Some(previous) = old.iter().find(|o| o.key == n.key) {
                    let generated = previous.device_name.is_empty()
                        && (previous.name.starts_with("硬件输入 ")
                            || previous.name.starts_with("硬件输出 ")
                            || previous.name.starts_with("虚拟播放 ")
                            || previous.name.starts_with("虚拟录音 "));
                    let mut restored = previous.clone();
                    if generated {
                        restored.name = n.name.clone();
                        restored.device_name = n.device_name.clone();
                    }
                    *n = restored;
                }
            }
        }
        let index_i = inputs
            .iter()
            .enumerate()
            .map(|(i, n)| (n.key.clone(), i))
            .collect::<std::collections::HashMap<_, _>>();
        let index_o = outputs
            .iter()
            .enumerate()
            .map(|(i, n)| (n.key.clone(), i))
            .collect::<std::collections::HashMap<_, _>>();
        let mut routes = vec![vec![Route::default(); outputs.len()]; inputs.len()];
        for (i, old_i) in self.inputs.iter().enumerate() {
            for (o, old_o) in self.outputs.iter().enumerate() {
                let (Some(ik), Some(ok)) = (canonical(&old_i.key), canonical(&old_o.key)) else {
                    continue;
                };
                // Old Windows<->DAW self bridges are now the same bus, not feedback cables.
                if ik == ok && old_i.key != old_o.key && ik.starts_with("virtual:") {
                    continue;
                }
                if let (Some(&ii), Some(&oo), Some(route)) = (
                    index_i.get(&ik),
                    index_o.get(&ok),
                    self.routes.get(i).and_then(|row| row.get(o)),
                ) {
                    if !routes[ii][oo].on || route.on {
                        routes[ii][oo] = *route;
                    }
                }
            }
        }
        self.inputs = inputs;
        self.outputs = outputs;
        self.routes = routes;
    }
    pub fn remove_bus(&mut self, bus: usize) -> Result<(), String> {
        if bus >= self.bus_count {
            return Err("这组虚拟总线已不存在".into());
        }
        if self.bus_count <= 1 {
            return Err("至少保留一组虚拟输入／输出以兼容 DAW；不需要显示时可隐藏卡片。".into());
        }
        let prefix = format!("virtual:{bus}:");
        let keep_inputs: Vec<_> = self
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.key.starts_with(&prefix))
            .map(|(i, _)| i)
            .collect();
        let keep_outputs: Vec<_> = self
            .outputs
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.key.starts_with(&prefix))
            .map(|(i, _)| i)
            .collect();
        // Build the reduced matrix from original indices before renumbering identifiers.
        let routes = keep_inputs
            .iter()
            .map(|&i| keep_outputs.iter().map(|&o| self.routes[i][o]).collect())
            .collect();
        let mut inputs: Vec<_> = keep_inputs
            .iter()
            .map(|&i| self.inputs[i].clone())
            .collect();
        let mut outputs: Vec<_> = keep_outputs
            .iter()
            .map(|&o| self.outputs[o].clone())
            .collect();
        for node in inputs.iter_mut().chain(&mut outputs) {
            if let Some(rest) = node.key.strip_prefix("virtual:") {
                if let Some((index, channel)) = rest.split_once(':') {
                    if let (Ok(index), Ok(channel)) =
                        (index.parse::<usize>(), channel.parse::<usize>())
                    {
                        if index > bus {
                            let name = format!(
                                "{} · {}",
                                crate::usb_audio::name(index - 1),
                                if channel == 0 { "L" } else { "R" }
                            );
                            if node.name == node.device_name {
                                node.name = name.clone();
                            }
                            node.device_name = name;
                            node.key = virtual_key(index - 1, channel);
                        }
                    }
                }
            }
        }
        self.bus_count -= 1;
        self.card_names = std::mem::take(&mut self.card_names)
            .into_iter()
            .filter_map(|(key, name)| {
                if let Some((prefix, index)) = ["true:virtual:", "false:virtual:", "virtual:"]
                    .into_iter()
                    .find_map(|prefix| {
                        key.strip_prefix(prefix)
                            .and_then(|s| s.parse::<usize>().ok())
                            .map(|index| (prefix, index))
                    })
                {
                    if index == bus {
                        None
                    } else {
                        Some((
                            format!("{prefix}{}", if index > bus { index - 1 } else { index }),
                            name,
                        ))
                    }
                } else {
                    Some((key, name))
                }
            })
            .collect();
        self.inputs = inputs;
        self.outputs = outputs;
        self.routes = routes;
        Ok(())
    }
    pub fn remove_application(&mut self, exe: &str) {
        self.card_names
            .remove(&format!("true:app:{}", exe.to_lowercase()));
        let prefix = format!("app:{}:", exe.to_lowercase());
        let keep: Vec<_> = self
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.key.starts_with(&prefix))
            .map(|(i, _)| i)
            .collect();
        self.inputs = keep.iter().map(|&i| self.inputs[i].clone()).collect();
        self.routes = keep.iter().map(|&i| self.routes[i].clone()).collect();
        self.applications
            .retain(|app| !app.exe.eq_ignore_ascii_case(exe));
    }
    pub fn add_bus(&mut self) -> Result<(), String> {
        let next = self
            .bus_count
            .checked_add(1)
            .ok_or("通道数量已超过系统可表示范围")?;
        if next
            .checked_mul(2)
            .is_none_or(|n| n > (i32::MAX as usize).saturating_sub(MAX_HARDWARE_CHANNELS))
        {
            return Err("通道数量超过 ASIO 接口可表示范围".into());
        }
        // No fixed product limit: reserve the dynamic matrix before changing the graph.
        let (ni, no) = self.counts();
        let rows = ni + next * 2 + 2 + self.applications.len() * 2;
        let cols = no + next * 2;
        let mut reservation = Vec::<Route>::new();
        reservation
            .try_reserve(rows.checked_mul(cols).ok_or("内存需求超过系统范围")?)
            .map_err(|_| "当前内存不足，无法再添加虚拟总线")?;
        drop(reservation);
        self.bus_count = next;
        self.reconfigure(ni, no);
        Ok(())
    }
    pub fn set_device_names(&mut self, input: bool, names: &[String]) {
        let nodes = if input {
            &mut self.inputs
        } else {
            &mut self.outputs
        };
        for n in nodes {
            if let Some(ch) = n
                .key
                .strip_prefix("hardware:")
                .and_then(|s| s.parse::<usize>().ok())
            {
                if let Some(name) = names.get(ch) {
                    if n.name == n.device_name
                        || n.name.starts_with("硬件输入 ")
                        || n.name.starts_with("硬件输出 ")
                    {
                        n.name = name.clone();
                    }
                    n.device_name = name.clone();
                }
            }
        }
    }
    pub fn add_application(
        &mut self,
        target: crate::process_capture::Target,
    ) -> Result<(), String> {
        if self.applications.len() >= crate::process_capture::MAX_APPLICATIONS {
            return Err("最多采集32个应用".into());
        }
        if self
            .applications
            .iter()
            .any(|t| t.exe.eq_ignore_ascii_case(&target.exe))
        {
            return Err("应用已添加".into());
        }
        self.applications.push(target);
        let (ni, no) = self.counts();
        self.reconfigure(ni, no);
        Ok(())
    }
    pub fn from_value(value: serde_json::Value) -> Option<Self> {
        if let Ok(mut g) = serde_json::from_value::<Self>(value.clone()) {
            // Older profiles and scenes could have no virtual channels. Add one
            // empty bus without replacing existing routing, gains or aliases.
            g.bus_count = g.bus_count.max(1);
            if !g.valid() {
                return None;
            }
            let (ni, no) = g.counts();
            g.reconfigure(ni, no);
            g.migrate_card_names();
            return Some(g);
        }
        #[derive(Deserialize)]
        struct OldNode {
            name: String,
            fader: f32,
            mute: bool,
            #[serde(default)]
            application: Option<crate::process_capture::Target>,
        }
        #[derive(Deserialize)]
        struct Old {
            inputs: Vec<OldNode>,
            outputs: Vec<OldNode>,
            routes: Vec<Vec<Route>>,
        }
        let old = serde_json::from_value::<Old>(value).ok()?;
        if old.inputs.len() < 6
            || old.outputs.len() != 5
            || old.routes.len() != old.inputs.len()
            || old.routes.iter().any(|r| r.len() != 5)
            || old.inputs.iter().skip(6).any(|n| n.application.is_none())
        {
            return None;
        }
        let mut g = Graph::new(2, 2);
        g.bus_count = 4;
        g.reconfigure(2, 2);
        g.applications = old
            .inputs
            .iter()
            .skip(6)
            .filter_map(|n| n.application.clone())
            .collect();
        g.reconfigure(2, 2);
        let ikey = |i: usize, c: usize| {
            if i == 0 {
                hardware_key(c)
            } else if i == 1 {
                format!("system:{c}")
            } else if i < 6 {
                virtual_key(i - 2, c)
            } else {
                app_key(&old.inputs[i].application.as_ref().unwrap().exe, c)
            }
        };
        let okey = |o: usize, c: usize| {
            if o == 0 {
                hardware_key(c)
            } else {
                virtual_key(o - 1, c)
            }
        };
        for (i, n) in old.inputs.iter().enumerate() {
            for c in 0..2 {
                let k = ikey(i, c);
                if let Some(new) = g.inputs.iter_mut().find(|n| n.key == k) {
                    new.fader = n.fader;
                    new.mute = n.mute;
                    let defaults = [
                        "声卡输入",
                        "系统声音",
                        "虚拟 ASIO 1",
                        "虚拟 ASIO 2",
                        "虚拟 ASIO 3",
                        "虚拟 ASIO 4",
                    ];
                    let modern = [
                        "声卡输入",
                        "系统声音",
                        "虚拟播放 1",
                        "虚拟播放 2",
                        "虚拟播放 3",
                        "虚拟播放 4",
                    ];
                    if i >= 6 || (n.name != defaults[i] && n.name != modern[i]) {
                        new.name = format!("{} · {}", n.name, if c == 0 { "L" } else { "R" });
                    }
                }
            }
        }
        for (o, n) in old.outputs.iter().enumerate() {
            for c in 0..2 {
                let k = okey(o, c);
                if let Some(new) = g.outputs.iter_mut().find(|n| n.key == k) {
                    new.fader = n.fader;
                    new.mute = n.mute;
                    let defaults = [
                        "耳机 / 监听",
                        "ASIO 1 · 录音",
                        "ASIO 2 · 通话",
                        "ASIO 3 · 处理",
                        "ASIO 4 · 直播",
                    ];
                    let modern = [
                        "耳机 / 监听",
                        "虚拟录音 1",
                        "虚拟录音 2",
                        "虚拟录音 3",
                        "虚拟录音 4",
                    ];
                    if n.name != defaults[o] && n.name != modern[o] {
                        new.name = format!("{} · {}", n.name, if c == 0 { "L" } else { "R" });
                    }
                }
            }
        }
        for i in 0..old.inputs.len() {
            for o in 0..5 {
                for c in 0..2 {
                    let ii = g.inputs.iter().position(|n| n.key == ikey(i, c))?;
                    let oo = g.outputs.iter().position(|n| n.key == okey(o, c))?;
                    g.routes[ii][oo] = old.routes[i][o];
                }
            }
        }
        g.migrate_card_names();
        Some(g)
    }
}
impl Default for Graph {
    fn default() -> Self {
        Self::new(0, 0)
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub backend: usize,
    pub asio_name: String,
    pub input: String,
    pub output: String,
    pub rate: u32,
    pub buffer: usize,
    pub input_pair: i32,
    pub output_pair: i32,
    pub loopback: bool,
    pub virtual_devices: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            backend: 0,
            asio_name: String::new(),
            input: String::new(),
            output: String::new(),
            rate: DEFAULT_RATE,
            buffer: DEFAULT_BUFFER,
            input_pair: 0,
            output_pair: 0,
            loopback: false,
            virtual_devices: true,
        }
    }
}
#[derive(Clone)]
enum Source {
    Hardware(usize),
    System(usize),
    Virtual(usize, usize),
    App(usize, usize),
}
pub struct Plan {
    connections: Vec<(usize, usize, f32)>,
    input_gain: Vec<f32>,
    output_gain: Vec<f32>,
}
pub struct Processor {
    pub buses: Vec<Mapping>,
    pub virtual_buses: usize,
    pub hardware: HardwareMapping,
    pub physical_inputs: usize,
    pub physical_outputs: usize,
    pub capture_multi: Box<FrameRing>,
    pub loopback: Box<Ring>,
    pub applications: Vec<Box<Ring>>,
    pub windows_capture: Vec<Box<Ring>>,
    pub windows_render: Vec<Box<Ring>>,
    pub windows_render_enabled: Vec<AtomicBool>,
    pub windows_render_heartbeat: Vec<AtomicU64>,
    pub windows_capture_frames: Vec<AtomicU64>,
    pub windows_render_frames: Vec<AtomicU64>,
    pub system_driver_active: AtomicBool,
    buffer: usize,
    active: AtomicBool,
    sources: Vec<Source>,
    output_ports: Vec<(u8, usize, usize)>,
    peaks: Vec<AtomicU32>,
    plan: Mutex<Arc<Plan>>,
    // The UI retains replaced plans so the realtime callback never frees their vectors.
    retired_plans: Mutex<Vec<Arc<Plan>>>,
    pub frames: AtomicU64,
    pub clips: AtomicU64,
    pub errors: Mutex<Vec<String>>,
    pub rate: u32,
    pid: u32,
}
fn zero_box<T>() -> Box<T> {
    unsafe {
        let layout = std::alloc::Layout::new::<T>();
        let p = std::alloc::alloc_zeroed(layout);
        if p.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Box::from_raw(p.cast())
    }
}
impl Processor {
    fn new(c: &Config, g: &Graph, ni: usize, no: usize) -> Result<Arc<Self>, String> {
        if ni > MAX_HARDWARE_CHANNELS || no > MAX_HARDWARE_CHANNELS {
            return Err("本版最多转发每方向128个硬件通道，不会截断声卡通道".into());
        }
        let buses = (0..g.bus_count.max(1))
            .map(Mapping::open)
            .collect::<Result<Vec<_>, _>>()?;
        if buses.iter().any(|b| b.shared().engine_live()) {
            return Err("另一实例正在使用音频引擎，请先停止旧版".into());
        }
        let hardware = HardwareMapping::open()?;
        let h = hardware.shared();
        h.generation.store(0, Ordering::Release);
        h.input_channels.store(ni as u32, Ordering::Release);
        h.output_channels.store(no as u32, Ordering::Release);
        h.virtual_buses.store(g.bus_count as u32, Ordering::Release);
        h.engine_frames.store(0, Ordering::Release);
        h.client_buffer.store(c.buffer as u32, Ordering::Release);
        h.from_daw.discard();
        let pid = std::process::id();
        for b in &buses {
            let s = b.shared();
            s.engine_owner.store(pid, Ordering::Release);
            s.rate.store(c.rate, Ordering::Release);
            s.to_engine.discard();
        }
        let mut sources = Vec::new();
        for n in &g.inputs {
            let source = if let Some(c) = n.key.strip_prefix("hardware:") {
                Source::Hardware(c.parse::<usize>().map_err(|_| "输入通道无效")?)
            } else if let Some(c) = n.key.strip_prefix("system:") {
                Source::System(c.parse::<usize>().map_err(|_| "系统通道无效")?)
            } else if n.key.starts_with("virtual:") {
                let p = n.key.split(':').collect::<Vec<_>>();
                Source::Virtual(
                    p[1].parse().map_err(|_| "总线无效")?,
                    p[2].parse().map_err(|_| "通道无效")?,
                )
            } else {
                let (slot, ch) = g
                    .applications
                    .iter()
                    .enumerate()
                    .find_map(|(i, a)| {
                        (0..2)
                            .find(|c| app_key(&a.exe, *c) == n.key)
                            .map(|c| (i, c))
                    })
                    .ok_or("应用端口无效")?;
                Source::App(slot, ch)
            };
            sources.push(source);
        }
        let output_ports = g
            .outputs
            .iter()
            .map(|n| {
                if let Some(c) = n.key.strip_prefix("hardware:") {
                    (0, c.parse::<usize>().unwrap(), 0)
                } else {
                    let p = n.key.split(':').collect::<Vec<_>>();
                    (
                        if p[0] == "virtual" { 1 } else { 2 },
                        p[1].parse::<usize>().unwrap(),
                        p[2].parse::<usize>().unwrap(),
                    )
                }
            })
            .collect();
        let p = Arc::new(Self {
            buses,
            virtual_buses: g.bus_count,
            hardware,
            physical_inputs: ni,
            physical_outputs: no,
            capture_multi: zero_box(),
            loopback: zero_box(),
            applications: g.applications.iter().map(|_| zero_box()).collect(),
            windows_capture: (0..g.bus_count).map(|_| zero_box()).collect(),
            windows_render: (0..g.bus_count).map(|_| zero_box()).collect(),
            windows_render_enabled: (0..g.bus_count).map(|_| AtomicBool::new(false)).collect(),
            windows_render_heartbeat: (0..g.bus_count).map(|_| AtomicU64::new(0)).collect(),
            windows_capture_frames: (0..g.bus_count).map(|_| AtomicU64::new(0)).collect(),
            windows_render_frames: (0..g.bus_count).map(|_| AtomicU64::new(0)).collect(),
            system_driver_active: AtomicBool::new(false),
            buffer: c.buffer,
            active: AtomicBool::new(true),
            sources,
            output_ports,
            peaks: (0..g.inputs.len() + g.outputs.len())
                .map(|_| AtomicU32::new(0))
                .collect(),
            plan: Mutex::new(Arc::new(Plan {
                connections: Vec::new(),
                input_gain: Vec::new(),
                output_gain: Vec::new(),
            })),
            retired_plans: Mutex::new(Vec::new()),
            frames: AtomicU64::new(0),
            clips: AtomicU64::new(0),
            errors: Mutex::new(Vec::new()),
            rate: c.rate,
            pid,
        });
        p.sync(g, false);
        Ok(p)
    }
    pub fn sync(&self, g: &Graph, panic: bool) {
        let node_gain = |n: &Node| if n.mute { 0.0 } else { gain(n.fader) };
        let mut connections = Vec::new();
        for (i, row) in g.routes.iter().enumerate() {
            for (o, r) in row.iter().enumerate() {
                if r.on && !r.mute && !panic {
                    let v = node_gain(&g.inputs[i]) * gain(r.fader) * node_gain(&g.outputs[o]);
                    if v != 0.0 {
                        connections.push((i, o, v));
                    }
                }
            }
        }
        let new_plan = Arc::new(Plan {
            connections,
            input_gain: g
                .inputs
                .iter()
                .map(|n| if panic { 0.0 } else { node_gain(n) })
                .collect(),
            output_gain: g
                .outputs
                .iter()
                .map(|n| if panic { 0.0 } else { node_gain(n) })
                .collect(),
        });
        let old = std::mem::replace(&mut *self.plan.lock().unwrap(), new_plan);
        let mut retired = self.retired_plans.lock().unwrap();
        retired.retain(|plan| Arc::strong_count(plan) > 1);
        retired.push(old);
    }

    pub fn plan(&self) -> Arc<Plan> {
        self.plan.lock().unwrap().clone()
    }
    fn update_plan(&self, plan: &mut Arc<Plan>) {
        if let Ok(p) = self.plan.try_lock() {
            if !Arc::ptr_eq(plan, &p) {
                *plan = p.clone();
            }
        }
    }
    fn begin(&self, scratch: &mut FrameScratch) {
        scratch.peaks.fill(0.0);
        scratch.processed_frames = 0;
        scratch.clips = 0;
        let now = clock_ms();
        for b in &self.buses {
            b.shared().heartbeat.store(now, Ordering::Release);
        }
        for b in 0..self.virtual_buses {
            if now.saturating_sub(self.windows_render_heartbeat[b].load(Ordering::Relaxed)) > 200 {
                self.windows_render_enabled[b].store(false, Ordering::Release);
            }
            scratch.windows_render_enabled[b] =
                self.windows_render_enabled[b].load(Ordering::Relaxed);
        }
        for b in 0..self.buses.len() {
            let live = self.buses[b].shared().client_live();
            let previous = std::mem::replace(&mut scratch.asio_clients[b], live);
            if live && !previous {
                if b == 0 {
                    self.hardware.shared().from_daw.discard();
                    scratch.hardware_armed = false;
                }
                self.buses[b].shared().to_engine.discard();
                if let Some(reader) = scratch.asio_readers.get_mut(b) {
                    reader.reset();
                }
            }
        }
    }
    fn frame(&self, physical: &[f32], output: &mut [f32], plan: &Plan, scratch: &mut FrameScratch) {
        let system = self.loopback.pop().unwrap_or([0.0; 2]);
        for (i, ring) in self.applications.iter().enumerate() {
            scratch.apps[i] = ring.pop().unwrap_or([0.0; 2]);
        }
        let client = scratch.asio_clients[0];
        for bus in 0..self.virtual_buses {
            // Reader state belongs exclusively to this output callback.
            let windows = scratch.windows_readers[bus].next(&self.windows_capture[bus]);
            let asio = if client {
                scratch.asio_readers[bus].next(&self.buses[bus].shared().to_engine)
            } else {
                [0.0; 2]
            };
            scratch.virtuals[bus] = [windows[0] + asio[0], windows[1] + asio[1]];
        }
        let h = self.hardware.shared();
        let mut direct = [0.0; MAX_HARDWARE_CHANNELS];
        if client {
            let threshold = (h.client_buffer.load(Ordering::Relaxed) as usize * 2)
                .min(HARDWARE_CAPACITY)
                .max(2);
            if !scratch.hardware_armed && h.from_daw.available() >= threshold {
                scratch.hardware_armed = true;
            }
            if scratch.hardware_armed && !h.from_daw.pop(&mut direct[..self.physical_outputs]) {
                scratch.hardware_armed = false;
            }
        } else {
            scratch.hardware_armed = false;
            h.from_daw.discard();
        }
        let mut daw_hardware = [0.0; MAX_HARDWARE_CHANNELS];
        for (i, source) in self.sources.iter().enumerate() {
            let value = match *source {
                Source::Hardware(ch) => physical.get(ch).copied().unwrap_or(0.0),
                Source::System(ch) => system[ch],
                Source::Virtual(bus, ch) => scratch.virtuals[bus][ch],
                Source::App(slot, ch) => scratch.apps[slot][ch],
            };
            let value = if value.is_finite() { value } else { 0.0 };
            scratch.sources[i] = value;
            scratch.peaks[i] = scratch.peaks[i].max(value.abs());
            if let Source::Hardware(ch) = source {
                daw_hardware[*ch] = value * plan.input_gain[i];
            }
        }
        scratch.mixed.fill(0.0);
        for &(i, o, gain) in &plan.connections {
            scratch.mixed[o] += scratch.sources[i] * gain;
        }
        scratch.sends.fill([0.0; 2]);
        output.fill(0.0);
        for (o, &(kind, a, b)) in self.output_ports.iter().enumerate() {
            let value = scratch.mixed[o]
                + if kind == 0 {
                    direct[a] * plan.output_gain[o]
                } else {
                    0.0
                };
            let value = if value.is_finite() { value } else { 0.0 };
            if value.abs() > 1.0 {
                scratch.clips += 1;
            }
            if kind == 0 {
                output[a] = value.clamp(-1.0, 1.0);
            } else {
                scratch.sends[a][b] = value.clamp(-1.0, 1.0);
            }
            let peak_index = self.sources.len() + o;
            scratch.peaks[peak_index] = scratch.peaks[peak_index].max(value.abs());
        }
        if client && self.physical_inputs > 0 {
            h.to_daw.push(&daw_hardware[..self.physical_inputs]);
        }
        for bus in 0..self.virtual_buses {
            if client {
                self.buses[bus]
                    .shared()
                    .to_client
                    .push_limited(scratch.sends[bus], (self.rate as usize / 20).min(CAPACITY));
            }
            if scratch.windows_render_enabled[bus] {
                self.windows_render[bus].push(scratch.sends[bus]);
            }
        }
        scratch.processed_frames += 1;
    }

    fn finish(&self, scratch: &FrameScratch) {
        // Publish once per audio block instead of an atomic read/modify/write
        // for every sample and every meter. UI polling still receives each block's peak.
        for (index, &peak) in scratch.peaks.iter().enumerate() {
            if peak > 0.0 {
                self.peak(index, peak);
            }
        }
        if scratch.clips > 0 {
            self.clips.fetch_add(scratch.clips, Ordering::Relaxed);
        }
        let frames = self
            .frames
            .fetch_add(scratch.processed_frames, Ordering::Relaxed)
            + scratch.processed_frames;
        self.hardware
            .shared()
            .engine_frames
            .store(frames, Ordering::Release);
    }
    fn peak(&self, i: usize, v: f32) {
        let _ = self.peaks[i].fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
            Some(f32::from_bits(old).max(v).to_bits())
        });
    }
    pub fn take_peaks(&self) -> Vec<f32> {
        self.peaks
            .iter()
            .map(|p| f32::from_bits(p.swap(0, Ordering::Relaxed)))
            .collect()
    }
    fn fault(&self, e: cpal::StreamError) {
        if let Ok(mut errors) = self.errors.try_lock() {
            if errors.len() < 8 {
                errors.push(e.to_string());
            }
        }
    }
}
impl Processor {
    fn deactivate(&self) {
        if !self.active.swap(false, Ordering::AcqRel) {
            return;
        }
        self.hardware
            .shared()
            .generation
            .store(0, Ordering::Release);
        for b in &self.buses {
            if b.shared().engine_owner.load(Ordering::Acquire) == self.pid {
                b.shared().heartbeat.store(0, Ordering::Release);
                b.shared().engine_owner.store(0, Ordering::Release);
            }
        }
    }
}
impl Drop for Processor {
    fn drop(&mut self) {
        self.deactivate();
    }
}

struct FrameScratch {
    windows_readers: Vec<AdaptiveReader>,
    asio_readers: Vec<AdaptiveReader>,
    asio_clients: Vec<bool>,
    windows_render_enabled: Vec<bool>,
    hardware_armed: bool,
    peaks: Vec<f32>,
    processed_frames: u64,
    clips: u64,
    sources: Vec<f32>,
    mixed: Vec<f32>,
    virtuals: Vec<[f32; 2]>,
    sends: Vec<[f32; 2]>,
    apps: Vec<[f32; 2]>,
}
impl FrameScratch {
    fn new(p: &Processor) -> Self {
        Self {
            windows_readers: (0..p.virtual_buses)
                .map(|_| AdaptiveReader::new(p.rate, p.buffer))
                .collect(),
            asio_readers: (0..p.virtual_buses)
                .map(|_| AdaptiveReader::new(p.rate, p.buffer))
                .collect(),
            asio_clients: vec![false; p.virtual_buses.max(1)],
            windows_render_enabled: vec![false; p.virtual_buses],
            hardware_armed: false,
            peaks: vec![0.0; p.peaks.len()],
            processed_frames: 0,
            clips: 0,
            sources: vec![0.0; p.sources.len()],
            mixed: vec![0.0; p.output_ports.len()],
            virtuals: vec![[0.0; 2]; p.virtual_buses],
            sends: vec![[0.0; 2]; p.virtual_buses],
            apps: vec![[0.0; 2]; p.applications.len()],
        }
    }
}
struct NativeState {
    scratch: FrameScratch,
    processor: Arc<Processor>,
    buffers: Vec<BufferInfo>,
    types: Vec<i32>,
    frames: usize,
    plan: Arc<Plan>,
}
unsafe impl Send for NativeState {}
static NATIVE: Mutex<Option<NativeState>> = Mutex::new(None);
unsafe extern "C" fn switch(index: i32, _: i32) {
    if !(0..2).contains(&index) {
        return;
    }
    if let Ok(mut state) = NATIVE.try_lock() {
        if let Some(s) = state.as_mut() {
            s.processor.update_plan(&mut s.plan);
            s.processor.begin(&mut s.scratch);
            for f in 0..s.frames {
                let mut input = [0.0; MAX_HARDWARE_CHANNELS];
                let mut out = [0.0; MAX_HARDWARE_CHANNELS];
                for (b, t) in s.buffers.iter().zip(&s.types) {
                    if b.input != 0 {
                        input[b.channel as usize] =
                            asio::read_sample(b.buffers[index as usize], *t, f);
                    }
                }
                s.processor.frame(
                    &input[..s.processor.physical_inputs],
                    &mut out[..s.processor.physical_outputs],
                    &s.plan,
                    &mut s.scratch,
                );
                for (b, t) in s.buffers.iter().zip(&s.types) {
                    if b.input == 0 {
                        asio::write_sample(
                            b.buffers[index as usize],
                            *t,
                            f,
                            out[b.channel as usize],
                        );
                    }
                }
            }
            s.processor.finish(&s.scratch);
        }
    }
}
unsafe extern "C" fn changed(_: f64) {
    if let Ok(s) = NATIVE.try_lock() {
        if let Some(s) = s.as_ref() {
            if let Ok(mut e) = s.processor.errors.try_lock() {
                e.push("声卡采样率改变，请停止 DAW 后重启引擎".into());
            }
        }
    }
}
unsafe extern "C" fn message(selector: i32, value: i32, _: *mut c_void, _: *mut f64) -> i32 {
    match selector {
        1 => matches!(value, 1 | 2 | 3 | 4 | 5) as i32,
        2 => 2,
        3 | 4 | 5 => {
            changed(0.0);
            1
        }
        _ => 0,
    }
}
pub struct Engine {
    native: Option<Asio>,
    streams: Vec<cpal::Stream>,
    app_captures: Vec<crate::process_capture::ProcessCapture>,
    pub system_transport: Option<crate::usb_audio::Transport>,
    pub processor: Arc<Processor>,
    pub description: String,
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.processor.deactivate();
        crate::usb_audio::log(&format!(
            "Stopping mixer: hardware {} in / {} out, raw input drops={}, raw output drops={}",
            self.processor.physical_inputs,
            self.processor.physical_outputs,
            self.processor
                .hardware
                .shared()
                .to_daw
                .dropped
                .load(Ordering::Relaxed),
            self.processor
                .hardware
                .shared()
                .from_daw
                .dropped
                .load(Ordering::Relaxed)
        ));
        self.processor
            .hardware
            .shared()
            .generation
            .store(0, Ordering::Release);
        self.system_transport = None;
        self.app_captures.clear();
        self.streams.clear();
        if let Some(d) = self.native.take() {
            d.stop();
            if let Ok(mut n) = NATIVE.lock() {
                *n = None;
            }
            drop(d);
        }
    }
}
pub fn wasapi_devices() -> (Vec<String>, Vec<String>) {
    let h = cpal::default_host();
    (
        h.input_devices()
            .map(|v| v.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default(),
        h.output_devices()
            .map(|v| v.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default(),
    )
}
fn device(input: bool, name: &str) -> Result<cpal::Device, String> {
    let h = cpal::default_host();
    let d = if name.is_empty() {
        if input {
            h.default_input_device()
        } else {
            h.default_output_device()
        }
        .ok_or("没有可用的 Windows 音频设备")?
    } else {
        let mut ds = if input {
            h.input_devices()
        } else {
            h.output_devices()
        }
        .map_err(|e| e.to_string())?;
        ds.find(|d| d.name().ok().as_deref() == Some(name))
            .ok_or_else(|| format!("设备已断开：{name}"))?
    };
    if crate::usb_audio::is_virtual(&d.name().unwrap_or_default()) {
        return Err("请选择实体声卡，不能把本软件的虚拟设备作为硬件后端".into());
    }
    Ok(d)
}
fn config(d: &cpal::Device, input: bool, rate: u32) -> Result<cpal::SupportedStreamConfig, String> {
    let values: Vec<_> = if input {
        d.supported_input_configs()
            .map_err(|e| e.to_string())?
            .collect()
    } else {
        d.supported_output_configs()
            .map_err(|e| e.to_string())?
            .collect()
    };
    let preferred = if input {
        d.default_input_config()
    } else {
        d.default_output_config()
    }
    .ok()
    .map(|c| c.channels());
    values
        .into_iter()
        .filter(|c| {
            c.channels() > 0
                && c.channels() as usize <= MAX_HARDWARE_CHANNELS
                && c.min_sample_rate().0 <= rate
                && c.max_sample_rate().0 >= rate
                && matches!(
                    c.sample_format(),
                    cpal::SampleFormat::F32 | cpal::SampleFormat::I16 | cpal::SampleFormat::U16
                )
        })
        .max_by_key(|c| {
            (
                Some(c.channels()) == preferred,
                c.sample_format() == cpal::SampleFormat::F32,
                c.channels(),
            )
        })
        .map(|c| c.with_sample_rate(cpal::SampleRate(rate)))
        .ok_or_else(|| format!("设备不支持 {rate} Hz 或所需格式"))
}
fn input_stream(
    d: &cpal::Device,
    c: &cpal::SupportedStreamConfig,
    p: Arc<Processor>,
    loopback: bool,
) -> Result<cpal::Stream, String> {
    let channels = c.channels() as usize;
    let cfg = c.clone().config();
    let ep = p.clone();
    let err = move |e| ep.fault(e);
    macro_rules! build {
        ($t:ty) => {
            d.build_input_stream(
                &cfg,
                move |data: &[$t], _: &cpal::InputCallbackInfo| {
                    use cpal::Sample;
                    for f in data.chunks_exact(channels) {
                        if loopback {
                            p.loopback.push([
                                f[0].to_sample::<f32>(),
                                f[if channels > 1 { 1 } else { 0 }].to_sample::<f32>(),
                            ]);
                        } else {
                            let mut frame = [0.0; MAX_HARDWARE_CHANNELS];
                            for (c, v) in f.iter().enumerate() {
                                frame[c] = v.to_sample::<f32>();
                            }
                            p.capture_multi.push(&frame[..channels]);
                        }
                    }
                },
                err,
                None,
            )
        };
    }
    match c.sample_format() {
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::U16 => build!(u16),
        _ => return Err("不支持的输入格式".into()),
    }
    .map_err(|e| e.to_string())
}
impl Engine {
    pub fn start(c: &Config, g: &mut Graph) -> Result<Self, String> {
        if g.bus_count == 0 {
            g.bus_count = 1;
            let (ni, no) = g.counts();
            g.reconfigure(ni, no);
        }
        if !g.valid() {
            return Err("场景路由不完整".into());
        }
        if Mapping::open(0)?.shared().engine_live() {
            return Err("请先停止另一个 Soon Link Pro 音频引擎".into());
        }
        if c.virtual_devices && g.bus_count > 0 && usbip_audio::usbip_exe::find().is_none() {
            return Err(crate::usb_audio::driver_status());
        }
        if c.backend == 1 {
            let d = asio::devices()
                .into_iter()
                .find(|d| {
                    d.name == c.asio_name
                        && d.clsid != UNIFIED_GUID
                        && !d.name.starts_with("Patchbay ASIO")
                })
                .ok_or("请选择实体 ASIO 声卡")?;
            let a = Asio::open(&d)?;
            let (ni, no) = a.channels()?;
            if ni < 0
                || no < 1
                || ni as usize > MAX_HARDWARE_CHANNELS
                || no as usize > MAX_HARDWARE_CHANNELS
            {
                return Err("声卡通道数量无效或超过每方向128通道".into());
            }
            a.set_rate(c.rate as f64)?;
            if (a.sample_rate()? - c.rate as f64).abs() > 1.0 {
                return Err("声卡采样率未切换成功".into());
            }
            let (min, max, pref, gran) = a.sizes()?;
            let size = c.buffer as i32;
            if !(size >= min
                && size <= max
                && if gran == -1 {
                    size.is_positive() && (size as u32).is_power_of_two()
                } else if gran > 0 {
                    (size - min) % gran == 0
                } else {
                    size == pref
                })
            {
                return Err(format!(
                    "声卡缓冲要求 {min}–{max}，建议 {pref} 帧，步进 {gran}"
                ));
            }
            g.reconfigure(ni as usize, no as usize);
            g.set_device_names(true, &asio_channel_names(&a, true, ni as usize, &d.name)?);
            g.set_device_names(false, &asio_channel_names(&a, false, no as usize, &d.name)?);
            let p = Processor::new(c, g, ni as usize, no as usize)?;
            let mut buffers = (0..ni)
                .map(|channel| BufferInfo {
                    input: 1,
                    channel,
                    buffers: [ptr::null_mut(); 2],
                })
                .chain((0..no).map(|channel| BufferInfo {
                    input: 0,
                    channel,
                    buffers: [ptr::null_mut(); 2],
                }))
                .collect::<Vec<_>>();
            let types = buffers
                .iter()
                .map(|b| {
                    a.channel_info(b.input != 0, b.channel)
                        .map(|c| c.sample_type)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if let Some(t) = types.iter().find(|t| !asio::supported(**t)) {
                return Err(format!("不支持声卡样本格式 {t}"));
            }
            a.create(
                &mut buffers,
                c.buffer,
                &Callbacks {
                    switch: Some(switch),
                    rate_changed: Some(changed),
                    message: Some(message),
                    time_switch: None,
                },
            )?;
            let mut apps = Vec::new();
            for (slot, t) in g.applications.iter().enumerate() {
                apps.push(crate::process_capture::ProcessCapture::start(
                    t,
                    p.clone(),
                    slot,
                )?);
            }
            *NATIVE.lock().map_err(|e| e.to_string())? = Some(NativeState {
                scratch: FrameScratch::new(&p),
                plan: p.plan(),
                processor: p.clone(),
                buffers,
                types,
                frames: c.buffer,
            });
            p.hardware
                .shared()
                .generation
                .store(clock_ms().max(1), Ordering::Release);
            if let Err(e) = a.start() {
                *NATIVE.lock().unwrap() = None;
                return Err(e);
            }
            let mut engine = Self {
                native: Some(a),
                streams: Vec::new(),
                app_captures: apps,
                system_transport: None,
                processor: p,
                description: format!(
                    "{} · 全部硬件通道 {} 入 / {} 出 · {} 帧",
                    d.name, ni, no, c.buffer
                ),
            };
            if c.virtual_devices && g.bus_count > 0 {
                engine.system_transport = Some(crate::usb_audio::Transport::start(
                    engine.processor.clone(),
                    c.buffer,
                )?);
            }
            Ok(engine)
        } else {
            let output = device(false, &c.output)?;
            let oc = config(&output, false, c.rate)?;
            let no = oc.channels() as usize;
            let input = if c.input == "（不使用输入）" {
                None
            } else {
                let d = device(true, &c.input)?;
                let cfg = config(&d, true, c.rate)?;
                Some((d, cfg))
            };
            let ni = input
                .as_ref()
                .map(|(_, c)| c.channels() as usize)
                .unwrap_or(0);
            g.reconfigure(ni, no);
            if let Some((d, _)) = &input {
                g.set_device_names(
                    true,
                    &endpoint_channel_names(&d.name().unwrap_or_else(|_| "输入设备".into()), ni),
                );
            }
            g.set_device_names(
                false,
                &endpoint_channel_names(&output.name().unwrap_or_else(|_| "输出设备".into()), no),
            );
            let p = Processor::new(c, g, ni, no)?;
            let mut streams = Vec::new();
            if let Some((d, ic)) = input {
                streams.push(input_stream(&d, &ic, p.clone(), false)?);
            }
            if c.loopback {
                streams.push(input_stream(&output, &oc, p.clone(), true)?);
            }
            let cfg = oc.clone().config();
            let pp = p.clone();
            let ep = p.clone();
            let err = move |e| ep.fault(e);
            let mut plan = p.plan();
            let mut scratch = FrameScratch::new(&p);
            macro_rules! build {
                ($t:ty) => {
                    output.build_output_stream(
                        &cfg,
                        move |data: &mut [$t], _: &cpal::OutputCallbackInfo| {
                            use cpal::Sample;
                            pp.update_plan(&mut plan);
                            pp.begin(&mut scratch);
                            for f in data.chunks_exact_mut(no) {
                                let mut input = [0.0; MAX_HARDWARE_CHANNELS];
                                pp.capture_multi.pop(&mut input[..ni]);
                                let mut out = [0.0; MAX_HARDWARE_CHANNELS];
                                pp.frame(&input[..ni], &mut out[..no], &plan, &mut scratch);
                                for (c, v) in f.iter_mut().enumerate() {
                                    *v = out[c].to_sample::<$t>();
                                }
                            }
                            pp.finish(&scratch);
                        },
                        err,
                        None,
                    )
                };
            }
            let stream = match oc.sample_format() {
                cpal::SampleFormat::F32 => build!(f32),
                cpal::SampleFormat::I16 => build!(i16),
                cpal::SampleFormat::U16 => build!(u16),
                _ => return Err("不支持的输出格式".into()),
            }
            .map_err(|e| e.to_string())?;
            streams.push(stream);
            let mut apps = Vec::new();
            for (slot, t) in g.applications.iter().enumerate() {
                apps.push(crate::process_capture::ProcessCapture::start(
                    t,
                    p.clone(),
                    slot,
                )?);
            }
            p.hardware
                .shared()
                .generation
                .store(clock_ms().max(1), Ordering::Release);
            for s in &streams {
                s.play().map_err(|e| e.to_string())?;
            }
            let mut engine = Self {
                native: None,
                streams,
                app_captures: apps,
                system_transport: None,
                processor: p,
                description: format!(
                    "{} · WASAPI {}入/{}出 · 系统缓冲",
                    output.name().unwrap_or_default(),
                    ni,
                    no
                ),
            };
            if c.virtual_devices && g.bus_count > 0 {
                engine.system_transport = Some(crate::usb_audio::Transport::start(
                    engine.processor.clone(),
                    c.buffer,
                )?);
            }
            Ok(engine)
        }
    }
}

fn endpoint_channel_names(name: &str, count: usize) -> Vec<String> {
    (0..count)
        .map(|ch| {
            if count == 1 {
                name.to_string()
            } else {
                format!(
                    "{} · {}",
                    name,
                    if count == 2 {
                        if ch == 0 {
                            "L".into()
                        } else {
                            "R".into()
                        }
                    } else {
                        format!("CH {}", ch + 1)
                    }
                )
            }
        })
        .collect()
}
fn asio_channel_names(
    a: &Asio,
    input: bool,
    count: usize,
    device: &str,
) -> Result<Vec<String>, String> {
    (0..count)
        .map(|ch| {
            let c = a.channel_info(input, ch as i32)?;
            let end = c.name.iter().position(|&c| c == 0).unwrap_or(c.name.len());
            let name = decode_asio_name(&c.name[..end]).trim().to_string();
            Ok(if name.is_empty() {
                format!(
                    "{} · {} {}",
                    device,
                    if input { "IN" } else { "OUT" },
                    ch + 1
                )
            } else {
                format!("{} · {}", device, name)
            })
        })
        .collect()
}
pub fn refresh_hardware(c: &Config, g: &mut Graph) -> Result<(), String> {
    if c.backend == 1 {
        if c.asio_name.is_empty() {
            g.reconfigure(0, 0);
            return Ok(());
        }
        let d = asio::devices()
            .into_iter()
            .find(|d| {
                d.name == c.asio_name
                    && d.clsid != UNIFIED_GUID
                    && !d.name.starts_with("Patchbay ASIO")
            })
            .ok_or("所选 ASIO 声卡未连接")?;
        let a = Asio::open(&d)?;
        let (ni, no) = a.channels()?;
        if ni < 0
            || no < 1
            || ni as usize > MAX_HARDWARE_CHANNELS
            || no as usize > MAX_HARDWARE_CHANNELS
        {
            return Err("声卡返回的通道布局无效".into());
        }
        g.reconfigure(ni as usize, no as usize);
        g.set_device_names(true, &asio_channel_names(&a, true, ni as usize, &d.name)?);
        g.set_device_names(false, &asio_channel_names(&a, false, no as usize, &d.name)?);
    } else {
        let output = device(false, &c.output)?;
        let no = config(&output, false, c.rate)?.channels() as usize;
        let input = if c.input == "（不使用输入）" {
            None
        } else {
            let d = device(true, &c.input)?;
            let ni = config(&d, true, c.rate)?.channels() as usize;
            Some((d, ni))
        };
        let ni = input.as_ref().map(|(_, n)| *n).unwrap_or(0);
        g.reconfigure(ni, no);
        if let Some((d, n)) = input {
            g.set_device_names(
                true,
                &endpoint_channel_names(&d.name().unwrap_or_default(), n),
            );
        }
        g.set_device_names(
            false,
            &endpoint_channel_names(&output.name().unwrap_or_default(), no),
        );
    }
    Ok(())
}

fn decode_asio_name(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn MultiByteToWideChar(
            codepage: u32,
            flags: u32,
            bytes: *const u8,
            count: i32,
            out: *mut u16,
            length: i32,
        ) -> i32;
    }
    unsafe {
        let count = MultiByteToWideChar(
            0,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        );
        if count > 0 {
            let mut wide = vec![0u16; count as usize];
            MultiByteToWideChar(
                0,
                0,
                bytes.as_ptr(),
                bytes.len() as i32,
                wide.as_mut_ptr(),
                count,
            );
            String::from_utf16_lossy(&wide)
        } else {
            String::from_utf8_lossy(bytes).to_string()
        }
    }
}

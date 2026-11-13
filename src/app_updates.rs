use eframe::egui::{self, Context};
use semver::Version;
use serde::Deserialize;
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub const RELEASE_URL: &str = "https://github.com/akamstsu/Soon-Link-Pro/releases/tag/dev";
const API_URL: &str = "https://api.github.com/repos/akamstsu/Soon-Link-Pro/releases/tags/dev";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    state: String,
    size: u64,
}

fn published_version(bytes: &[u8]) -> Result<Version, String> {
    let release: Release =
        serde_json::from_slice(bytes).map_err(|_| "更新信息格式不正确，请稍后重试。".to_owned())?;
    if release.draft || release.tag_name != "dev" {
        return Err("未找到可用的开发版发布。".into());
    }
    release
        .assets
        .iter()
        .filter(|asset| asset.state == "uploaded" && asset.size > 0)
        .filter_map(|asset| {
            asset
                .name
                .strip_prefix("Soon-Link-Pro-R")?
                .strip_suffix("-Setup-x64.exe")?
                .parse::<Version>()
                .ok()
        })
        .max()
        .ok_or_else(|| "新版安装包尚未就绪，请稍后重试。".into())
}

fn fetch() -> Result<Version, String> {
    // Network and child-process work stays on this worker, never the UI/audio threads.
    let script = r#"$ErrorActionPreference='Stop';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
try {
 [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12
 $r=Invoke-WebRequest -UseBasicParsing -Uri $env:SOON_LINK_UPDATE_API -Headers @{'User-Agent'='Soon-Link-Pro';'Accept'='application/vnd.github+json'} -TimeoutSec 15
 [Console]::Write($r.Content)
} catch { exit 1 }"#;
    let output = crate::driver_tools::shell(script)
        .env("SOON_LINK_UPDATE_API", API_URL)
        .output()
        .map_err(|_| "无法启动版本检查，请手动访问发布页面。".to_owned())?;
    if !output.status.success() {
        return Err("暂时无法连接 GitHub，请检查网络后重试。".into());
    }
    published_version(&output.stdout)
}

#[derive(Default)]
pub struct Updates {
    started: bool,
    pending: Option<Receiver<Result<Version, String>>>,
    latest: Option<Version>,
    message: String,
}
impl Updates {
    pub fn check(&mut self, ctx: &Context) {
        if self.pending.is_some() {
            return;
        }
        self.started = true;
        let (send, receive) = mpsc::channel();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("app-update-check".into())
            .spawn(move || {
                let _ = send.send(fetch());
                ctx.request_repaint();
            }) {
            Ok(_) => {
                self.pending = Some(receive);
                self.message = "正在检查新版本…".into();
            }
            Err(_) => self.message = "无法启动版本检查，请稍后重试。".into(),
        }
    }

    pub fn poll(&mut self, ctx: &Context) {
        if !self.started {
            self.check(ctx);
        }
        let result = match self.pending.as_ref().map(|receive| receive.try_recv()) {
            Some(Ok(result)) => result,
            Some(Err(TryRecvError::Disconnected)) => Err("版本检查已结束，请重试。".into()),
            _ => return,
        };
        self.pending = None;
        match result {
            Ok(version) => {
                let current = Version::parse(CURRENT_VERSION).expect("Cargo package version");
                self.message = match version.cmp(&current) {
                    std::cmp::Ordering::Greater => format!("发现新版本 {version}"),
                    std::cmp::Ordering::Equal => format!("当前已是最新版本 {CURRENT_VERSION}"),
                    std::cmp::Ordering::Less => format!("本机版本高于已发布版本 {version}"),
                };
                self.latest = Some(version);
            }
            Err(message) => self.message = message,
        }
    }

    pub fn available(&self) -> bool {
        self.latest.as_ref().is_some_and(|latest| {
            Version::parse(CURRENT_VERSION).is_ok_and(|current| *latest > current)
        })
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.pending.is_none(), egui::Button::new("检查新版本"))
                .clicked()
            {
                self.check(ui.ctx());
            }
            if self.pending.is_some() {
                ui.spinner();
            }
            ui.label(&self.message);
            ui.hyperlink_to(
                if self.available() {
                    "下载新版本"
                } else {
                    "查看发布页面"
                },
                RELEASE_URL,
            );
        });
        ui.small("启动时后台检查一次；仅检查版本，由你选择是否下载和安装。");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn payload(assets: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"tag_name":"dev","draft":false,"assets":assets}))
            .unwrap()
    }
    fn installer(version: &str) -> serde_json::Value {
        serde_json::json!({"name":format!("Soon-Link-Pro-R{version}-Setup-x64.exe"),"state":"uploaded","size":100})
    }
    #[test]
    fn rolling_tag_uses_installer_version_and_semver_order() {
        let data = payload(serde_json::json!([
            installer("2.4.9-dev"),
            installer("2.4.10-dev")
        ]));
        assert_eq!(
            published_version(&data).unwrap(),
            Version::parse("2.4.10-dev").unwrap()
        );
        assert!(Version::parse("2.4.10").unwrap() > Version::parse("2.4.10-dev").unwrap());
        assert!(Version::parse("2.4.10-dev.10").unwrap() > Version::parse("2.4.10-dev.2").unwrap());
    }
    #[test]
    fn incomplete_or_unrelated_assets_are_not_updates() {
        for assets in [
            serde_json::json!([]),
            serde_json::json!([{"name":"Soon-Link-Pro-R3.0.0-dev-Setup-x64.exe","state":"new","size":100}]),
            serde_json::json!([{"name":"Soon-Link-Pro-R3.0.0-dev-Setup-x64.exe","state":"uploaded","size":0}]),
            serde_json::json!([{"name":"unrelated.exe","state":"uploaded","size":100}]),
        ] {
            assert!(published_version(&payload(assets)).is_err());
        }
        assert!(published_version(b"not json").is_err());
    }
    #[test]
    fn draft_and_wrong_tags_are_rejected() {
        for (draft, tag) in [(true, "dev"), (false, "other")] {
            let bytes = serde_json::to_vec(&serde_json::json!({"draft":draft,"tag_name":tag,"assets":[installer("9.0.0-dev")]})).unwrap();
            assert!(published_version(&bytes).is_err());
        }
    }
    #[test]
    fn only_newer_versions_show_the_update_indicator() {
        let current = Version::parse(CURRENT_VERSION).unwrap();
        let mut updates = Updates::default();
        updates.latest = Some(current.clone());
        assert!(!updates.available());
        updates.latest = Some(Version::new(0, 0, 1));
        assert!(!updates.available());
        updates.latest = Some(Version::new(current.major + 1, 0, 0));
        assert!(updates.available());
    }
}

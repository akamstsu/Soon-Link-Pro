use serde::Deserialize;
use std::{
    os::windows::process::CommandExt,
    path::PathBuf,
    process::Command,
    sync::mpsc::{self, Receiver},
};

pub const BUNDLED_VERSION: &str = "0.9.8.1";
pub const RELEASES_URL: &str = "https://github.com/vadimgrn/usbip-win2/releases";
const INSTALLER: &str = "usbip-win2-0.9.8.1-x64.exe";
#[derive(Deserialize)]
pub struct Release {
    pub tag: String,
    pub url: String,
    pub installed: String,
}
impl Release {
    pub fn summary(&self) -> String {
        let version = |s: &str| {
            let parts: Vec<u32> = s
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty())
                .take(4)
                .filter_map(|s| s.parse().ok())
                .collect();
            (!parts.is_empty()).then(|| {
                [
                    *parts.first().unwrap_or(&0),
                    *parts.get(1).unwrap_or(&0),
                    *parts.get(2).unwrap_or(&0),
                    *parts.get(3).unwrap_or(&0),
                ]
            })
        };
        let state = match (version(&self.installed), version(&self.tag)) {
            (Some(current), Some(latest)) if current < latest => "发现可用的客户端更新",
            (Some(current), Some(latest)) if current == latest => "客户端已是当前稳定版",
            (Some(_), Some(_)) => "本机客户端版本高于当前稳定版",
            _ => "请安装驱动或查看官方更新页面",
        };
        format!(
            "{}\n官方稳定版：{} · 已安装客户端：{} · 随包版本：{}",
            state,
            self.tag,
            if self.installed.is_empty() {
                "未检测到"
            } else {
                &self.installed
            },
            BUNDLED_VERSION
        )
    }
}
pub(crate) fn shell(script: &str) -> Command {
    let mut command = Command::new("powershell.exe");
    command
        .creation_flags(0x08000000)
        .args(["-NoProfile", "-NonInteractive", "-Command", script]);
    command
}
pub fn bundled_path() -> PathBuf {
    std::env::current_exe()
        .unwrap_or_default()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("drivers")
        .join(INSTALLER)
}
pub fn check_updates() -> Receiver<Result<Release, String>> {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let script = r#"$ErrorActionPreference='Stop';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
try {
 $installed=''
 if($env:SOON_LINK_USBIP_EXE -and (Test-Path -LiteralPath $env:SOON_LINK_USBIP_EXE)) { $installed=(Get-Item -LiteralPath $env:SOON_LINK_USBIP_EXE).VersionInfo.FileVersion }
 $r=Invoke-RestMethod -Uri 'https://api.github.com/repos/vadimgrn/usbip-win2/releases/latest' -Headers @{'User-Agent'='Soon-Link-Pro';'Accept'='application/vnd.github+json'} -TimeoutSec 15
 if($r.draft -or $r.prerelease -or -not $r.tag_name) {throw 'No stable release was returned'}
 @{tag=[string]$r.tag_name;url=[string]$r.html_url;installed=[string]$installed} | ConvertTo-Json -Compress
} catch { [Console]::Error.WriteLine($_.Exception.Message);exit 1 }"#;
        let result = (|| {
            let output = shell(script)
                .env(
                    "SOON_LINK_USBIP_EXE",
                    usbip_audio::usbip_exe::find().unwrap_or_default(),
                )
                .output()
                .map_err(|e| format!("无法检查更新：{e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "检查驱动更新失败：{}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            let release: Release = serde_json::from_slice(&output.stdout)
                .map_err(|e| format!("更新信息解析失败：{e}"))?;
            if !release
                .url
                .starts_with("https://github.com/vadimgrn/usbip-win2/releases/")
            {
                return Err("更新来源校验失败".into());
            }
            Ok(release)
        })();
        let _ = send.send(result);
    });
    receive
}
pub fn install() -> Receiver<Result<String, String>> {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let script = r#"$ErrorActionPreference='Stop';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
try {
 $p=$env:SOON_LINK_DRIVER_INSTALLER
 if(-not (Test-Path -LiteralPath $p -PathType Leaf)) {throw 'Bundled driver installer is missing; extract the complete application package'}
 $hash=(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash
 if($hash -ne '38cad6d4432b52d5bb9409d9ad03b72fdffc4ada4cd3a48fbeca1a2752a8518a') {throw 'Bundled installer SHA256 does not match'}
 $signature=Get-AuthenticodeSignature -LiteralPath $p
 if($signature.Status -ne 'Valid') {throw ('Installer signature is not valid: '+$signature.Status)}
 $process=Start-Process -FilePath $p -Verb RunAs -Wait -PassThru
 if($process.ExitCode -eq 3010 -or $process.ExitCode -eq 1641) {Write-Output '驱动安装已完成，安装器提示需要重启 Windows。'}
 elseif($process.ExitCode -eq 0) {Write-Output '安装器已退出；请刷新设备列表。'}
 else {throw ('Installer exited with code '+$process.ExitCode)}
} catch { [Console]::Error.WriteLine($_.Exception.Message);exit 1 }"#;
        let result = (|| {
            let output = shell(script)
                .env("SOON_LINK_DRIVER_INSTALLER", bundled_path())
                .output()
                .map_err(|e| format!("无法运行驱动安装器：{e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "驱动安装未完成：{}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        })();
        let _ = send.send(result);
    });
    receive
}
#[derive(Default)]
pub struct Tools {
    pub pending_install: bool,
    pub resume_audio: bool,
    pub install: Option<Receiver<Result<String, String>>>,
    pub update: Option<Receiver<Result<Release, String>>>,
    pub release: Option<Release>,
    pub update_status: String,
}
impl Tools {
    pub fn busy(&self) -> bool {
        self.pending_install || self.install.is_some()
    }
}

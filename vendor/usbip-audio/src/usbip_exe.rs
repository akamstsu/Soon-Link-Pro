//! Attaching and detaching with the installed usbip-win2 client (`usbip.exe`).
//!
//! Applications sharing the driver must only detach the ports they attached: never `detach --all`.

#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
fn command(path: &std::path::Path) -> Command {
    let mut cmd = Command::new(path);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    cmd
}

fn run(command: &mut Command) -> Result<Output, String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| e.to_string()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("usbip.exe timed out after 15 seconds".into());
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
    }
}

/// The usbip-win2 client: `USBIP_EXE` if set, else its default install locations.
pub fn find() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("USBIP_EXE") {
        return Some(path.into());
    }
    ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|base| PathBuf::from(base).join("USBip").join("usbip.exe"))
        .find(|path| path.is_file())
}

/// Attaches `busid` of the server on `127.0.0.1:port`; returns the usbip-win2 port it was attached to.
pub fn attach(usbip: &std::path::Path, port: u16, busid: &str) -> Result<u32, String> {
    let output = run(command(usbip)
        .args([
            "--tcp-port",
            &port.to_string(),
            "attach",
            "--remote",
            "127.0.0.1",
            "--bus-id",
            busid,
        ])
        .args(["--once", "--terse"]))
    .map_err(|e| format!("could not run {}: {e}", usbip.display()))?;
    if !output.status.success() {
        return Err(format!(
            "usbip attach failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim()
        .parse()
        .map_err(|_| format!("usbip attach returned no port: {text:?}"))
}

/// Detaches a port this application attached.
pub fn detach(usbip: &std::path::Path, port: u32) -> Result<(), String> {
    let output = run(command(usbip).args(["detach", "--port", &port.to_string()]))
        .map_err(|e| format!("could not run {}: {e}", usbip.display()))?;
    output.status.success().then_some(()).ok_or_else(|| {
        format!(
            "usbip detach of port {port} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

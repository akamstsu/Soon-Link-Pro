use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon.rc");
    println!("cargo:rerun-if-changed=assets/soon-link-pro-icon-v2.ico");
    println!("cargo:rerun-if-env-changed=RC");
    println!("cargo:rerun-if-env-changed=WindowsSdkDir");
    println!("cargo:rerun-if-env-changed=WindowsSDKVersion");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }
    let rc = env::var_os("RC")
        .map(PathBuf::from)
        .or_else(|| {
            let sdk = env::var_os("WindowsSdkDir")
                .map(PathBuf::from)
                .or_else(|| {
                    env::var_os("ProgramFiles(x86)")
                        .map(|p| PathBuf::from(p).join("Windows Kits/10"))
                })?;
            let mut versions: Vec<_> = std::fs::read_dir(sdk.join("bin"))
                .ok()?
                .filter_map(Result::ok)
                .map(|entry| entry.path().join("x64/rc.exe"))
                .filter(|path| path.is_file())
                .collect();
            versions.sort();
            versions.pop()
        })
        .unwrap_or_else(|| PathBuf::from("rc.exe"));
    let resource = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("soon-link-pro-icon.res");
    let status = Command::new(rc)
        .current_dir(PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("assets"))
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg("app-icon.rc")
        .status()
        .expect("Windows SDK resource compiler is required to embed the application icon");
    assert!(
        status.success(),
        "Failed to compile the application icon resource"
    );
    println!(
        "cargo:rustc-link-arg-bin=SoonLinkPro={}",
        resource.display()
    );
}

# 自动构建与发布

工作流：`.github/workflows/windows-build.yml`。

| 触发方式 | 行为 |
| --- | --- |
| 推送 `main`、向 `main` 提 PR | 格式检查、工作区测试、发布模式构建、生成安装包 |
| Actions 页面手动运行 | 同样生成安装包，不创建 Release |
| 推送 `v<版本号>` 标签 | 构建成功后上传安装包和 SHA256 到 GitHub Release |

普通构建在 Actions 的 `Soon-Link-Pro-Windows-x64` artifact 中保留 30 天。
只有标签发布任务获得仓库写入权限，PR 构建只读，无需额外访问令牌或签名密钥。
Action 引用固定到 commit，下载工具固定版本与 SHA256，Cargo 使用锁文件。

## 工具和版本

Windows Server 2022 runner 使用 Rust stable MSVC；构建时记录 Rust 版本。
安装工具固定为 Inno Setup 6.4.3 和原厂 USBip 0.9.8.1 x64，校验后只运行 Inno
编译器安装程序；不在 CI 中安装 USB/IP 驱动、注册 ASIO 或执行音频设备测试。
应用及本项目 ASIO DLL 尚无代码签名，原厂驱动签名保持原样。

版本来自根目录 Cargo.toml；安装器的数字版本由其去掉预发布后缀得到。
标签必须严格等于 `v` 加根包版本。带 `-dev` 等后缀的版本标记为 prerelease。
发布前同步主程序、`driver/Cargo.toml`、关于页和更新说明中的显示版本。

## 本地复现

在 PowerShell 7 中执行：

```powershell
.\scripts\fetch-build-tools.ps1
# 此脚本只下载和校验；自行安装 Inno Setup 6.4.3 或使用已有编译器。
.\scripts\build.ps1 -MakeInstaller `
  -InnoCompiler 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe' `
  -DriverInstaller '.\build\tools\usbip-win2-0.9.8.1-x64.exe'
```

产物在 `dist`；`build/payload` 仅暂存运行文件、驱动、使用说明与许可证。
安装包和 Actions artifact 不包含项目源码、用户配置或调试文件。
GitHub 为源码仓库自动生成的 Source code 下载属于源码分发，独立于安装包。

## 发布前验收

1. 在目标 Windows 上安装、升级、卸载，确认配置保留。
2. DAW 加载、音频往返、关闭和重开、托盘退出与宿主生命周期。
3. 硬件／虚拟总线／应用的接线、备注、静音、隐藏、场景恢复。
4. 中文长名称、矩阵、窗口缩放与动画开关。
5. 用实际设备测量延迟和持续运行稳定性。

GitHub 工作流须在首次上传后运行，才能确认托管 runner 和网络下载实际成功。
仅本地打包通过时，不应写成 GitHub Actions 已运行成功。

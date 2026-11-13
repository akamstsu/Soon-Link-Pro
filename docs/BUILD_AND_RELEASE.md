# 自动构建与发布

工作流：`.github/workflows/windows-build.yml`。

| 触发方式 | 行为 |
| --- | --- |
| 推送 `main` | 检查、测试、构建成功后为新版本创建独立标签和 Release |
| 向 `main` 提 PR | 格式检查、工作区测试、发布模式构建、生成测试安装包 |
| Actions 页面手动运行 | 同样生成安装包，不创建 Release |
| 推送 `v<版本号>` 标签 | 构建成功后创建对应版本的 Release |

普通构建在 Actions 的 `Soon-Link-Pro-Windows-x64` artifact 中保留 30 天。
只有发布任务获得仓库写入权限，PR 构建只读，无需额外访问令牌或签名密钥。
Action 引用固定到 commit，下载工具固定版本与 SHA256，Cargo 使用锁文件。

## 工具和版本

Windows Server 2022 runner 使用 Rust stable MSVC；构建时记录 Rust 版本。
安装工具固定为 Inno Setup 6.4.3 和原厂 USBip 0.9.8.1 x64，校验后只运行 Inno
编译器安装程序；不在 CI 中安装 USB/IP 驱动、注册 ASIO 或执行音频设备测试。
应用及本项目 ASIO DLL 尚无代码签名，原厂驱动签名保持原样。

版本来自根目录 Cargo.toml；安装器的数字版本由其去掉预发布后缀得到。
每个版本使用严格匹配根包版本的 `v<版本号>` 标签，并在 Releases 中独立显示、保留历史安装包。
开发阶段保留 `-dev` 版本后缀，但 GitHub 发布不使用 prerelease 状态，设为 Latest，
让仓库首页的 Releases 区域直接展示最新版本。这只是发布页的展示规则，不代表完成设备验收。
不使用固定 `dev` 发布项。

日常发布先增加版本号，再推送 `main`。CI 先测试与构建，再创建该版本的新标签和 Release；
CI 使用自带令牌创建标签，不会再次触发标签构建。已存在的标签不移动、附件不覆盖。
相同版本再次推送仅生成测试 artifact；若同一提交的发布失败，可重跑工作流恢复。
本地 commit 不触发发布，推送才触发；需要先审阅或先测试时，应在推送前完成。
PR 和 Actions 页面手动构建只产生测试 artifact。

发布前同步主程序、`driver/Cargo.toml`、关于页和更新说明中的版本。
软件更新检测读取 Releases 列表，按语义版本排序，包含开发版；只采用标签版本一致且已上传的安装包。
旧 `2.4.7-dev` 使用已停用的固定 `dev` 检测地址，需要手动升级一次到 `2.4.8-dev` 或更高版本。

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

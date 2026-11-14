# Soon Link Pro

Windows 音频路由工作台，连接实体声卡、DAW、Windows 虚拟设备与应用声音。

版权方：挚彬Club；作者：赤松_Akamatsu。

## 功能

- 一个 DAW ASIO 入口，前段转发所选物理声卡输入和输出，后段提供虚拟总线。
- 每组虚拟总线提供 Windows 可识别的播放和录音设备；默认至少一组，可按需增加。
- 跳线面板按声道接线，立体声通道组提供联动音量、整组静音、显示选择和内部备注，显示为 `原名 (备注)`。
- 跳线面板显示流向和目标色渐变；紧凑路由矩阵按单声道交叉连接。
- 应用声音采集、场景保存、DAW 启动时自动运行、托盘控制和宿主生命周期跟随。
- 启动时后台检查新版本；在“关于与帮助”手动检查并打开发布页下载。
- 本机 MCP 服务：Agent 可读取通道、批量跳线、调整音量与静音；设置中启用并复制连接配置。

## 快速使用

安装配套 x64 安装包，打开软件选择实体 ASIO 声卡，或使用 WASAPI。
默认自动启动音频，保留至少一组虚拟输入与输出。按需要添加总线或应用，
从左侧输入端口连接到右侧输出端口；同一来源可以连接多个目标。
DAW 中选择 `Soon Link Pro ASIO`，其前段为物理声卡通道，后段为虚拟总线。

电平条同时用于调音量，L/R 默认联动；扬声器图标控制整个通道组。
点击组标题设置备注，输入和输出的备注独立，仅用于软件内显示。
矩阵点击方格切换连接，右键或双击调整连接；悬停查看完整名称。
关闭窗口收进托盘；退出请用托盘菜单。更新前保存并退出 DAW，再退出软件。

详细使用与配置位置见 [WINDOWS_AUDIO.md](WINDOWS_AUDIO.md)。
Agent 接入、工具参数和跳线示例见 [MCP 使用说明](docs/MCP.md)。


## 环境

- Windows x64，Rust stable 的 `x86_64-pc-windows-msvc` 工具链。
- Visual Studio C++ Build Tools 与 Windows SDK（需要链接器和 `rc.exe`）。
- 虚拟 Windows 音频设备使用 usbip-win2；安装驱动需要管理员权限。
- 应用回录需要系统提供进程回录 API；物理 ASIO 设备应安装对应厂商驱动。

## 编译

在仓库目录执行：

```powershell
cargo build --locked --release --workspace
```

也可运行：

```powershell
.\scripts\build.ps1
```

输出为 `target\release\SoonLinkPro.exe`、`SoonLinkProMcp.exe` 和 `patchbay_asio.dll`。
三个文件配套放置；MCP 入口不需要额外安装 Python 或 Node.js。
DLL 文件名和注册标识作为安装接口保持稳定。
仓库不包含用户配置、驱动二进制、编译缓存或发布压缩包。

## 安装包

准备 Inno Setup 6 与 usbip-win2 0.9.8.1 x64 原厂安装器，然后执行：

```powershell
.\scripts\build.ps1 -MakeInstaller `
  -InnoCompiler 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe' `
  -DriverInstaller 'D:\Downloads\usbip-win2-0.9.8.1-x64.exe'
```

脚本校验驱动 SHA256，暂存运行文件到 `build\payload`，安装包输出到 `dist`。
只有运行安装包才会注册 ASIO 和安装驱动；编译脚本不安装它们。
更新安装保留 `%LOCALAPPDATA%\SoonLinkPro` 下的配置、场景和备注。
打包脚本不把源码放入安装包。

## 代码结构

| 目录 | 内容 |
| --- | --- |
| `src` | 界面、路由引擎、声卡连接、应用采集与托盘 |
| `bridge` | 共享内存协议、音频队列和生命周期通信 |
| `driver` | ASIO COM DLL |
| `vendor/usbip-audio` | 固定版本的 USB/IP 音频实现及上游许可证 |
| `assets` | 图标、挚彬Club Logo 和第三方声明 |
| `installer` | Inno Setup 安装脚本及中文语言文件 |
| `docs` | 接口资料与实现约定 |

## 许可证与第三方组件

第三方组件的版权和许可见 [THIRD_PARTY_NOTICES.md](assets/THIRD_PARTY_NOTICES.md)、
`assets/upstream-licenses` 和 `vendor/usbip-audio/LICENSE`。
本项目许可证见 [LICENSE](LICENSE)。第三方组件继续遵循各自许可证；项目品牌与 Logo 不表示对衍生产品的背书。
ASIO 是 Steinberg 的商标，Windows 是 Microsoft 的商标；名称仅用于接口说明。

## 参与开发与自动构建

请阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [AGENTS.md](AGENTS.md)。
推送 `main` 后，测试与构建通过就会为新版本自动创建 `v<版本号>` 标签和独立 Release。
开发版保留 `-dev` 后缀；GitHub 发布设为 Latest，让首页 Releases 直接显示最新版本。
各版本与安装包分别保留，不覆盖历史发布。
相同版本号再次推送只生成测试安装包；PR 和手动运行也只构建测试包。
下载校验、产物和触发规则见
[BUILD_AND_RELEASE.md](docs/BUILD_AND_RELEASE.md)。安装包及构建 artifact 不包含源码。
本项目 EXE 和 ASIO DLL 尚未签名，随包 USB/IP 驱动保留原厂签名。

## 当前状态

当前为 `2.4.8-dev` 测试版。自动构建结果可在仓库 Actions 页面查看。

编译完成不等于通过 DAW、安装、听音、延迟或 Live 验收。实际使用前需要在目标设备验证。

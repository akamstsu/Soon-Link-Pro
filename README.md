# Soon Link Pro

Windows 音频路由工作台。版权方：挚彬Club；作者：赤松_Akamatsu。

官网：https://www.zhibin.club

## 功能

- 一个 DAW ASIO 入口，前段转发所选物理声卡输入和输出，后段提供虚拟总线。
- 每组虚拟总线提供 Windows 可识别的播放和录音设备；默认至少一组，可按需增加。
- 跳线面板按声道接线，立体声卡片提供联动音量、整卡静音、显示选择和内部备注。
- 应用声音采集、场景保存、DAW 启动时自动运行、托盘控制和宿主生命周期跟随。

使用说明见 [WINDOWS_AUDIO.md](WINDOWS_AUDIO.md)。

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

输出为 `target\release\SoonLinkPro.exe` 和 `target\release\patchbay_asio.dll`。
两者必须配套放置；DLL 文件名和注册标识作为安装接口保持稳定。
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

## 许可与验证状态

第三方组件的版权和许可见 [THIRD_PARTY_NOTICES.md](assets/THIRD_PARTY_NOTICES.md)、
`assets/upstream-licenses` 和 `vendor/usbip-audio/LICENSE`。
主项目发布许可证尚待版权方选择；第三方许可不自动授予主项目的许可。

编译完成不等于通过 DAW、安装、听音、延迟或 Live 验收。实际使用前需要在目标设备验证。

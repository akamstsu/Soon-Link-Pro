# 开发者与编码代理指南

本文件适用于整个仓库，帮助贡献者定位代码、保持音频与配置兼容，并选择合适的验证方式。
开始前阅读 [README.md](README.md)、[CONTRIBUTING.md](CONTRIBUTING.md)，再阅读改动涉及的模块与资料。
任务中的明确要求优先；以下约定应结合实际代码使用，不要据此扩大任务范围。

## 项目与构建环境

Soon Link Pro 是 Windows x64 桌面音频路由工具，界面使用 Rust / eframe / egui，
连接物理 ASIO 或 WASAPI、DAW ASIO、Windows 虚拟设备与应用声音。

需要 Rust stable MSVC、Visual Studio C++ Build Tools、Windows SDK，以及更新依赖声明时使用的 Python 3.11+。
在仓库根目录执行：

```powershell
cargo fmt --all -- --check
cargo test --locked --workspace
python scripts/update-notices.py --check
cargo build --locked --release --workspace
```

发布模式产物为 `target/release/SoonLinkPro.exe` 和 `target/release/patchbay_asio.dll`。
工作区包括主程序、`bridge`、`driver` 和 vendored USB/IP 音频库；改动公共接口时检查所有使用方。
请保留并提交 Cargo.lock；依赖升级应是有意的改动，不要顺带刷新整个锁文件。

## 模块导航

| 文件或目录 | 职责与修改入口 |
| --- | --- |
| `src/main.rs` | 应用状态、配置与场景、跳线面板、矩阵、通道管理、设置及备注对话框 |
| `src/ui_shell.rs` | 主题、布局辅助、图标、连线与信号动效绘制 |
| `src/widgets.rs` | 按钮、推子、电平等共用控件 |
| `src/about.rs` | 关于窗口、项目与第三方组件信息 |
| `src/audio.rs` | `Graph` / `Config`、配置迁移、路由计划、`Processor` 与 `Engine`、ASIO / WASAPI 流 |
| `src/engine_control.rs` | 音频控制线程、启动/停止命令、异步事件与 revision 匹配 |
| `src/asio.rs` | 物理 ASIO 设备枚举、COM 接口调用、缓冲和样本格式转换 |
| `src/process_capture.rs` | 应用清单、进程回录、异步激活、采集缓冲与格式转换 |
| `src/usb_audio.rs` | Windows 虚拟音频设备、总线命名、USB/IP 音频传输 |
| `src/runtime.rs` | 单实例、托盘、窗口显隐和宿主生命周期协调 |
| `src/driver_tools.rs` | 配套驱动安装器定位、版本与校验 |
| `bridge/src/lib.rs` | EXE 与 DLL 的共享内存、虚拟总线 SPSC 队列与音量换算 |
| `bridge/src/config.rs` | 公共容量、默认参数与协议布局常量 |
| `bridge/src/hardware.rs` | 物理通道共享内存与多通道队列 |
| `bridge/src/lifecycle.rs` | 宿主登记、心跳、实例所有权与唤起请求 |
| `driver/src/lib.rs` | DAW 使用的 ASIO COM DLL、缓冲回调、注册及自动启动主程序 |
| `vendor/usbip-audio` | 固定上游版本的 USB/IP 音频实现；修改说明见 `UPSTREAM.md` |
| `installer`、`scripts`、`.github/workflows` | 安装、打包、依赖声明更新与自动构建 |

音频与队列设计见 [AUDIO_DESIGN.md](docs/AUDIO_DESIGN.md)，Windows 接口资料见
[WINDOWS_INTERFACES.md](docs/WINDOWS_INTERFACES.md)，使用和配置说明见 [WINDOWS_AUDIO.md](WINDOWS_AUDIO.md)。

## 音频路径与线程边界

界面修改 `Graph`，由 `Processor::sync` 合并路由和增益形成 `Plan`；音频处理使用已准备好的计划。
引擎启动、停止、设备打开与释放由 `Controller` 的控制线程管理。ASIO COM 对象保留在其所属线程，
异步完成事件按 revision 判断是否仍然有效，避免快速切换设备后被旧结果覆盖。

- 回调中不增加堆分配、阻塞等待、日志、文件或网络访问、设备枚举及界面调用。
- 当前计划切换使用 `try_lock`，拿不到新计划时继续使用旧计划；不要改为等待锁。
- 替换的计划由控制侧保留，避免音频线程释放最后一个 `Arc` 时析构向量。
- 临时音频缓冲在启动阶段准备；每个样本只进行必要的读取、转换、乘加与写入。
- 修改停止流程时同时检查回调结束、采集客户端停止、工作线程退出和共享资源释放顺序。

每个 SPSC 队列仅允许一个生产者和一个消费者。样本写完后以 Release 发布写游标，消费者以 Acquire
读取；读游标对称发布。保留游标环绕、非法跨度和整块准入检查，不要把容量误当作目标延迟。
清空队列须由消费者执行，或先确认相关消费者已经停止。

应用回录的非空缓冲必须归还，包括转换失败与提前返回路径；保留 RAII 释放语义和异步激活参数的存活期。
调整 COM、FFI、裸指针或共享内存时，说明所有权、线程、长度、对齐及销毁条件，并增加对应的回归验证。

## 配置、场景与协议兼容

用户配置通常位于 `%LOCALAPPDATA%/SoonLinkPro/soon-link-pro-settings.json`。
路径选择、旧便携版配置回退和保存逻辑在 `src/main.rs`，图结构迁移在 `Graph::from_value` 等方法中。
不要用开发者自己的配置覆盖用户配置，也不要把本地配置提交到仓库。

- 通道的稳定 key 用于恢复路由；显示名称不是标识。重新枚举设备或调整通道时，保留可匹配的路由、
  推子、静音、隐藏状态、备注和场景。
- `card_names` 是已保存 JSON 的兼容键。内部函数和类型中仍有 `card` 命名，修改用户文案无需连带重命名。
- 通道组备注按输入、输出独立保存，显示为 `原名 (备注)`；清空备注恢复原名。
  备注仅用于软件显示，不写入 Windows 设备属性、USB 描述符或 DAW 名称，也不应触发音频重启。
- 隐藏通道组只影响显示，已有接线与声音继续保留。至少保留一组虚拟输入与输出。
- 推子保存值为 0..1；0 表示静音，其余按现有 -40..0 dB 刻度换算。改变刻度需要明确的兼容策略。
- EXE 与 ASIO DLL 必须配套。DLL 文件名、CLSID、注册路径、共享映射名称和协议标识都是兼容接口，
  不能仅为代码整洁而更改。
- 改变 `repr(C)` 结构或布局常量时，更新两端协议标识及相应使用方，明确旧版与新版如何共存或拒绝连接。

配置迁移测试应使用可读的最小旧配置，验证数据经过加载、修改和再次保存后仍然正确，
特别是输入/输出备注独立、路由索引、隐藏通道和空虚拟总线的处理。

## 界面与绘制

优先复用 `ui_shell` 的颜色、间距和图标，以及 `widgets` 的交互控件，避免同类控件各自维护样式。
界面遍历、设备扫描和图形绘制留在各自线程，音频回调不参与界面工作。

- 长名称在受限区域单行省略，悬停提供全称；中文、备注、换行输入和极长应用名都需要考虑。
- 矩阵保持紧凑单元格、统一内边距和完整面板底色；增加通道时通过滚动容纳，避免把列平均拉宽。
  修改标签时同步检查旋转列标题、裁剪范围、悬停区域和点击命中区域。
- 自定义 mesh 不会自动获得 egui 路径的边缘羽化；修改连线时保留物理像素级抗锯齿，考虑 DPI 与曲率。
- 连线颜色和动效使用同一曲线；到达目标及循环重启时平滑衔接。遵循动画开关、窗口隐藏与重绘节流逻辑。
- 检查小窗口、常见缩放比例、很多通道和空状态。源码编译成功不能代替这些视觉检查。

## 如何选择验证

只运行与改动有关的有效验证，再执行提交所需的基础检查；没有新增改动或问题时不反复运行同一轮检查。

| 改动范围 | 重点验证 |
| --- | --- |
| 纯文档、文案 | 链接、命令、命名与当前代码一致，`git diff --check` |
| 界面布局、绘制 | `cargo fmt`、编译，检查尺寸、缩放、中文长名称、裁剪和交互；简单外观无需重复实现的单元测试 |
| 配置、图结构、场景 | 迁移与往返保存测试，确认路由、增益、备注和隐藏状态保留 |
| 音频、队列、FFI、生命周期 | 工作区测试及针对边界条件的回归测试；涉及实际流行为时另做设备测试 |
| 依赖、vendored 代码 | 锁文件与上游差异、许可声明检查、相关测试与发布构建 |
| 打包、工作流 | 脚本与工作流检查、完整生成安装包、检查实际 CI 结果 |

可用 `cargo test --locked -p bridge`、`cargo test --locked -p soon-link-pro`、
`cargo test --locked -p usbip-audio` 缩小定位范围；提交前按改动影响运行所需工作区检查。
如已安装 actionlint，可运行 `actionlint .github/workflows/windows-build.yml`。

不要仅为验证源码而安装/注册驱动、修改系统音频设置、启动 DAW 或控制桌面；
这些设备或系统检查应在任务明确授权后进行。报告中区分本地测试、托管 CI 和真实设备验收。
安装、升级、卸载、听音、DAW 兼容、实际往返延迟和长时间稳定性都需要目标环境验证。

## 依赖、素材与仓库内容

新增贡献使用项目 MIT 许可；第三方代码、字体和工具保留各自作者、来源与完整许可。
修改 `vendor/usbip-audio` 时保留上游固定版本，并在 `UPSTREAM.md` 说明修改，避免无关格式变化掩盖差异。
品牌与 Logo 不表示对衍生产品的背书。

依赖更新后运行 `python scripts/update-notices.py`，审阅 `assets/open-source.json`、
`assets/THIRD_PARTY_NOTICES.md` 与许可文本，再运行 `--check`。检查覆盖 Windows 运行、构建与测试依赖，
但不能代替阅读新增组件的许可。工具会保留已有条目，删除条目前核对其他目标与分发路径。

遵循 `.gitignore`：`target`、`build`、`dist`、安装器、日志、用户配置、凭据、私钥及本地缓存不进入源码仓库。
提交前检查暂存区；保留正常图标和已有第三方许可文件，不要用过宽的忽略规则遮住需要维护的源码或素材。

## 安装包与发布

详细步骤见 [BUILD_AND_RELEASE.md](docs/BUILD_AND_RELEASE.md)。
`scripts/fetch-build-tools.ps1` 下载并校验固定版本工具，不安装音频驱动。
本地安装包使用 Inno Setup 6.4.3 和已校验的原厂 usbip-win2 0.9.8.1 x64 安装器：

```powershell
.\scripts\fetch-build-tools.ps1
.\scripts\build.ps1 -MakeInstaller `
  -InnoCompiler 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe' `
  -DriverInstaller '.\build\tools\usbip-win2-0.9.8.1-x64.exe'
```

打包前需要自行准备编译器。`build/payload` 暂存 EXE、配套 DLL、原厂驱动、文档与许可，安装包输出到 `dist`。
不要把源码、开发者配置或调试文件加入安装器；协议页是安装器首屏，使用说明与更新日志留在安装目录。
更新安装保留现有用户配置，已有配置优先于可选导入文件；卸载逻辑不递归删除用户数据。

`main` 推送、PR 和手动触发产生测试安装包 artifact；开发版统一移动固定 `dev` 标签并更新同一个
prerelease，版本保留在安装包、标题与更新说明中，不创建 `v<version>-dev` 标签。
正式版使用严格匹配根包版本的 `v<version>` 标签并保留该标签。更新 `dev` 时使用带预期旧值的
`--force-with-lease`，避免覆盖其他发布者的更新。发布前核对根包、driver、关于页和更新说明的显示版本。
工作流使用最小权限、固定 Action commit 和下载校验，不需要在仓库中保存个人访问令牌。

任务要求先审阅或先测试时，完成约定步骤后再提交、推送或发布；不要把创建源码仓库等同于批准正式版本。
提交和 PR 说明应写清具体行为、原因、已做检查与剩余限制；可以如实注明 AI 协助。

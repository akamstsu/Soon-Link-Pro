# 参与贡献

欢迎提交可复现的问题报告、文档改进和代码。先阅读 [AGENTS.md](AGENTS.md)。

## 本地开发

使用 Windows x64、Rust stable MSVC、Visual Studio C++ Build Tools 和 Windows SDK。
从 `main` 创建分支，修改后执行：

```powershell
cargo fmt --all -- --check
cargo test --locked --workspace
cargo build --locked --release --workspace
```

界面检查请覆盖跳线面板、矩阵、中文长名称、备注、缩放及小窗口。
音频与 IPC 改动请增加能发现回归的测试；简单外观改动无需编写重复实现的测试。
真实声卡、安装、DAW 与听音测试需在已获授权的设备上进行。

## 问题报告与 PR

说明 Windows 版本、程序版本、声卡及厂商驱动、DAW、采样率、缓冲大小、重现步骤
和预期结果。日志和配置可能包含用户名、进程路径与设备名称，请先删去私人信息。
PR 说明具体行为变化、已执行的检查和未验证部分；不要把块时长当作往返延迟。

修改配置结构时保持旧配置和场景兼容；不要重置用户的路由、音量、隐藏设置与备注。
修改音频回调、共享内存或队列前阅读 [音频实现约定](docs/AUDIO_DESIGN.md)。

## 许可与署名

原创贡献按本项目 MIT 许可证提交，第三方代码保留原作者、许可和来源。
引用第三方组件时同步更新组件清单和完整声明。品牌名、Logo 与商标不能用作他人背书。
可以在提交或 PR 中如实注明 AI 工具协助，同时由提交者负责核对行为和来源。

## 发布

参见 [自动构建与发布](docs/BUILD_AND_RELEASE.md)。测试安装包经维护者确认后再打标签；
提交、拉取请求和编译成功均不等于已经完成 Live 使用验收。

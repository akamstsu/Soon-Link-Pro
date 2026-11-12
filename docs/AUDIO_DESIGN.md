# 音频实现约定

## 公共接口

- Windows Application Loopback：
  https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/
- AUDIOCLIENT_ACTIVATION_PARAMS：
  https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ns-audioclientactivationparams-audioclient_activation_params
- IAudioClient::Initialize：
  https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-initialize
- IAudioCaptureClient::GetBuffer：
  https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudiocaptureclient-getbuffer
- IAudioCaptureClient::ReleaseBuffer：
  https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudiocaptureclient-releasebuffer
- WAVEFORMATEX：
  https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/ns-mmeapi-waveformatex
- Rust 原子内存顺序：
  https://doc.rust-lang.org/std/sync/atomic/enum.Ordering.html

接口声明使用 Rust Windows SDK 绑定。应用回录采用进程树包含模式、共享模式和事件回调；
先请求引擎采样率下的双声道 float32，失败后用新激活的客户端请求 signed PCM32。
PCM 字节率由采样率乘块对齐计算；激活 BLOB 大小由 SDK 类型的 `size_of` 得出。
异步完成回调持有参数内存；每个非空采集缓冲都由 RAII 归还，停止路径结束音频客户端。

## 应用参数

`bridge/src/config.rs` 定义共享协议布局和应用默认值。
默认 48 kHz、256 帧，虚拟方向为双声道。8192 帧音频队列及 4096 帧硬件队列是容量上限，
并不代表实际延迟；可用通道数还受设备、ASIO 和内存限制。
改变共享内存布局时必须同时升级 EXE、DLL 和映射协议标识。

## 音量

保存的推子位置在 0 至 1 之间。0 表示静音，其余位置对应 -40 至 0 dB。
幅度增益按 `exp(dB * ln(10) / 20)` 计算。输入、接线及输出增益在计划更新时合并，
音频回调只执行乘加；保持已有配置的音量刻度。

## 共享队列

每个队列仅允许一个生产者和一个消费者。样本写入后用 Release 发布写游标，
消费者用 Acquire 读取写游标；读游标对称发布。空间不足拒绝整块，避免半个块进入音频流。
游标采用环绕计数，容量为二的幂；不可信游标跨度不返回音频。
控制线程清空队列前须停止对应消费者，或保证由消费者执行。
回调路径不增加堆分配、等待锁、日志和文件访问。

# Windows 接口资料

- IAudioSessionManager2::GetSessionEnumerator：
  https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiosessionmanager2-getsessionenumerator
- IAudioMeterInformation::GetPeakValue：
  https://learn.microsoft.com/en-us/windows/win32/api/endpointvolume/nf-endpointvolume-iaudiometerinformation-getpeakvalue
- VerQueryValueW：
  https://learn.microsoft.com/en-us/windows/win32/api/winver/nf-winver-verqueryvaluew
- ShowWindowAsync：
  https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindowasync
- PostMessageW：
  https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postmessagew

应用名称来自版本资源，无可用名称时退回会话名或文件名。发声排序基于可枚举音频会话的峰值，
不能保证涵盖独占 ASIO 或受保护音频。扫描在应用选择面板的后台线程执行。

卡片名称仅为内部备注，不修改 Windows 属性、注册表设备名称或 DAW 通道名称。
Windows 音频设备描述符采用固定的总线编号；DLL 和主程序使用固定标识建立连接。

托盘通过异步窗口恢复和合并的窗口消息唤醒界面；关闭窗口隐藏到托盘。
自动启动实例跟随已登记宿主进程，手动保持独立后不跟随宿主退出。

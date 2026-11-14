#ifndef PayloadDir
  #error PayloadDir must point to the prepared runtime files
#endif
#ifndef InstallerOutput
  #error InstallerOutput is required
#endif
#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef AppDisplayVersion
  #error AppDisplayVersion is required
#endif
[Setup]
AppId={{29A9AA46-075D-4AC4-9839-AFD35A3F784C}
AppName=Soon Link Pro
AppVersion={#AppVersion}
AppVerName=Soon Link Pro R{#AppDisplayVersion}
AppPublisher=挚彬Club · 赤松_Akamatsu
AppPublisherURL=https://www.zhibin.club
AppSupportURL=https://www.zhibin.club
DefaultDirName={autopf}\Soon Link Pro
DefaultGroupName=Soon Link Pro
DisableProgramGroupPage=yes
DisableWelcomePage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.18362
PrivilegesRequired=admin
UsePreviousAppDir=yes
UsePreviousTasks=yes
AppMutex=Local\SoonLinkProMixerInstance_v1
CloseApplications=no
RestartApplications=no
UninstallDisplayIcon={app}\SoonLinkPro.exe
SetupIconFile={#PayloadDir}\assets\soon-link-pro-icon-v2.ico
OutputDir={#InstallerOutput}
OutputBaseFilename=Soon-Link-Pro-R{#AppDisplayVersion}-Setup-x64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupLogging=yes
VersionInfoCompany=挚彬Club
VersionInfoDescription=Soon Link Pro 安装程序
VersionInfoCopyright=© 2026 挚彬Club · 赤松_Akamatsu
LicenseFile={#PayloadDir}\licenses\LICENSE

[Languages]
Name: "chinesesimp"; MessagesFile: "{#SourcePath}ChineseSimplified.isl"

[Tasks]
Name: desktopicon; Description: "创建桌面快捷方式"; Flags: unchecked

[Files]
Source: "{#PayloadDir}\SoonLinkPro.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\SoonLinkProMcp.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\patchbay_asio.dll"; DestDir: "{app}"; Flags: ignoreversion regserver
Source: "{#PayloadDir}\drivers\*"; DestDir: "{app}\drivers"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#PayloadDir}\licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#PayloadDir}\使用说明.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\MCP使用说明.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\本次更新.md"; DestDir: "{app}"; Flags: ignoreversion
; No profile, scene, routing file, log, source tree or recursive deletion is shipped.

[Icons]
Name: "{group}\Soon Link Pro"; Filename: "{app}\SoonLinkPro.exe"
Name: "{autodesktop}\Soon Link Pro"; Filename: "{app}\SoonLinkPro.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\SoonLinkPro.exe"; Parameters: "--import-settings-if-missing ""{code:LegacyProfile}"""; Flags: runhidden waituntilterminated runasoriginaluser; Check: HasLegacyProfile
Filename: "{app}\SoonLinkPro.exe"; Description: "打开 Soon Link Pro"; Flags: postinstall nowait skipifsilent runasoriginaluser unchecked

[Code]
var
  ProfilePage: TInputFileWizardPage;
  DriverNeedsRestart: Boolean;
  LastProfileDirectory: String;
  SuggestedProfile: String;

function DriverInstalled: Boolean;
begin
  Result := RegKeyExists(HKLM64, 'SYSTEM\CurrentControlSet\Services\usbip2_ude') or
     RegKeyExists(HKLM64, 'SYSTEM\CurrentControlSet\Services\usbip_ude');
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Locator, Services, Processes: Variant;
begin
  Result := '';
  try
    Locator := CreateOleObject('WbemScripting.SWbemLocator');
    Services := Locator.ConnectServer('', 'root\CIMV2');
    Processes := Services.ExecQuery('SELECT ProcessId FROM Win32_Process WHERE Name="SoonLinkPro.exe" OR Name="SoonLinkProMcp.exe"');
    if Processes.Count > 0 then
      Result := '请先保存 DAW 工程、退出使用 Soon Link Pro ASIO 的宿主，从托盘菜单退出 Soon Link Pro，并在 Agent 客户端停止 Soon Link Pro MCP 服务，再继续更新。现有配置会保留。';
  except
    Result := '无法核对旧版是否仍在运行，请关闭旧程序后重试安装。';
  end;
end;

procedure InitializeWizard;
begin
  ProfilePage := CreateInputFilePage(wpSelectDir, '保留或导入配置',
    '更新安装自动保留现有配置',
    '已有用户配置始终优先，不会被此处选择的文件覆盖。第一次从便携版迁移时，可选取旧版 soon-link-pro-settings.json；留空即可。');
  ProfilePage.Add('旧版配置文件（可选）：', 'JSON 配置|*.json', '.json');
end;

procedure CurPageChanged(CurPageID: Integer);
var
  ProfileDirectory, Candidate: String;
begin
  // The destination is not initialized during InitializeWizard.
  // Resolve the suggestion after the destination page; preserve manual choices.
  if CurPageID = ProfilePage.ID then begin
    ProfileDirectory := WizardDirValue;
    if CompareText(ProfileDirectory, LastProfileDirectory) <> 0 then begin
      if (ProfilePage.Values[0] = '') or
         (ProfilePage.Values[0] = SuggestedProfile) then begin
        Candidate := AddBackslash(ProfileDirectory) + 'soon-link-pro-settings.json';
        SuggestedProfile := '';
        if FileExists(Candidate) then SuggestedProfile := Candidate;
        ProfilePage.Values[0] := SuggestedProfile;
      end;
      LastProfileDirectory := ProfileDirectory;
    end;
  end;
end;

function LegacyProfile(Param: String): String;
begin
  Result := ProfilePage.Values[0];
end;

function HasLegacyProfile: Boolean;
begin
  Result := (ProfilePage.Values[0] <> '') and FileExists(ProfilePage.Values[0]);
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if (CurPageID = ProfilePage.ID) and (ProfilePage.Values[0] <> '') and
     not FileExists(ProfilePage.Values[0]) then begin
    MsgBox('找不到所选配置文件，请重新选择或留空。', mbError, MB_OK);
    Result := False;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Driver: String;
  Code: Integer;
begin
  if (CurStep = ssPostInstall) and not DriverInstalled then begin
    Driver := ExpandConstant('{app}\drivers\usbip-win2-0.9.8.1-x64.exe');
    if CompareText(GetSHA256OfFile(Driver), '38cad6d4432b52d5bb9409d9ad03b72fdffc4ada4cd3a48fbeca1a2752a8518a') <> 0 then
      RaiseException('随包 USB/IP 驱动文件校验失败，未执行安装。');
    WizardForm.StatusLabel.Caption := '正在安装 USB/IP 虚拟声卡驱动…';
    if not Exec(Driver, '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /RESTARTEXITCODE=3010', '', SW_HIDE, ewWaitUntilTerminated, Code) then
      RaiseException('无法启动 USB/IP 原厂驱动安装器。');
    if (Code = 3010) or (Code = 1641) then DriverNeedsRestart := True
    else if Code <> 0 then RaiseException(Format('USB/IP 驱动安装未完成（%d）。可在软件设置中重新安装。', [Code]));
    if not DriverNeedsRestart and not DriverInstalled then
      RaiseException('原厂安装器已退出，但未检测到 USB/IP 客户端和驱动。请查看安装日志或在软件设置中重试。');
  end;
end;

function NeedRestart: Boolean;
begin
  Result := DriverNeedsRestart;
end;
// Shared USB/IP drivers and user configuration are retained on uninstall.

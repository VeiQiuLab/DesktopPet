; DesktopPet Inno Setup script
; Build: iscc installer/DesktopPet.iss

#define MyAppName "DesktopPet"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "DesktopPet"
#define MyAppExeName "DesktopPet.exe"

[Setup]
AppId={{8B7E7E50-4FBF-4AC1-9E3A-2C5D8E9C4E1F}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename=DesktopPet-Setup
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog

[Languages]
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
; 开始菜单快捷方式（默认勾选）
Name: "startmenuicon"; Description: "创建开始菜单快捷方式"; GroupDescription: "快捷方式:"; Flags: checkedonce
; 桌面快捷方式（默认不勾选）
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "快捷方式:"; Flags: unchecked

[Files]
; 主程序
Source: "..\dist\DesktopPet\DesktopPet.exe"; DestDir: "{app}"; Flags: ignoreversion
; 角色包
Source: "..\dist\DesktopPet\characters\*"; DestDir: "{app}\characters"; Flags: ignoreversion recursesubdirs createallsubdirs
; Cubism shader
Source: "..\dist\DesktopPet\FrameworkShaders\*"; DestDir: "{app}\FrameworkShaders"; Flags: ignoreversion
; README
Source: "..\dist\DesktopPet\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; 开始菜单（可选）
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppExeName}"; Tasks: startmenuicon
Name: "{group}\卸载 {#MyAppName}"; Filename: "{uninstallexe}"; Tasks: startmenuicon
; 桌面（可选，默认不勾选）
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
; 安装完成后的可选启动
Filename: "{app}\{#MyAppExeName}"; Description: "启动 {#MyAppName}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; 清理运行时生成的配置和日志
Type: files; Name: "{app}\desktop-pet.config.json"
Type: files; Name: "{app}\pet_runtime.log"

[Registry]
; 清理可能残留的开机启动项（如用户之前启用过）
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "DesktopPet"; Flags: dontcreatekey uninsdeletevalue

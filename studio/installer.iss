; F103 Studio — установщик Inno Setup 6. Собирает build_windows.ps1.
; UTF-8 с BOM. Версия: ISCC /DAppVersion=0.1.3 /Orelease\v0.1.3 /Fимя

#ifndef AppVersion
  #define AppVersion "0.1.3"
#endif

#define AppName "F103 Studio"
#define AppPublisher "primGPT"
#define AppExe "F103 Studio.exe"

[Setup]
AppId={{8F3C2A71-5E14-4B9D-9C60-2D7A1E84B6F3}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
OutputDir=release
OutputBaseFilename=F103-Studio-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
SetupIconFile=target\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
CloseApplications=force

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "target\win-bundle\F103 Studio\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#StringChange(AppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

; Inno Setup script for the mp3rgui Windows installer.
;
; Why this exists: winget ships mp3rgui as a portable zip, which leaves no
; Start Menu entry and no uninstaller. Users who do not use a terminal had no
; way to install or find the GUI at all.
;
; One installer serves both architectures: the x86_64 and arm64 builds are
; both packed in, and Check: decides which one lands on disk.
;
; Not built in place — scripts/build-windows-installer.sh stages this file, the
; icon, the licence and both binaries under target/ and runs ISCC there. The
; Source: paths below are relative to that staging directory.

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#ifndef OutputBase
  #define OutputBase "mp3rgui-setup"
#endif

#define MyAppName "mp3rgui"
#define MyAppPublisher "M-Igashi"
#define MyAppURL "https://github.com/M-Igashi/mp3rgain"
#define MyAppExeName "mp3rgui.exe"

[Setup]
; Never change AppId — it is what lets a new version upgrade an old one in
; place instead of installing alongside it.
AppId={{1E842DEF-78BE-4800-BE39-55F35E7DF4CF}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
VersionInfoVersion={#MyAppVersion}

; Install per-user so there is no UAC prompt. An admin-elevated install would
; add one more thing to go wrong for exactly the audience this installer is
; for, and mp3rgui needs nothing outside its own directory.
PrivilegesRequired=lowest
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
LicenseFile=LICENSE.txt

; x64 binaries do run on ARM64 through emulation, but shipping both lets an
; ARM64 machine get the native build.
ArchitecturesAllowed=x64compatible or arm64
ArchitecturesInstallIn64BitMode=x64compatible or arm64

OutputDir=.
OutputBaseFilename={#OutputBase}
SetupIconFile=icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName} {#MyAppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "x86_64\{#MyAppExeName}"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Check: not IsArm64; Flags: ignoreversion
Source: "arm64\{#MyAppExeName}";  DestDir: "{app}"; DestName: "{#MyAppExeName}"; Check: IsArm64;     Flags: ignoreversion
Source: "LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#MyAppName}}"; Flags: nowait postinstall skipifsilent

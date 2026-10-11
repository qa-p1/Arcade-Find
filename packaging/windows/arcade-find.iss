; Per-user Inno Setup 6. Shared tasks, PATH shim, receipts and silent parity.
; iscc /DVersion=0.3.0 /DSource=..\..\target\release packaging\windows\arcade-find.iss
#ifndef Version
  #define Version "0.3.0"
#endif
#ifndef Source
  #define Source "..\..\target\release"
#endif
#define ARCADE_ID "arcade.find"
#define ARCADE_NAME "Arcade Find"
#define ARCADE_VERSION Version
#define ARCADE_EXE "arcade-find.exe"
#define ARCADE_CLI "arcade-find"
; Preserve Find's existing upgrade/uninstall identity.
#define ARCADE_INSTALLER_ID "{{6C0E7A43-3B0B-4F55-9F7A-2E1D8C5B9A11}"
#define ARCADE_HOMEPAGE "https://github.com/qa-p1/Arcade-Find/tree/main/docs/user"
#define ARCADE_START_AT_LOGIN_DEFAULT 1
#include "arcade.iss"

[Setup]
DisableDirPage=yes
OutputDir=..\..\dist
OutputBaseFilename=Arcade-Find-{#Version}-x64-setup
SetupIconFile={#Source}\arcade-find.ico
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
LicenseFile=..\..\LICENSE-MIT
CloseApplications=yes

[Files]
Source: "{#Source}\arcade-find.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\arcade-find.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion

Source: "..\..\docs\user\*"; DestDir: "{app}\docs\user"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{userprograms}\Arcade Find\Arcade Find Settings"; Filename: "{app}\arcade-find.exe"; Parameters: "--settings"; IconFilename: "{app}\arcade-find.ico"; Flags: uninsneveruninstall

[Run]
Filename: "{app}\arcade-find.exe"; Parameters: "--background"; Flags: nowait postinstall skipifsilent; Description: "Start Arcade Find"

[UninstallRun]
Filename: "{app}\arcade-find.exe"; Parameters: "--quit"; Flags: runhidden; RunOnceId: "QuitArcadeFind"

[Registry]
; Remove the legacy Run value on uninstall. New installs use the shared
; optional Startup shortcut task, which Find respects through its receipt.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "ArcadeFind"; ValueType: none; Flags: dontcreatekey uninsdeletevalue

[UninstallDelete]
; The Arcade Link manifest (SPEC §3: uninstallers remove it). User data stays.
Type: files; Name: "{localappdata}\Arcade\apps\arcade.find.json"; Check: ArcadeOwnsSharedIntegration
Type: files; Name: "{userprograms}\Arcade Find\Arcade Find Settings.lnk"; Check: ArcadeOwnsSharedIntegration

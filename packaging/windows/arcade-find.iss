; Per-user Inno Setup 6. Shared tasks, PATH shim, receipts and silent parity.
; iscc /DVersion=0.4.3 /DSource=..\..\target\release packaging\windows\arcade-find.iss
#ifndef Version
  #define Version "0.4.3"
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
; Find removes app-specific files only when the shared helper proves ownership.
#define ARCADE_CUSTOM_EVENTS
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

[Code]
// Mirrors arcade-integration.ps1: ARCADE_HOME isolates the shared root.
function FindArcadeRoot(Param: String): String;
begin
  Result := GetEnv('ARCADE_HOME');
  if Result = '' then Result := ExpandConstant('{localappdata}\Arcade');
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then ArcadeIntegration(True);
end;

// The helper deletes the receipt only when this copy owns it (exit 0); exit 10
// leaves a successor's receipt in place, so its files are kept too.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Receipt: String;
  HadReceipt: Boolean;
begin
  if CurUninstallStep = usUninstall then begin
    Receipt := FindArcadeRoot('') + '\installs\arcade.find.json';
    HadReceipt := FileExists(Receipt);
    ArcadeIntegration(False);
    if HadReceipt and not FileExists(Receipt) then begin
      // [UninstallDelete] Check functions run during setup, before ownership
      // is known, so these are removed here. The Arcade Link manifest goes
      // too (SPEC §3: uninstallers remove it); user data stays.
      DeleteFile(FindArcadeRoot('') + '\apps\arcade.find.json');
      DeleteFile(ExpandConstant('{userprograms}\Arcade Find\Arcade Find Settings.lnk'));
      RemoveDir(ExpandConstant('{userprograms}\Arcade Find'));
    end;
  end;
end;

function GetCustomSetupExitCode: Integer;
begin
  if ArcadeIntegrationFailed then Result := 1 else Result := 0;
end;

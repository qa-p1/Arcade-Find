; Per-user installer (no administrator rights), built with Inno Setup 6:
;   iscc /DVersion=0.1.0 /DSource=target\release packaging\windows\arcade-find.iss
; Installs to %LOCALAPPDATA%\Programs\Arcade Find, the path Arcade Tools uses.

#ifndef Version
  #define Version "0.1.0"
#endif
#ifndef Source
  #define Source "..\..\target\release"
#endif

[Setup]
AppId={{6C0E7A43-3B0B-4F55-9F7A-2E1D8C5B9A11}
AppName=Arcade Find
AppVersion={#Version}
AppPublisher=qa-p1
AppPublisherURL=https://github.com/qa-p1/Arcade-Find
DefaultDirName={localappdata}\Programs\Arcade Find
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\..\dist
OutputBaseFilename=Arcade-Find-{#Version}-x64-setup
SetupIconFile={#Source}\arcade-find.ico
UninstallDisplayIcon={app}\arcade-find.exe
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

[Icons]
Name: "{userprograms}\Arcade Find"; Filename: "{app}\arcade-find.exe"; IconFilename: "{app}\arcade-find.ico"
Name: "{userprograms}\Arcade Find Settings"; Filename: "{app}\arcade-find.exe"; Parameters: "--settings"; IconFilename: "{app}\arcade-find.ico"

[Run]
Filename: "{app}\arcade-find.exe"; Parameters: "--background"; Flags: nowait postinstall skipifsilent; Description: "Start Arcade Find"

[UninstallRun]
Filename: "{app}\arcade-find.exe"; Parameters: "--quit"; Flags: runhidden; RunOnceId: "QuitArcadeFind"

[Registry]
; Start at login is the app's own setting; remove its value on uninstall.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "ArcadeFind"; ValueType: none; Flags: dontcreatekey uninsdeletevalue

[UninstallDelete]
; The Arcade Link manifest (SPEC §3: uninstallers remove it). User data stays.
Type: files; Name: "{localappdata}\Arcade\apps\arcade.find.json"

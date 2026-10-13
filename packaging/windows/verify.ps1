# Real silent Inno path on the ephemeral CI account, never the owner's PC.
$ErrorActionPreference = 'Stop'
$root = Join-Path $env:RUNNER_TEMP ('find-install-' + [guid]::NewGuid())
$env:ARCADE_HOME = Join-Path $root 'arcade'
$env:ARCADE_FIND_HOME = Join-Path $root 'find'
New-Item -ItemType Directory -Force $root | Out-Null
$setup = (Get-ChildItem dist\*setup.exe | Select-Object -First 1).FullName
try {
    $process = Start-Process $setup -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /MERGETASKS=arcade_startup' -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Inno install exited $($process.ExitCode)" }
    $receiptFile = Join-Path $env:ARCADE_HOME 'installs\arcade.find.json'
    $receipt = Get-Content $receiptFile -Raw | ConvertFrom-Json
    if ($receipt.method -ne 'windows-installer' -or $receipt.managedBy -ne 'self') { throw 'Wrong receipt metadata' }
    if (!(Test-Path $receipt.path) -or !(Test-Path $receipt.integration.cli) -or !(Test-Path $receipt.integration.desktopEntry) -or !(Test-Path $receipt.integration.autostart)) { throw 'Missing integration' }
    if ($receipt.path -ne (Join-Path $env:LOCALAPPDATA 'Programs\Arcade Find\arcade-find.exe')) { throw 'Wrong per-user install location' }
    if (Get-Process arcade-find -ErrorAction SilentlyContinue) { throw 'Silent install launched Find' }
    New-Item -ItemType Directory -Force (Join-Path $env:ARCADE_FIND_HOME 'data') | Out-Null
    Set-Content (Join-Path $env:ARCADE_FIND_HOME 'data\sentinel') 'keep data'
    # The app writes its Link manifest on first run; stand in for that run.
    $manifest = Join-Path $env:ARCADE_HOME 'apps\arcade.find.json'
    New-Item -ItemType Directory -Force (Split-Path $manifest) | Out-Null
    Set-Content $manifest '{}'
    $settingsLink = Join-Path ([Environment]::GetFolderPath('Programs')) 'Arcade Find\Arcade Find Settings.lnk'
    if (!(Test-Path $settingsLink)) { throw 'Install did not create the Settings shortcut' }
    $process = Start-Process $receipt.integration.uninstaller -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Inno uninstall exited $($process.ExitCode)" }
    if (Test-Path $receiptFile) { throw 'Uninstall kept receipt' }
    if (Test-Path $receipt.integration.cli) { throw 'Uninstall kept CLI shim' }
    if (Test-Path $receipt.integration.autostart) { throw 'Uninstall kept the login shortcut' }
    if (Test-Path $manifest) { throw 'Uninstall kept the Arcade Link manifest' }
    if (Test-Path (Split-Path $settingsLink)) { throw 'Uninstall kept the Start menu folder' }
    if (!(Test-Path (Join-Path $env:ARCADE_FIND_HOME 'data\sentinel'))) { throw 'Uninstall removed user data' }
    Write-Output 'Windows silent installer: receipt, per-user files, startup, CLI, no launch, uninstall (manifest, shortcuts) and data preservation passed'
} finally {
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
}

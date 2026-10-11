# Construit la version distribuée pour Windows :
#   dist\BoomGround-Setup-Windows.exe    (installateur, à double-cliquer)
#   dist\BoomGround-Windows.exe          (version portable, sans installation)
#   dist\BoomGround-Server-Windows.exe   (serveur dédié, optionnel)
#
# Usage : powershell -ExecutionPolicy Bypass -File scripts\package.ps1
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

cargo build --release --bins
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$env:BOOMGROUND_EXE = (Resolve-Path 'target\release\boomground.exe').Path
cargo build --release -p boomground-setup
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item -Force 'target\release\BoomGround-Setup.exe' 'dist\BoomGround-Setup-Windows.exe'
Copy-Item -Force 'target\release\boomground.exe' 'dist\BoomGround-Windows.exe'
Copy-Item -Force 'target\release\boomground-server.exe' 'dist\BoomGround-Server-Windows.exe'
Get-ChildItem dist | Format-Table Name, @{n='Mo'; e={[math]::Round($_.Length / 1MB, 1)}}

# Construit LE fichier à distribuer sous Windows :
#   dist\BoomGround-Setup.exe        (le jeu)
#   dist\BoomGround-server.exe       (serveur dédié)
# Un seul exécutable : au premier lancement il propose de s'installer
# (raccourcis, pare-feu, désinstallation), et il contient aussi le serveur
# des parties en ligne.
#
# Usage : powershell -ExecutionPolicy Bypass -File scripts\package.ps1
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

New-Item -ItemType Directory -Force dist | Out-Null
# Anciens fichiers (plusieurs exécutables avant la v1.1).
Get-ChildItem dist -File |
    Where-Object { $_.Name -notin @('BoomGround-Setup.exe', 'BoomGround-server.exe') } |
    Remove-Item -Force -ErrorAction SilentlyContinue
Copy-Item -Force 'target\release\boomground.exe' 'dist\BoomGround-Setup.exe'
# Serveur dédié (console, sans fenêtre) : server.conf est créé au premier lancement.
Copy-Item -Force 'target\release\boomground-server.exe' 'dist\BoomGround-server.exe'
Get-ChildItem dist | Format-Table Name, @{n='Mo'; e={[math]::Round($_.Length / 1MB, 1)}}

$ErrorActionPreference = 'Stop'

$projectRoot = Split-Path -Parent $PSScriptRoot
$releaseDir = Join-Path $projectRoot 'target\release'
$nsisDir = Join-Path $releaseDir 'bundle\nsis'

Push-Location $projectRoot
try {
    pnpm tauri build
}
finally {
    Pop-Location
}

$portableSource = Join-Path $releaseDir 'km003c-app.exe'
$portableTarget = Join-Path $releaseDir 'Star monitor.exe'
$installerTarget = Join-Path $nsisDir 'Star monitor-setup.exe'
$installerSource = Get-ChildItem -LiteralPath $nsisDir -Filter 'Star monitor*-setup.exe' -File |
    Where-Object { $_.Name -ne (Split-Path -Leaf $installerTarget) } |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1

if (-not (Test-Path -LiteralPath $portableSource)) {
    throw "Portable executable not found: $portableSource"
}
if ($null -eq $installerSource) {
    throw "NSIS installer not found in: $nsisDir"
}

Copy-Item -LiteralPath $portableSource -Destination $portableTarget -Force
Copy-Item -LiteralPath $installerSource.FullName -Destination $installerTarget -Force

Write-Host "Portable: $portableTarget"
Write-Host "Installer: $installerTarget"

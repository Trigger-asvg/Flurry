# Сборка релизного exe в dist\ (перед сборкой останавливает запущенную копию, после — запускает снова в трее).
param([switch]$NoRun)
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
Get-Process Flurry -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300
cargo build --release
if (-not $?) { exit 1 }
New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item target\release\Flurry.exe dist\Flurry.exe -Force
"{0:N1} MB -> dist\Flurry.exe" -f ((Get-Item dist\Flurry.exe).Length / 1MB)
if (-not $NoRun) { Start-Process dist\Flurry.exe -ArgumentList '--tray' }

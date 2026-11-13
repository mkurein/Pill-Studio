# Сборка F103 Studio для Windows: release\vX.Y.Z\F103-Studio-vX.Y.Z-windows-x64.zip
#   powershell -ExecutionPolicy Bypass -File build_windows.ps1
# Нужно: Rust (rustup, toolchain stable-msvc) и Visual Studio Build Tools (C++).
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$cargo = Get-Content Cargo.toml -Raw
if ($cargo -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw "нет версии в Cargo.toml" }
$v = $Matches[1]
Write-Host "F103 Studio v$v — сборка Windows" -ForegroundColor Cyan

cargo test -q
if ($LASTEXITCODE -ne 0) { throw "тесты не прошли" }
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "сборка не удалась" }

$out = "release\v$v"
New-Item -ItemType Directory -Force -Path $out | Out-Null
$stage = "target\win-bundle\F103 Studio"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item target\release\f103-studio.exe "$stage\F103 Studio.exe"
Copy-Item README.md "$stage\README.md"

$zip = "$out\F103-Studio-v$v-windows-x64.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $stage -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $(Split-Path $zip -Leaf)" | Add-Content "$out\checksum.txt"
Write-Host "готово: $zip" -ForegroundColor Green

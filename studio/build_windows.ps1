# Сборка F103 Studio для Windows:
#   release\vX.Y.Z\F103-Studio-vX.Y.Z-windows-x64.zip
#   release\vX.Y.Z\F103-Studio-vX.Y.Z-windows-x64-setup.exe
#   powershell -ExecutionPolicy Bypass -File build_windows.ps1
# Нужно: Rust (rustup, toolchain stable-msvc), Visual Studio (C++), Inno Setup 6.
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

# Иконка мастера установки — тот же PNG, что у окна программы.
New-Item -ItemType Directory -Force -Path "target" | Out-Null
python -c "from pathlib import Path; p=Path('assets/icon.png').read_bytes(); Path('target/icon.ico').write_bytes((0).to_bytes(2,'little')+(1).to_bytes(2,'little')+(1).to_bytes(2,'little')+bytes([0,0,0,0])+(1).to_bytes(2,'little')+(32).to_bytes(2,'little')+len(p).to_bytes(4,'little')+(22).to_bytes(4,'little')+p)"
if ($LASTEXITCODE -ne 0) { throw "не собралась icon.ico" }

$iscc = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
if (-not (Test-Path -LiteralPath $iscc)) { $iscc = "$env:ProgramFiles\Inno Setup 6\ISCC.exe" }
if (-not (Test-Path -LiteralPath $iscc)) { throw "нужен Inno Setup 6 (ISCC.exe)" }
$setupName = "F103-Studio-v$v-windows-x64-setup"
& $iscc "/DAppVersion=$v" "/O$out" "/F$setupName" "installer.iss"
if ($LASTEXITCODE -ne 0) { throw "Inno Setup не собрал установщик" }
$setup = Join-Path $out "$setupName.exe"

$lines = @(
    "$((Get-FileHash $zip -Algorithm SHA256).Hash.ToLower())  $(Split-Path $zip -Leaf)",
    "$((Get-FileHash $setup -Algorithm SHA256).Hash.ToLower())  $(Split-Path $setup -Leaf)"
)
Set-Content -Path "$out\checksum.txt" -Value $lines -Encoding ascii
Write-Host "готово: $zip" -ForegroundColor Green
Write-Host "готово: $setup" -ForegroundColor Green

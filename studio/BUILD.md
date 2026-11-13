# Сборка F103 Studio

F103 Studio — отладочная программа для платы STM32F103C8 (Blue Pill).
Исходники лежат в папке `studio/` этого репозитория, крейт называется `f103-studio`.
Прошивка собирается отдельно, из корня репозитория: `make -C Debug all -j8`.

Все команды ниже запускаются из `studio`. Версия берётся из поля `version` в `studio/Cargo.toml` (сейчас `0.1.10`). Нужен Rust 1.88 или новее.

## Windows

### Один раз

1. Rust с тулчейном MSVC. После установки откройте новое окно PowerShell:

```powershell
winget install Rustlang.Rustup
rustup default stable-msvc
rustc --version
cargo --version
```

2. Visual Studio 2022 (Community или Build Tools) с нагрузкой «Разработка классических приложений на C++». Без неё `cargo` не находит компоновщик `link.exe`.

3. Python 3 — скрипт упаковки делает из `assets/icon.png` иконку установщика:

```powershell
python --version
```

4. [Inno Setup 6](https://jrsoftware.org/isdl.php). После установки на диске должен быть файл:

```text
C:\Program Files (x86)\Inno Setup 6\ISCC.exe
```

### Запуск из исходников

```powershell
cd studio
cargo run --release
```

Готовый exe без установщика:

```text
studio\target\release\f103-studio.exe
```

### Установщик и zip

```powershell
cd studio
powershell -ExecutionPolicy Bypass -File .\build_windows.ps1
```

Скрипт прогоняет `cargo test`, собирает release и вызывает Inno Setup. На выходе:

```text
studio\release\v0.1.10\F103-Studio-v0.1.10-windows-x64-setup.exe
studio\release\v0.1.10\F103-Studio-v0.1.10-windows-x64.zip
studio\release\v0.1.10\checksum.txt
```

`setup.exe` ставит программу в профиль пользователя: `%LOCALAPPDATA%\Programs\F103 Studio`. Права администратора не нужны. Галочка «ярлык на рабочем столе» в мастере уже включена.

## macOS

### Один раз

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
brew install librsvg
rustc --version
```

`librsvg` даёт команду `rsvg-convert`: из неё скрипт собирает иконку `.icns`. `python3` в системе уже есть.

### Запуск из исходников

```bash
cd studio
cargo run --release
```

### .app и .dmg

Эти команды работают только на Mac:

```bash
cd studio
python3 build_macos.py
```

Один диск и для Apple Silicon, и для Intel:

```bash
python3 build_macos.py --universal
```

Открыть папку с результатом после сборки:

```bash
python3 build_macos.py --open
```

На выходе (вместо `arm64` будет `x64` или `universal`):

```text
studio/release/v0.1.10/F103-Studio-v0.1.10-macos-arm64.dmg
studio/release/v0.1.10/F103-Studio-v0.1.10-macos-arm64.app.zip
studio/release/v0.1.10/checksum.txt
```

Первый запуск скачанного приложения: правый клик по `F103 Studio.app` → «Открыть». Если приложение уже скопировано в «Программы»:

```bash
xattr -dr com.apple.quarantine "/Applications/F103 Studio.app"
```

## Проверки

Из папки `studio`. На Windows те же команды в PowerShell, на Mac — в Terminal.

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

## Плата

Сборка студии проходит без подключённой платы. OpenOCD и `arm-none-eabi-addr2line` нужны, когда студия читает живую плату по SWD.

```bash
brew install open-ocd
```

На Windows положите `openocd` в `PATH` или укажите путь в настройках студии.

# Сборка Pill Studio

Pill Studio — отладочная программа для плат Blue Pill (STM32F103C8) и Black Pill (STM32F411CE).
Исходники лежат в папке `studio/` этого репозитория, крейт называется `pill-studio`.
Прошивка собирается отдельно: Blue Pill — из корня этого репозитория (`make` или `make -C Debug all -j8`), Black Pill — в репозитории F411CE.

Все команды ниже запускаются из `studio`. Версия берётся из поля `version` в `studio/Cargo.toml` (сейчас `0.3.1`). Нужен Rust 1.88 или новее.

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
studio\target\release\pill-studio.exe
```

### Установщик и zip

```powershell
cd studio
powershell -ExecutionPolicy Bypass -File .\build_windows.ps1
```

Скрипт прогоняет `cargo test`, собирает release и вызывает Inno Setup. На выходе:

```text
studio\release\v0.3.1\Pill-Studio-v0.3.1-windows-x64-setup.exe
studio\release\v0.3.1\Pill-Studio-v0.3.1-windows-x64.zip
studio\release\v0.3.1\checksum.txt
```

`setup.exe` ставит программу в профиль пользователя: `%LOCALAPPDATA%\Programs\Pill Studio`. Права администратора не нужны. Галочка «ярлык на рабочем столе» в мастере уже включена.

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
studio/release/v0.3.1/Pill-Studio-v0.3.1-macos-arm64.dmg
studio/release/v0.3.1/Pill-Studio-v0.3.1-macos-arm64.app.zip
studio/release/v0.3.1/checksum.txt
```

Первый запуск скачанного приложения: правый клик по `Pill Studio.app` → «Открыть». Если приложение уже скопировано в «Программы»:

```bash
xattr -dr com.apple.quarantine "/Applications/Pill Studio.app"
```

## Проверки

Из папки `studio`. На Windows те же команды в PowerShell, на Mac — в Terminal.

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

Проверка приёма UART1 на подключённой плате — [UART_TESTING.md](../UART_TESTING.md).

## Плата

Сборка студии проходит без подключённой платы. OpenOCD и `arm-none-eabi-addr2line` нужны, когда студия читает живую плату по SWD.

```bash
brew install open-ocd
```

На Windows положите `openocd` в `PATH` или укажите путь в настройках студии.

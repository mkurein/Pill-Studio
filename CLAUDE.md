# CLAUDE.md

@AGENTS.md

## Claude Code specifics

- Рабочая папка — этот репозиторий (`primGPT/`), не родительская `STM32F1/` (ADR-009).
- Общение на русском.
- Сборка из PowerShell (PATH к toolchain задаётся вручную):

```powershell
$env:PATH = "C:\ST\STM32CubeIDE_1.17.0\STM32CubeIDE\plugins\com.st.stm32cube.ide.mcu.externaltools.make.win32_2.2.0.202409170845\tools\bin;C:\ST\STM32CubeIDE_1.17.0\STM32CubeIDE\plugins\com.st.stm32cube.ide.mcu.externaltools.gnu-tools-for-stm32.13.3.rel1.win32_1.0.0.202411081344\tools\bin;$env:PATH"
make -C Debug all -j8
arm-none-eabi-size Debug/primGPT.elf
```

- Файлы проекта в CRLF — сохранять окончания строк при правках.
- Факты о проекте держать в `docs/`, не в памяти чата: изменилось — обнови doc в том же commit.
- macOS: `make` (arm-none-eabi-gcc из Homebrew) → `build/`. Проверка на плате —
  сам гоняй `.venv/bin/python tools/hil.py test`, читай `log` / `diag` / `regs`
  и исправляй прошивку, пока тест не PASS (не останавливаться на «собралось»).
- GUI-студия: `studio/` (Rust, egui), `cargo run --release` в `studio/`.
  Pill Studio — для Blue Pill и Black Pill; данные плат в `studio/src/core/boards.rs`
  и `mcu_f411.rs`, после правки — `cargo test` и `cargo clippy --all-targets -- -D warnings`.
- Black Pill: прошивка в `../F411CE` (тот же цикл: `make` → `hil.py test`).
  Перед `hil.py test` проверь `hil.py probe`: какая плата на ST-Link.

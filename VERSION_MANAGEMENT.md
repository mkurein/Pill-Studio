# Управление версиями Pill Studio

Единственный источник — **`config/version.json`**. Руками версии в файлах не правятся: только через `tools/update_version.py`, затем запись в `CHANGELOG.md`.
Разработчик: Kurein M. N. (`app_info.developer`).

| Часть | Формат | Где хранится | Куда расставляется |
|---|---|---|---|
| Pill Studio (репозиторий) | SemVer `0.4.0` | `app_info.version` | `studio/Cargo.toml` (`[package] version`), запись `pill-studio` в `studio/Cargo.lock`, значок «версия» в `README.md` |
| Интерфейс и пакеты | — | из `Cargo.toml` | подпись `vX.Y.Z` в боковой панели (`CARGO_PKG_VERSION`), `setup.exe` / `.zip` (`build_windows.ps1`), `.dmg` / `.app` (`build_macos.py`); при расхождении с `config/version.json` скрипты не собирают |
| Прошивка Blue Pill | `git describe` | метка Git `vX.Y.Z` | `FW_VERSION` в `Makefile` → `fw_version[]`, ответ PING, строка `BOOT fw=` в логе; сборка CubeIDE (`Debug/`) даёт `cube` |
| Прошивка Black Pill | своя | репозиторий `../F411CE` | этот скрипт её не трогает |

## Команды (из корня `C:\Project\Pill-Studio`)

> Это справочник: **не запускайте блок целиком** — каждая строка меняет версию. Выполняйте только нужную команду; сначала можно с `--dry-run`.

```bash
python tools/update_version.py status                     # всё ли совпадает; код 1 при расхождении
python tools/update_version.py sync                       # расставить версию из config/version.json
python tools/update_version.py bump patch|minor|major     # 0.4.0 -> 0.4.1 / 0.5.0 / 1.0.0, дата выпуска = сегодня
python tools/update_version.py set 0.5.0                  # задать номер явно
python tools/update_version.py --dry-run bump minor       # показать, что изменится, без записи
```

`Cargo.lock` скрипт правит сам — `cargo check` после смены версии не нужен.

## Когда что поднимать

- **patch** — исправление, текст, справка в студии (`0.3.1 → 0.3.2`).
- **minor** — новый экран или функция студии, новая команда протокола (меняются обе прошивки и `studio/src/core/proto.rs` / `sim.rs`).
- **major** — несовместимый протокол или формат настроек студии.
- Правки только прошивки или документов версию не поднимают: прошивку отличает `git describe` (`v0.4.0-3-gabc1234`).

## Порядок выпуска

Поднять версию → `status` = OK → раздел `## [X.Y.Z] - YYYY-MM-DD` в `CHANGELOG.md` (перенести из `[Unreleased]`) → коммит → метка `vX.Y.Z` → `git push origin main --tags` → пакеты по [studio/BUILD.md](studio/BUILD.md).

## Проблемы

- **`DRIFT`** — файл правили вручную: `python tools/update_version.py sync`.
- **`MISSING` или «не найдена строка версии»** — строку переформатировали. Верните вид из таблицы выше (значок: `![Версия X.Y.Z](https://img.shields.io/badge/версия-X.Y.Z-…)`) и повторите `sync`.
- **Сборка пакета остановилась на сверке версии** — то же: `sync`, затем `status`.

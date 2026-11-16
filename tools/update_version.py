#!/usr/bin/env python3
"""Версия Pill Studio: источник — config/version.json, остальное расставляется отсюда.

Версия одна на репозиторий: её показывает студия (CARGO_PKG_VERSION), её несут
установщик Windows и пакет macOS, а метка Git vX.Y.Z попадает в строку прошивки
(`git describe` в Makefile). Только стандартная библиотека Python.

    python tools/update_version.py status                  # код 1 при расхождении
    python tools/update_version.py sync                    # из config/version.json во все файлы
    python tools/update_version.py bump patch|minor|major  # дата выпуска = сегодня
    python tools/update_version.py set X.Y.Z
Флаг --dry-run показывает изменения без записи.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "config" / "version.json"
CARGO_TOML = ROOT / "studio" / "Cargo.toml"
CARGO_LOCK = ROOT / "studio" / "Cargo.lock"
README = ROOT / "README.md"
CHANGELOG = ROOT / "CHANGELOG.md"

SEMVER = r"\d+\.\d+\.\d+"

# (метка, файл, выражение с одной группой — версией, флаги). Все совпадения равны источнику.
TRACKERS: list[tuple[str, Path, str, int]] = [
    ("cargo", CARGO_TOML, r'^\[package\][^\[]*?^version\s*=\s*"(' + SEMVER + r')"', re.M | re.S),
    ("cargo_lock", CARGO_LOCK, r'^name = "pill-studio"\r?\nversion = "(' + SEMVER + r')"', re.M),
    ("readme", README, r"badge/версия-(" + SEMVER + r")-", 0),
    ("readme", README, r"!\[Версия (" + SEMVER + r")\]", 0),
]


def rel(p: Path) -> str:
    return p.relative_to(ROOT).as_posix()


def read(p: Path) -> str:
    with p.open(encoding="utf-8", newline="") as f:  # newline="" — сохранить CRLF/LF файла
        return f.read()


def write(p: Path, text: str) -> None:
    with p.open("w", encoding="utf-8", newline="") as f:
        f.write(text)


def load() -> dict:
    return json.loads(read(SOURCE))


def save(data: dict, dry: bool) -> None:
    text = json.dumps(data, ensure_ascii=False, indent=2) + "\n"
    if text == read(SOURCE):
        return
    if dry:
        print(f"[DRY RUN] {rel(SOURCE)}")
        return
    write(SOURCE, text)
    print(f"Updated {rel(SOURCE)}")


def semver(v: str) -> tuple[int, int, int]:
    if not re.fullmatch(SEMVER, v):
        raise SystemExit(f"не semver: {v!r} (нужно X.Y.Z)")
    a, b, c = (int(x) for x in v.split("."))
    return a, b, c


def bumped(v: str, part: str) -> str:
    a, b, c = semver(v)
    return {"major": f"{a + 1}.0.0", "minor": f"{a}.{b + 1}.0", "patch": f"{a}.{b}.{c + 1}"}[part]


def has_changelog(v: str) -> bool:
    return CHANGELOG.exists() and re.search(rf"^## \[{re.escape(v)}\]", read(CHANGELOG), re.M) is not None


def status() -> int:
    info = load()["app_info"]
    v = info["version"]
    print(f"Source  {rel(SOURCE)}: {info['name']} v{v}  (release {info['release_date']}, "
          f"updated {info['last_updated']}, {info.get('developer', '')})\n")
    bad = 0
    for label, path, rx, flags in TRACKERS:
        found = re.findall(rx, read(path), flags) if path.exists() else []
        if not found:
            print(f"  {label:11} MISSING  {rel(path)}")
            bad += 1
            continue
        ok = all(x == v for x in found)
        bad += not ok
        print(f"  {label:11} {'OK' if ok else 'DRIFT':8} {rel(path):22} {', '.join(sorted(set(found)))}")
    if not has_changelog(v):
        print(f"\n  В {rel(CHANGELOG)} нет раздела ## [{v}] — допишите перед коммитом.")
    print("\nAll tracked files match." if not bad else f"\n{bad} mismatch(es): python tools/update_version.py sync")
    return 1 if bad else 0


def sync(v: str, dry: bool) -> None:
    for label, path, rx, flags in TRACKERS:
        text = read(path)
        new, n = re.subn(rx, lambda m: m.group(0)[: m.start(1) - m.start(0)] + v + m.group(0)[m.end(1) - m.start(0):], text, flags=flags)
        if n == 0:
            raise SystemExit(f"{rel(path)}: не найдена строка версии ({label}) — верните ожидаемый вид, см. VERSION_MANAGEMENT.md")
        if new == text:
            continue
        if dry:
            print(f"[DRY RUN] {rel(path)} -> {v} ({label})")
        else:
            write(path, new)
            print(f"Updated {rel(path)} ({label} {v})")


def main(argv: list[str] | None = None) -> int:
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8")  # кириллица в консоли Windows
    ap = argparse.ArgumentParser(description="Версия Pill Studio (источник: config/version.json)")
    ap.add_argument("--dry-run", action="store_true", help="показать изменения без записи")
    sp = ap.add_subparsers(dest="cmd", required=True)
    sp.add_parser("status", help="сверить файлы с источником")
    sp.add_parser("sync", help="расставить версию из источника")
    b = sp.add_parser("bump", help="поднять версию")
    b.add_argument("part", choices=["patch", "minor", "major"])
    s = sp.add_parser("set", help="задать версию X.Y.Z")
    s.add_argument("version")
    a = ap.parse_args(argv)

    if a.cmd == "status":
        return status()

    data = load()
    info = data["app_info"]
    if a.cmd != "sync":
        old = info["version"]
        info["version"] = bumped(old, a.part) if a.cmd == "bump" else a.version
        semver(info["version"])
        today = date.today()
        info["release_date"] = today.strftime("%d.%m.%Y")
        info["last_updated"] = today.isoformat()
        print(f"{info['name']} {old} -> {info['version']}")
    sync(info["version"], a.dry_run)
    save(data, a.dry_run)
    if a.cmd != "sync" and not has_changelog(info["version"]):
        print(f"\nДальше: раздел ## [{info['version']}] - {info['last_updated']} в {rel(CHANGELOG)}, "
              f"status, коммит, метка v{info['version']}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

//! Firmware ELF (`build/primGPT.elf` or CubeIDE `Debug/primGPT.elf`):
//! symbols for live variable reads and the section map for the memory view.
//! Source lines for fault addresses come from `arm-none-eabi-addr2line` if present.

use object::{Object, ObjectSection, ObjectSymbol, SymbolKind};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug)]
pub struct Sym {
    pub name: String,
    pub addr: u32,
    pub size: u32,
    pub func: bool,
}

#[derive(Clone, Debug)]
pub struct Section {
    pub name: String,
    pub addr: u32,
    pub size: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Elf {
    pub path: PathBuf,
    pub syms: Vec<Sym>,
    pub sections: Vec<Section>,
}

impl Elf {
    pub fn load(path: &Path) -> Result<Elf, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        let file = object::File::parse(&*data).map_err(|e| format!("ELF: {}", e))?;
        let mut syms: Vec<Sym> = file
            .symbols()
            .filter(|s| matches!(s.kind(), SymbolKind::Data | SymbolKind::Text) && s.size() > 0)
            .filter_map(|s| {
                Some(Sym {
                    name: s.name().ok()?.to_string(),
                    addr: s.address() as u32 & !1,
                    size: s.size() as u32,
                    func: s.kind() == SymbolKind::Text,
                })
            })
            .collect();
        syms.sort_by(|a, b| a.name.cmp(&b.name));
        syms.dedup_by(|a, b| a.name == b.name);
        let sections = file
            .sections()
            .filter(|s| s.size() > 0 && s.address() != 0)
            .filter_map(|s| Some(Section { name: s.name().ok()?.to_string(), addr: s.address() as u32, size: s.size() as u32 }))
            .collect();
        Ok(Elf {
            path: path.to_path_buf(),
            syms,
            sections,
        })
    }

    pub fn sym(&self, name: &str) -> Option<&Sym> {
        self.syms.iter().find(|s| s.name == name)
    }

    /// Function containing `addr` (fallback when addr2line is missing).
    pub fn func_at(&self, addr: u32) -> Option<&Sym> {
        let a = addr & !1;
        self.syms.iter().filter(|s| s.func).find(|s| a >= s.addr && a < s.addr + s.size)
    }

    pub fn used(&self, lo: u32, hi: u32) -> u32 {
        self.sections.iter().filter(|s| s.addr >= lo && s.addr < hi).map(|s| s.size).sum()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub func: String,
    pub file: String,
    pub line: u32,
}

/// Function and `file:line` from DWARF (`arm-none-eabi-addr2line`).
pub fn place(elf: &Elf, tool_dir: &str, addr: u32) -> Option<Place> {
    let mut c = Command::new(addr2line_bin(tool_dir));
    c.args(["-f", "-C", "-e"]).arg(&elf.path).arg(format!("0x{:08x}", addr & !1));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let o = c.output().ok()?;
    let s = String::from_utf8_lossy(&o.stdout);
    let mut lines = s.lines();
    let func = lines.next()?.trim();
    let loc = lines.next()?.trim();
    if func.is_empty() || func == "??" {
        return None;
    }
    let (file, line) = split_loc(loc)?;
    Some(Place { func: func.to_string(), file, line })
}

/// "file.c:123 (function)" via addr2line, or the symbol name.
pub fn where_is(elf: &Elf, tool_dir: &str, addr: u32) -> String {
    if let Some(p) = place(elf, tool_dir, addr) {
        let short = p.file.rsplit(['/', '\\']).next().unwrap_or(&p.file);
        return format!("{short}:{} ({})", p.line, p.func);
    }
    elf.func_at(addr).map(|s| format!("{} +0x{:x}", s.name, (addr & !1) - s.addr)).unwrap_or_else(|| "—".into())
}

/// Last `:digits` is the line. The drive letter colon in `C:/...` stays in the path.
pub fn split_loc(loc: &str) -> Option<(String, u32)> {
    let (path, line) = loc.rsplit_once(':')?;
    let line: u32 = line.parse().ok()?;
    if line == 0 || path.is_empty() || path == "??" {
        return None;
    }
    Some((path.to_string(), line))
}

/// DWARF often stores a path relative to the project, sometimes absolute.
pub fn source_file(project: &Path, loc: &str) -> Option<PathBuf> {
    let p = PathBuf::from(loc);
    if p.is_file() {
        return Some(p);
    }
    let joined = project.join(&p);
    if joined.is_file() {
        return Some(joined);
    }
    let name = p.file_name()?;
    let under = project.join("Core").join("Src").join(name);
    under.is_file().then_some(under)
}

/// Thumb listing from the ELF. ST's OpenOCD is built without Capstone, so
/// `arm disassemble` only answers "capstone disassembly framework required".
pub fn objdump_range(elf: &Path, tool_dir: &str, start: u32, stop: u32) -> Result<String, String> {
    let start = start & !1;
    let stop = stop.max(start.wrapping_add(2));
    let bin = objdump_bin(tool_dir);
    let mut c = Command::new(&bin);
    c.args([
        "-d",
        "-z",
        "--no-show-raw-insn",
        &format!("--start-address=0x{start:08x}"),
        &format!("--stop-address=0x{stop:08x}"),
    ])
    .arg(elf);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let o = c.output().map_err(|_| format!("Не найден {}. Каталог toolchain задаётся в Настройках.", bin.display()))?;
    if !o.status.success() {
        let err = String::from_utf8_lossy(&o.stderr);
        let err = err.trim();
        return Err(if err.is_empty() { "objdump не разобрал этот диапазон ELF".into() } else { format!("objdump: {err}") });
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

fn objdump_bin(tool_dir: &str) -> PathBuf {
    tool_bin(tool_dir, "objdump")
}

fn addr2line_bin(tool_dir: &str) -> PathBuf {
    tool_bin(tool_dir, "addr2line")
}

fn tool_bin(tool_dir: &str, which: &str) -> PathBuf {
    let name = if cfg!(windows) { format!("arm-none-eabi-{which}.exe") } else { format!("arm-none-eabi-{which}") };
    if !tool_dir.is_empty() {
        let p = Path::new(tool_dir).join(&name);
        if p.is_file() {
            return p;
        }
    }
    if let Some(extra) = crate::core::config::cube_toolchain_path() {
        for dir in std::env::split_paths(&extra) {
            let p = dir.join(&name);
            if p.is_file() {
                return p;
            }
        }
    }
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_path_keeps_the_drive_letter() {
        assert_eq!(
            split_loc("C:/Project/F103C8T/Core/Src/main.c:142"),
            Some(("C:/Project/F103C8T/Core/Src/main.c".into(), 142))
        );
        assert_eq!(split_loc("??:0"), None);
    }
}

/// Default ELF locations relative to the project root.
pub fn candidates(project: &Path) -> Vec<PathBuf> {
    vec![project.join("build/primGPT.elf"), project.join("Debug/primGPT.elf")]
}

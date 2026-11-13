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

/// "file.c:123 (function)" via arm-none-eabi-addr2line, or the symbol name.
pub fn where_is(elf: &Elf, tool_dir: &str, addr: u32) -> String {
    let exe = if tool_dir.is_empty() {
        "arm-none-eabi-addr2line".to_string()
    } else {
        Path::new(tool_dir).join("arm-none-eabi-addr2line").to_string_lossy().to_string()
    };
    let mut c = Command::new(exe);
    c.args(["-f", "-C", "-e"]).arg(&elf.path).arg(format!("0x{:08x}", addr));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    if let Ok(o) = c.output() {
        let s = String::from_utf8_lossy(&o.stdout);
        let mut l = s.lines();
        if let (Some(f), Some(loc)) = (l.next(), l.next()) {
            if f != "??" {
                let short = loc.rsplit(['/', '\\']).next().unwrap_or(loc);
                return format!("{} ({})", short, f);
            }
        }
    }
    elf.func_at(addr).map(|s| format!("{} +0x{:x}", s.name, (addr & !1) - s.addr)).unwrap_or_else(|| "—".into())
}

/// Default ELF locations relative to the project root.
pub fn candidates(project: &Path) -> Vec<PathBuf> {
    vec![project.join("build/primGPT.elf"), project.join("Debug/primGPT.elf")]
}

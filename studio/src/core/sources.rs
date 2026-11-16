//! Source paths on disk and function locations from the loaded firmware ELF.

use super::elf::{self, Elf, Place};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct Folder {
    pub folders: BTreeMap<String, Folder>,
    pub files: Vec<PathBuf>,
}

pub struct Function {
    pub name: String,
    pub addr: u32,
    pub place: Option<Place>,
}

#[derive(Default)]
pub struct Index {
    pub tree: Folder,
    pub files: Vec<PathBuf>,
    pub functions: Vec<Function>,
    pub symbol_error: Option<String>,
}

impl Index {
    pub fn load(root: &Path, elf: Option<&Elf>, tool_dir: &str) -> Self {
        let mut index = Self::default();
        if root.is_dir() {
            scan(root, &mut index.tree, &mut index.files);
        }
        let Some(elf) = elf else { return index };
        index.functions = elf
            .syms
            .iter()
            .filter(|s| s.func && Some(s.addr) != elf.default_handler)
            .map(|s| Function { name: s.name.clone(), addr: s.addr, place: None })
            .collect();
        // Bounded argument lists also fit the Windows command-line limit.
        for chunk in index.functions.chunks_mut(128) {
            let addrs: Vec<_> = chunk.iter().map(|f| f.addr).collect();
            match elf::places(&elf.path, tool_dir, &addrs) {
                Ok(places) => {
                    for (function, place) in chunk.iter_mut().zip(places) {
                        function.place = place;
                    }
                }
                Err(err) => {
                    index.symbol_error = Some(err);
                    break;
                }
            }
        }
        index
    }

    /// Prefer the exact path recorded by DWARF. If the project was moved,
    /// accept a unique path suffix from the project tree.
    pub fn resolve(&self, root: &Path, loc: &str) -> Option<PathBuf> {
        let recorded = Path::new(loc);
        for path in [root.join(recorded), PathBuf::from(recorded)] {
            if path.is_file() {
                return Some(path.canonicalize().unwrap_or(path));
            }
        }
        let normalized = loc.replace('\\', "/");
        let recorded = Path::new(&normalized);
        let mut candidates = self.files.iter().filter_map(|file| {
            let rel = file.strip_prefix(root).ok()?;
            let shared = recorded
                .components()
                .rev()
                .zip(rel.components().rev())
                .take_while(|(a, b)| a.as_os_str().eq_ignore_ascii_case(b.as_os_str()))
                .count();
            (shared > 0).then_some((shared, file))
        });
        let (mut score, mut selected) = candidates.next()?;
        let mut unique = true;
        for (shared, file) in candidates {
            if shared > score {
                (score, selected) = (shared, file);
                unique = true;
            } else if shared == score {
                unique = false;
            }
        }
        unique.then(|| selected.clone())
    }
}

fn scan(dir: &Path, tree: &mut Folder, all: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        if kind.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.')
                || matches!(
                    name.to_ascii_lowercase().as_str(),
                    "build" | "debug" | "release" | "target" | "studio" | "node_modules"
                )
            {
                continue;
            }
            let mut folder = Folder::default();
            scan(&path, &mut folder, all);
            if !folder.files.is_empty() || !folder.folders.is_empty() {
                tree.folders.insert(name, folder);
            }
        } else if kind.is_file()
            && (path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                matches!(
                    s.to_ascii_lowercase().as_str(),
                    "c" | "h" | "cc" | "cpp" | "hpp" | "cxx" | "hh" | "s" | "asm" | "ld" | "inc" | "rs"
                )
            }) || entry.file_name().eq_ignore_ascii_case("Makefile"))
        {
            all.push(path.clone());
            tree.files.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moved_project_uses_the_most_specific_unambiguous_source_path() {
        let root = Path::new("missing-project-root");
        let main = root.join("Core/Src/main.c");
        let index = Index { files: vec![main.clone(), root.join("Examples/main.c")], ..Index::default() };
        assert_eq!(index.resolve(root, "../old/Core/Src/main.c"), Some(main));
        assert_eq!(index.resolve(root, "../old/main.c"), None);
    }
}

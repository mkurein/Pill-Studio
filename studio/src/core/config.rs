//! Persistent settings (JSON in the OS config dir): tool paths, ports, project.

use super::openocd::OcdConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Firmware project root (contains Core/, Makefile, build/).
    pub project_dir: String,
    pub ocd: OcdConfig,
    /// Folder with arm-none-eabi-* (empty = PATH).
    pub toolchain_dir: String,
    pub build_cmd: String,
    pub proto_port: String,
    pub log_port: String,
    pub poll_ms: u64,
    pub dark: bool,
    /// On every stop of the core, open its source line in the editor (`cursor -g file:line`).
    pub editor_follow: bool,
    /// Editor CLI: "cursor", "code" or a full path. Empty = find Cursor.
    pub editor_cmd: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            project_dir: default_project_dir(),
            ocd: OcdConfig::default(),
            toolchain_dir: String::new(),
            build_cmd: "make -j8".into(),
            proto_port: String::new(),
            log_port: String::new(),
            poll_ms: 1000,
            dark: true,
            editor_follow: true,
            editor_cmd: String::new(),
        }
    }
}

impl Config {
    /// Editor CLI to call on a stop, or `None` when following is off or no editor is found.
    pub fn editor(&self) -> Option<String> {
        if !self.editor_follow {
            return None;
        }
        let c = self.editor_cmd.trim();
        if !c.is_empty() {
            return Some(c.to_string());
        }
        find_cursor()
    }
}

/// Cursor's command-line launcher. An app started from the Dock or Explorer does not
/// inherit the shell PATH, so the install locations are checked first.
pub fn find_cursor() -> Option<String> {
    let mut cands: Vec<PathBuf> = vec![];
    if cfg!(target_os = "macos") {
        cands.push("/Applications/Cursor.app/Contents/Resources/app/bin/cursor".into());
        if let Some(h) = dirs::home_dir() {
            cands.push(h.join("Applications/Cursor.app/Contents/Resources/app/bin/cursor"));
        }
        cands.push("/usr/local/bin/cursor".into());
        cands.push("/opt/homebrew/bin/cursor".into());
    } else if cfg!(windows) {
        if let Some(d) = dirs::data_local_dir() {
            cands.push(d.join(r"Programs\cursor\resources\app\bin\cursor.cmd"));
        }
    } else {
        cands.push("/usr/bin/cursor".into());
        cands.push("/usr/local/bin/cursor".into());
        cands.push("/opt/cursor/resources/app/bin/cursor".into());
    }
    cands.into_iter().find(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned())
}

/// The studio lives in `<project>/studio`: use the parent when it looks like the firmware.
fn default_project_dir() -> String {
    let mut cands: Vec<PathBuf> = vec![];
    if let Ok(cwd) = std::env::current_dir() {
        cands.push(cwd.clone());
        if let Some(p) = cwd.parent() {
            cands.push(p.to_path_buf());
        }
    }
    cands.push(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
    for c in cands {
        if c.join("Core/Src/main.c").exists() {
            return c.canonicalize().unwrap_or(c).to_string_lossy().to_string();
        }
    }
    String::new()
}

/// CubeIDE's `Debug/` makefile is not in the repo. The root Makefile writes `build/primGPT.elf`.
pub fn build_cmd_for(project: &std::path::Path, saved: &str) -> String {
    let legacy = saved == "make -C Debug all -j8";
    let debug_mk = project.join("Debug").join("makefile").is_file() || project.join("Debug").join("Makefile").is_file();
    if legacy && project.join("Makefile").is_file() && !debug_mk {
        "make -j8".into()
    } else {
        saved.to_string()
    }
}

/// `make` and `arm-none-eabi-gcc` shipped with STM32CubeIDE, for a GUI that has neither on PATH.
pub fn cube_toolchain_path() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let mut make_bin = None;
    let mut gcc_bin = None;
    let Ok(st) = std::fs::read_dir(r"C:\ST") else { return None };
    for ide in st.flatten() {
        let plugins = ide.path().join("STM32CubeIDE").join("plugins");
        let Ok(rd) = std::fs::read_dir(&plugins) else { continue };
        for ent in rd.flatten() {
            let name = ent.file_name();
            let name = name.to_string_lossy();
            let bin = ent.path().join("tools").join("bin");
            if name.starts_with("com.st.stm32cube.ide.mcu.externaltools.make.win32") && bin.join("make.exe").is_file() {
                make_bin = Some(bin);
            } else if name.starts_with("com.st.stm32cube.ide.mcu.externaltools.gnu-tools-for-stm32")
                && bin.join("arm-none-eabi-gcc.exe").is_file()
            {
                gcc_bin = Some(bin);
            }
        }
    }
    match (make_bin, gcc_bin) {
        (Some(m), Some(g)) => Some(format!("{};{}", m.display(), g.display())),
        (Some(m), None) => Some(m.display().to_string()),
        (None, Some(g)) => Some(g.display().to_string()),
        (None, None) => None,
    }
}

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("f103-studio").join("config.json"))
}

impl Config {
    pub fn load() -> Config {
        let mut c: Config = path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        c.ocd = c.ocd.for_this_machine();
        if c.project_dir.is_empty() || !std::path::Path::new(&c.project_dir).join("Core").exists() {
            c.project_dir = default_project_dir();
        }
        c.build_cmd = build_cmd_for(std::path::Path::new(&c.project_dir), &c.build_cmd);
        c
    }
    pub fn save(&self) {
        if let Some(p) = path() {
            let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
            let _ = std::fs::write(p, serde_json::to_string_pretty(self).unwrap_or_default());
        }
    }
    pub fn python(&self) -> PathBuf {
        let root = PathBuf::from(&self.project_dir);
        if cfg!(windows) {
            root.join(".venv/Scripts/python.exe")
        } else {
            root.join(".venv/bin/python")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_debug_dir_uses_root_makefile() {
        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
        let cmd = build_cmd_for(&root, "make -C Debug all -j8");
        if root.join("Makefile").is_file() && !root.join("Debug/makefile").is_file() {
            assert_eq!(cmd, "make -j8");
        }
        assert_eq!(build_cmd_for(&root, "make -j4"), "make -j4");
    }
}

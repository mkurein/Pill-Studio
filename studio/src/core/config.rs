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
}

impl Default for Config {
    fn default() -> Self {
        Config {
            project_dir: default_project_dir(),
            ocd: OcdConfig::default(),
            toolchain_dir: String::new(),
            build_cmd: if cfg!(windows) { "make -C Debug all -j8".into() } else { "make -j8".into() },
            proto_port: String::new(),
            log_port: String::new(),
            poll_ms: 1000,
            dark: true,
        }
    }
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

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("f103-studio").join("config.json"))
}

impl Config {
    pub fn load() -> Config {
        let mut c: Config = path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if c.project_dir.is_empty() || !std::path::Path::new(&c.project_dir).join("Core").exists() {
            c.project_dir = default_project_dir();
        }
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

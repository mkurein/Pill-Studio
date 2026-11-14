//! Persistent settings (JSON in the OS config dir): tool paths, ports, project.

use super::boards::{self, BoardId};
use super::openocd::OcdConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Settings that belong to one board (work and home can have different boards,
/// projects and USB-UART adapters). The selected board's values live in the
/// flat fields of `Config`; the others wait here.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Slot {
    pub project_dir: String,
    pub build_cmd: String,
    pub proto_port: String,
    pub log_port: String,
}

/// A firmware project used recently (any board): the switch list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub dir: String,
    pub board: BoardId,
    pub build_cmd: String,
    /// Unix seconds of the last time it was the active project.
    pub used: u64,
}

/// One build / flash / autotest run, kept across restarts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// Unix seconds when it finished.
    pub t: u64,
    pub dir: String,
    pub board: BoardId,
    /// "Сборка", "Прошивка", "Автотест hil.py".
    pub what: String,
    pub ok: bool,
    pub secs: f32,
    /// "main a1b2c3d +изм." at the start of the run, empty outside git.
    pub git: String,
}

pub const RECENT_MAX: usize = 12;
pub const HISTORY_MAX: usize = 60;

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Board selected in the studio (Blue Pill F103 / Black Pill F411).
    pub board: BoardId,
    /// Per-board settings of the boards not selected now, by `BoardId::key()`.
    pub slots: BTreeMap<String, Slot>,
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
    /// Projects used recently, newest first.
    pub recent: Vec<Recent>,
    /// Build / flash / autotest runs, newest last.
    pub history: Vec<Run>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            board: BoardId::BluePill,
            slots: BTreeMap::new(),
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
            recent: vec![],
            history: vec![],
        }
    }
}

impl Config {
    /// Select another board: the current project, ports and build command are
    /// stashed for the old board and the new board's ones come back.
    pub fn switch_board(&mut self, to: BoardId) {
        if to == self.board {
            return;
        }
        let old = Slot {
            project_dir: self.project_dir.clone(),
            build_cmd: self.build_cmd.clone(),
            proto_port: self.proto_port.clone(),
            log_port: self.log_port.clone(),
        };
        self.slots.insert(self.board.key().into(), old);
        let s = self.slots.remove(to.key()).unwrap_or_default();
        let near = self.project_dir.clone();
        self.board = to;
        boards::set(to);
        self.project_dir = if is_project_of(Path::new(&s.project_dir), to) { s.project_dir } else { guess_project(to, &near) };
        self.build_cmd = if s.build_cmd.is_empty() { "make -j8".into() } else { s.build_cmd };
        self.proto_port = s.proto_port;
        self.log_port = s.log_port;
    }

    /// Put the active project on top of the recent list.
    pub fn touch_project(&mut self) {
        if self.project_dir.is_empty() {
            return;
        }
        self.recent.retain(|r| r.dir != self.project_dir);
        self.recent.insert(
            0,
            Recent { dir: self.project_dir.clone(), board: self.board, build_cmd: self.build_cmd.clone(), used: now_secs() },
        );
        self.recent.truncate(RECENT_MAX);
    }

    /// Make `dir` the active project. Its board is found by the linker
    /// script; returns true when that switched the board (then the caller
    /// reloads the SVD and closes the links). Err when `dir` is no project.
    pub fn use_project(&mut self, dir: &str) -> Result<bool, String> {
        let p = Path::new(dir);
        let board = board_of(p).ok_or_else(|| {
            let ld: Vec<&str> = BoardId::ALL.iter().map(|b| b.profile().marker).collect();
            format!("{dir}: не проект прошивки (нужны Core/ и {})", ld.join(" или "))
        })?;
        let dir = p.canonicalize().map(|c| c.to_string_lossy().into_owned()).unwrap_or_else(|_| dir.to_string());
        let switched = board != self.board;
        self.touch_project();
        if switched {
            self.switch_board(board);
        }
        let cmd = self.recent.iter().find(|r| r.dir == dir).map(|r| r.build_cmd.clone());
        self.project_dir = dir;
        self.build_cmd = build_cmd_for(Path::new(&self.project_dir), &cmd.unwrap_or_else(|| self.build_cmd.clone()));
        self.touch_project();
        Ok(switched)
    }

    pub fn add_run(&mut self, r: Run) {
        self.history.push(r);
        let n = self.history.len();
        if n > HISTORY_MAX {
            self.history.drain(..n - HISTORY_MAX);
        }
    }

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

/// Full path of a tool given by bare name (`openocd`, `arm-none-eabi-objdump`).
/// An app started from Finder or the Dock gets PATH=/usr/bin:/bin:/usr/sbin:/sbin,
/// so the Homebrew prefixes are searched after PATH. Unknown → the name unchanged.
pub fn find_tool(name: &str) -> PathBuf {
    let given = Path::new(name);
    if name.is_empty() || given.components().count() > 1 {
        return given.to_path_buf();
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|v| std::env::split_paths(&v).collect()).unwrap_or_default();
    if !cfg!(windows) {
        dirs.push("/opt/homebrew/bin".into());
        dirs.push("/usr/local/bin".into());
    }
    let file = if cfg!(windows) && given.extension().is_none() { format!("{name}.exe") } else { name.to_string() };
    dirs.iter().map(|d| d.join(&file)).find(|p| p.is_file()).unwrap_or_else(|| given.to_path_buf())
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

/// The firmware repo of `board`: has Core/ and the board's linker script.
fn is_project_of(dir: &Path, board: BoardId) -> bool {
    dir.join("Core").is_dir() && dir.join(board.profile().marker).is_file()
}

/// Board of a firmware project folder, by its linker script.
pub fn board_of(dir: &Path) -> Option<BoardId> {
    BoardId::ALL.into_iter().find(|b| is_project_of(dir, *b))
}

/// Firmware projects of every board next to `near` and in ~/Project/ProjectSTM.
pub fn find_projects(near: &str) -> Vec<(String, BoardId)> {
    let mut roots: Vec<PathBuf> = vec![];
    roots.extend(Path::new(near).parent().map(Path::to_path_buf));
    let own = default_project_dir();
    roots.extend(Path::new(&own).parent().map(Path::to_path_buf));
    if let Some(h) = dirs::home_dir() {
        roots.push(h.join("Project").join("ProjectSTM"));
    }
    let mut out: Vec<(String, BoardId)> = vec![];
    for r in roots {
        let Ok(rd) = std::fs::read_dir(&r) else { continue };
        for e in rd.flatten() {
            let p = e.path().canonicalize().unwrap_or_else(|_| e.path());
            if let Some(b) = board_of(&p) {
                let d = p.to_string_lossy().into_owned();
                if !out.iter().any(|(x, _)| *x == d) {
                    out.push((d, b));
                }
            }
        }
    }
    out.sort_by(|x, y| x.0.cmp(&y.0));
    out
}

/// Firmware project of `board`: the studio's own repo, a sibling of `near`
/// (…/ProjectSTM/F103C8T ↔ …/ProjectSTM/F411CE) or ~/Project/ProjectSTM/*.
pub fn guess_project(board: BoardId, near: &str) -> String {
    let mut roots: Vec<PathBuf> = vec![];
    let own = default_project_dir();
    if !own.is_empty() {
        if is_project_of(Path::new(&own), board) {
            return own;
        }
        roots.extend(Path::new(&own).parent().map(Path::to_path_buf));
    }
    roots.extend(Path::new(near).parent().map(Path::to_path_buf));
    if let Some(h) = dirs::home_dir() {
        roots.push(h.join("Project").join("ProjectSTM"));
    }
    for r in roots {
        let Ok(rd) = std::fs::read_dir(&r) else { continue };
        let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| is_project_of(p, board)).collect();
        dirs.sort();
        if let Some(d) = dirs.into_iter().next() {
            return d.canonicalize().unwrap_or(d).to_string_lossy().into_owned();
        }
    }
    String::new()
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
    Some(dirs::config_dir()?.join("pill-studio").join("config.json"))
}

/// Settings of the studio before it was renamed (F103 Studio ≤ 0.1.12).
fn old_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("f103-studio").join("config.json"))
}

impl Config {
    pub fn load() -> Config {
        let mut c: Config = path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .or_else(|| old_path().and_then(|p| std::fs::read_to_string(p).ok()))
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        c.ocd = c.ocd.for_this_machine();
        boards::set(c.board);
        if !is_project_of(Path::new(&c.project_dir), c.board) {
            let g = guess_project(c.board, &c.project_dir);
            if !g.is_empty() || c.project_dir.is_empty() || !Path::new(&c.project_dir).join("Core").exists() {
                c.project_dir = g;
            }
        }
        c.build_cmd = build_cmd_for(std::path::Path::new(&c.project_dir), &c.build_cmd);
        if c.recent.is_empty() {
            // first start with the project list: the other boards' projects, then this one on top
            for (key, s) in &c.slots {
                if let Some(b) = BoardId::ALL.into_iter().find(|b| b.key() == key) {
                    if is_project_of(Path::new(&s.project_dir), b) {
                        c.recent.push(Recent { dir: s.project_dir.clone(), board: b, build_cmd: s.build_cmd.clone(), used: 0 });
                    }
                }
            }
            c.touch_project();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_switch_board_and_keep_recent() {
        let root = std::env::temp_dir().join(format!("pill-proj-{}", std::process::id()));
        let mk = |name: &str, b: BoardId| {
            let d = root.join(name);
            std::fs::create_dir_all(d.join("Core")).unwrap();
            std::fs::write(d.join(b.profile().marker), "").unwrap();
            d.canonicalize().unwrap().to_string_lossy().into_owned()
        };
        let a = mk("a", BoardId::BluePill);
        let b = mk("b", BoardId::BlackPill);
        let mut c = Config { project_dir: a.clone(), ..Default::default() };
        assert_eq!(c.use_project(&b), Ok(true));
        assert_eq!((c.board, c.project_dir.as_str()), (BoardId::BlackPill, b.as_str()));
        assert_eq!(c.use_project(&a), Ok(true));
        assert_eq!(c.recent.iter().map(|r| r.dir.as_str()).collect::<Vec<_>>(), [a.as_str(), b.as_str()]);
        assert!(c.use_project(root.to_str().unwrap()).is_err());
        assert!(find_projects(&a).iter().any(|(d, x)| *d == b && *x == BoardId::BlackPill));
        for i in 0..70 {
            c.add_run(Run { t: i, dir: a.clone(), board: c.board, what: "Сборка".into(), ok: true, secs: 1.0, git: String::new() });
        }
        assert_eq!(c.history.len(), HISTORY_MAX);
        assert_eq!(c.history.last().unwrap().t, 69);
        let _ = std::fs::remove_dir_all(&root);
        boards::set(BoardId::BluePill);
    }

    #[test]
    fn find_tool_keeps_paths_and_resolves_names() {
        assert_eq!(find_tool("/x/openocd"), PathBuf::from("/x/openocd"));
        assert_eq!(find_tool("no-such-tool-xyz"), PathBuf::from("no-such-tool-xyz"));
        if cfg!(unix) {
            assert!(find_tool("sh").is_absolute());
        }
    }

    #[test]
    fn board_switch_keeps_each_boards_ports() {
        let mut c = Config { proto_port: "/dev/a".into(), ..Config::default() };
        c.switch_board(BoardId::BlackPill);
        assert_eq!(c.board, BoardId::BlackPill);
        assert_eq!(c.proto_port, "");
        c.proto_port = "/dev/b".into();
        c.switch_board(BoardId::BluePill);
        assert_eq!(c.proto_port, "/dev/a");
        c.switch_board(BoardId::BlackPill);
        assert_eq!(c.proto_port, "/dev/b");
        boards::set(BoardId::BluePill);
    }

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

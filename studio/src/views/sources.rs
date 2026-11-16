//! Read-only source browser with independent file tabs and follow-PC navigation.

use crate::app::Shared;
use crate::core::elf::{self, Elf, Place};
use crate::core::sources::{Folder, Index};
use crate::widgets::{card, hint};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};

#[derive(Clone, Copy, Default, PartialEq)]
enum Search {
    #[default]
    Files,
    Functions,
}

struct Tab {
    path: PathBuf,
    text: Result<Vec<String>, String>,
    line: Option<u32>,
    jump: bool,
}

impl Tab {
    fn read(path: &Path) -> Result<Vec<String>, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&bytes).lines().map(|s| s.replace('\t', "    ")).collect())
    }
}

#[derive(Default)]
pub struct St {
    root: PathBuf,
    elf: Option<Arc<Elf>>,
    tool_dir: String,
    index: Index,
    scan: Option<mpsc::Receiver<Index>>,
    search: Search,
    queries: [String; 2],
    focus_search: bool,
    tabs: Vec<Tab>,
    selected: Option<usize>,
    /// A function or stack frame. A file selected through its tab has no address.
    address: Option<u32>,
    following: bool,
    stop: Option<u32>,
    stop_line: Option<(PathBuf, u32)>,
    status: Option<String>,
}

impl St {
    pub fn update(&mut self, ctx: &egui::Context, sh: &Shared, stop: Option<u32>) -> (bool, bool) {
        let root = Path::new(&sh.cfg.project_dir);
        let root_changed = self.root != root;
        if root_changed {
            *self = Self { root: root.to_owned(), ..Self::default() };
        }
        let elf_changed = match (&self.elf, &sh.st.elf) {
            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
            (None, None) => false,
            _ => true,
        };
        if root_changed || elf_changed || self.tool_dir != sh.cfg.toolchain_dir {
            self.elf = sh.st.elf.clone();
            self.tool_dir.clone_from(&sh.cfg.toolchain_dir);
            self.index.functions.clear();
            self.index.symbol_error = None;
            self.address = None;
            self.stop_line = None;
            self.status = None;
            self.stop = None;
            self.refresh(ctx);
        }
        if let Some(scan) = &self.scan {
            match scan.try_recv() {
                Ok(index) => {
                    self.index = index;
                    self.scan = None;
                    if stop.is_some() && self.following && self.stop_line.is_none() {
                        self.follow_stop();
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.scan = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let new_stop = self.stop != stop && stop.is_some();
        if self.stop != stop {
            self.stop = stop;
            if stop.is_some() {
                self.follow_stop();
            }
        }
        (new_stop, elf_changed)
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let (root, elf, tools, ctx) = (self.root.clone(), self.elf.clone(), self.tool_dir.clone(), ctx.clone());
        self.scan = Some(rx);
        std::thread::spawn(move || {
            let index = Index::load(&root, elf.as_deref(), &tools);
            let _ = tx.send(index);
            ctx.request_repaint();
        });
        for tab in &mut self.tabs {
            tab.text = Tab::read(&tab.path);
        }
    }

    pub fn breakpoint_address(&self) -> Option<u32> {
        self.address.or(if self.following { self.stop } else { None })
    }

    pub fn follow_stop(&mut self) {
        self.following = true;
        self.address = None;
        self.stop_line = None;
        if let Some(addr) = self.stop {
            self.locate(addr, None);
            self.stop_line =
                self.selected.and_then(|n| self.tabs[n].line.map(|line| (self.tabs[n].path.clone(), line)));
        }
    }

    pub fn browse_address(&mut self, addr: u32) {
        self.following = false;
        self.address = Some(addr);
        self.locate(addr, None);
    }

    pub fn browsed_address(&self) -> Option<u32> {
        self.address
    }

    fn locate(&mut self, addr: u32, known: Option<Place>) {
        let place = known.or_else(|| self.elf.as_ref().and_then(|e| elf::place(e, &self.tool_dir, addr)));
        self.status = Some(match place {
            Some(place) => {
                if let Some(path) = self.index.resolve(&self.root, &place.file) {
                    self.open(path, Some(place.line));
                    return;
                }
                format!("Файл {} не найден", place.file)
            }
            None => format!("Для адреса 0x{addr:08X} нет строки в ELF"),
        });
        // Keep open tabs, but don't highlight an unrelated file as the stop.
        self.selected = None;
    }

    fn open(&mut self, path: PathBuf, line: Option<u32>) {
        let canonical = path.canonicalize().unwrap_or(path);
        let n = self.tabs.iter().position(|t| t.path == canonical).unwrap_or_else(|| {
            let text = Tab::read(&canonical);
            self.tabs.push(Tab { path: canonical, text, line: None, jump: false });
            self.tabs.len() - 1
        });
        if let Some(line) = line {
            self.tabs[n].line = Some(line);
            self.tabs[n].jump = true;
        }
        self.selected = Some(n);
        self.status = None;
    }

    fn browse_file(&mut self, path: PathBuf) {
        self.following = false;
        self.address = None;
        self.open(path, None);
    }

    fn close(&mut self, n: usize) {
        self.tabs.remove(n);
        self.selected = self.selected.and_then(|curr| {
            if curr < n {
                Some(curr)
            } else if curr > n {
                Some(curr - 1)
            } else {
                self.following = false;
                self.address = None;
                (!self.tabs.is_empty()).then(|| n.min(self.tabs.len() - 1))
            }
        });
    }
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let files = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::P);
    let functions = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::O);
    if ui.input_mut(|i| i.consume_shortcut(&files)) {
        st.search = Search::Files;
        st.focus_search = true;
    }
    if ui.input_mut(|i| i.consume_shortcut(&functions)) {
        st.search = Search::Functions;
        st.focus_search = true;
    }

    let height = (ui.ctx().content_rect().height() - 350.0).clamp(250.0, 650.0);
    if ui.available_width() >= 650.0 {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(245.0, height + 74.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(245.0);
                    navigator(ui, sh, st, height);
                },
            );
            ui.vertical(|ui| code(ui, sh, st, height));
        });
    } else {
        navigator(ui, sh, st, 170.0);
        ui.add_space(6.0);
        code(ui, sh, st, height);
    }
}

fn navigator(ui: &mut egui::Ui, sh: &Shared, st: &mut St, height: f32) {
    let p = sh.pal;
    card(ui, &p, "", |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut st.search, Search::Files, "Файлы").on_hover_text("Ctrl+P");
            ui.selectable_value(&mut st.search, Search::Functions, "Функции").on_hover_text("Ctrl+Shift+O");
            if ui.small_button("⟳").on_hover_text("Обновить дерево и открытые файлы").clicked()
            {
                st.refresh(ui.ctx());
            }
        });
        let prompt = if st.search == Search::Files {
            "Имя файла или путь"
        } else {
            "Имя функции или файл"
        };
        let query = &mut st.queries[st.search as usize];
        let search = ui.add(egui::TextEdit::singleline(query).hint_text(prompt).desired_width(ui.available_width()));
        if std::mem::take(&mut st.focus_search) {
            search.request_focus();
        }
        let enter = search.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let query = query.trim().to_lowercase();
        let mut file = None;
        let mut function = None;
        egui::ScrollArea::vertical()
            .id_salt(("source_tree", st.search == Search::Functions))
            .auto_shrink([false, false])
            .max_height(height)
            .show(ui, |ui| {
                if st.scan.is_some() && st.index.files.is_empty() && st.index.functions.is_empty() {
                    ui.spinner();
                    return;
                }
                let chosen = st.selected.map(|n| st.tabs[n].path.as_path());
                match st.search {
                    Search::Files if st.index.files.is_empty() => {
                        hint(ui, &p, "Откройте папку проекта на «Обзоре» или обновите список файлов.");
                    }
                    Search::Files if query.is_empty() => {
                        file = tree(ui, &st.root, &st.index.tree, chosen);
                    }
                    Search::Files => {
                        let mut matches = st
                            .index
                            .files
                            .iter()
                            .filter(|path| relative(&st.root, path).to_lowercase().contains(&query))
                            .peekable();
                        if matches.peek().is_none() {
                            hint(ui, &p, "Файлы не найдены");
                        }
                        for (n, path) in matches.enumerate() {
                            let title = relative(&st.root, path);
                            if ui.selectable_label(chosen == Some(path), title).clicked() || (enter && n == 0) {
                                file = Some(path.clone());
                            }
                        }
                    }
                    Search::Functions => {
                        if st.elf.is_none() {
                            hint(ui, &p, "Соберите прошивку или загрузите ELF на «Обзоре».");
                            return;
                        }
                        if let Some(error) = &st.index.symbol_error {
                            ui.label(
                                RichText::new("Проверьте arm-none-eabi-addr2line в настройках.").small().color(p.warn),
                            )
                            .on_hover_text(error);
                        }
                        let mut matches = st
                            .index
                            .functions
                            .iter()
                            .filter(|f| {
                                f.name.to_lowercase().contains(&query)
                                    || f.place.as_ref().is_some_and(|p| p.file.to_lowercase().contains(&query))
                            })
                            .peekable();
                        if matches.peek().is_none() {
                            hint(ui, &p, "Функции не найдены");
                        }
                        for (n, f) in matches.enumerate() {
                            let label = match &f.place {
                                Some(place) => format!(
                                    "{}\n{}:{}",
                                    f.name,
                                    place.file.rsplit(['/', '\\']).next().unwrap_or(&place.file),
                                    place.line
                                ),
                                None => f.name.clone(),
                            };
                            if ui
                                .selectable_label(st.address == Some(f.addr), RichText::new(label).monospace().small())
                                .clicked()
                                || (enter && n == 0)
                            {
                                function = Some((f.addr, f.place.clone()));
                            }
                        }
                    }
                }
            });
        if let Some(path) = file {
            st.browse_file(path);
        }
        if let Some((addr, place)) = function {
            st.following = false;
            st.address = Some(addr);
            st.locate(addr, place);
        }
    });
}

fn basename(path: &Path) -> String {
    path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned()
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn tree(ui: &mut egui::Ui, dir: &Path, folder: &Folder, chosen: Option<&Path>) -> Option<PathBuf> {
    let mut selected = None;
    for (name, folder) in &folder.folders {
        let path = dir.join(name);
        let open = matches!(name.as_str(), "Core" | "Src") || chosen.is_some_and(|c| c.starts_with(&path));
        if let Some(path) = egui::CollapsingHeader::new(name)
            .id_salt(&path)
            .default_open(open)
            .show(ui, |ui| tree(ui, &path, folder, chosen))
            .body_returned
            .flatten()
        {
            selected = Some(path);
        }
    }
    for path in &folder.files {
        if ui.selectable_label(chosen == Some(path), basename(path)).clicked() {
            selected = Some(path.clone());
        }
    }
    selected
}

fn code(ui: &mut egui::Ui, sh: &Shared, st: &mut St, height: f32) {
    let p = sh.pal;
    card(ui, &p, "", |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(st.stop.is_some(), egui::Button::new("К останову").small()).clicked() {
                st.follow_stop();
            }
            if let Some(addr) = st.address {
                ui.label(RichText::new(format!("0x{addr:08X}")).monospace().small().color(p.muted));
                let has = sh.st.breaks.contains(&(addr & !1));
                let label = if has { "Снять точку останова" } else { "Точка останова" };
                if ui.add_enabled(sh.swd_ok() && !sh.busy, egui::Button::new(label).small()).clicked() {
                    let job = if has { crate::worker::Job::SwdBpClr(addr) } else { crate::worker::Job::SwdBpSet(addr) };
                    sh.job(job);
                }
            }
        });
        let mut close = None;
        ui.horizontal_wrapped(|ui| {
            for (n, tab) in st.tabs.iter().enumerate() {
                ui.push_id(n, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(st.selected == Some(n), basename(&tab.path))
                            .on_hover_text(relative(&st.root, &tab.path))
                            .clicked()
                        {
                            st.selected = Some(n);
                            st.following = false;
                            st.address = None;
                            st.status = None;
                        }
                        if ui.small_button("×").on_hover_text("Закрыть вкладку").clicked() {
                            close = Some(n);
                        }
                    });
                });
            }
        });
        if let Some(n) = close {
            st.close(n);
        }
        if let Some(status) = &st.status {
            ui.label(RichText::new(status).color(p.warn));
        }
        let Some(n) = st.selected else {
            hint(ui, &p, "Выберите файл или функцию. При остановке откроется текущая строка программы.");
            return;
        };
        let tab = &mut st.tabs[n];
        let path = relative(&st.root, &tab.path);
        let pc_line =
            st.stop_line.as_ref().filter(|(file, _)| st.stop.is_some() && *file == tab.path).map(|(_, line)| *line);
        let label = match tab.line {
            Some(line) => format!("{path}:{line}"),
            None => path,
        };
        ui.label(RichText::new(label).monospace().small().color(p.muted));
        ui.separator();
        let rows = match &tab.text {
            Ok(rows) => rows,
            Err(error) => {
                ui.label(RichText::new(error).color(p.err));
                return;
            }
        };
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
        let step = row_height + ui.spacing().item_spacing.y;
        let mut scroll =
            egui::ScrollArea::both().id_salt(("source", &tab.path)).auto_shrink([false, false]).max_height(height);
        if std::mem::take(&mut tab.jump) {
            let focus = tab.line.unwrap_or(1).min(rows.len() as u32).saturating_sub(1);
            scroll = scroll.vertical_scroll_offset((focus as f32 * step - height * 0.35).max(0.0));
        }
        scroll.show_rows(ui, row_height, rows.len(), |ui, range| {
            for i in range {
                let line = i as u32 + 1;
                let (mark, color) = if pc_line == Some(line) {
                    ("▶", p.warn)
                } else if tab.line == Some(line) {
                    ("›", p.accent)
                } else {
                    (" ", p.fg2)
                };
                ui.add(
                    egui::Label::new(RichText::new(format!("{mark}{line:5}  {}", rows[i])).monospace().color(color))
                        .extend(),
                );
            }
        });
    });
}

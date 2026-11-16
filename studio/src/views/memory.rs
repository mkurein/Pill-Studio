//! Memory map by functions and variables (from the linker map), what changed
//! since the previous build, and the vector table with live NVIC state.

use crate::app::Shared;
use crate::core::boards;
use crate::core::layout::{self, Chunk, Group, Vector, MAX_SYSCALL_PRIO};
use crate::fmt;
use crate::theme::{mix, Pal};
use crate::widgets::*;
use crate::worker::Job;
use eframe::egui::{self, Color32, RichText, Sense, Stroke};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Map,
    Vectors,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Where {
    #[default]
    All,
    Flash,
    Ram,
}

#[derive(Default)]
pub struct St {
    tab: Tab,
    /// ELF the data below was built from: path and its build time.
    key: Option<(PathBuf, Option<u64>)>,
    chunks: Vec<Chunk>,
    from_map: bool,
    /// Pieces of the build before this one (same ELF path), for the diff.
    prev: Vec<Chunk>,
    vectors: Vec<Vector>,
    q: String,
    place: Where,
    by_addr: bool,
    whole: bool,
    sel: Option<u32>,
    only_used: bool,
}

const SCS_ISER: u32 = 0xE000_E100;
const SCS_ISPR: u32 = 0xE000_E200;
const SCS_IABR: u32 = 0xE000_E300;
const SCS_IPR: u32 = 0xE000_E400;
const SCB_ICSR: u32 = 0xE000_ED04;
const SCB_VTOR: u32 = 0xE000_ED08;
const SCB_SHPR1: u32 = 0xE000_ED18;
const SCB_SHCSR: u32 = 0xE000_ED24;

fn group_color(p: &Pal, g: Group) -> Color32 {
    match g {
        Group::App => p.accent,
        Group::Rtos => p.clk,
        Group::Hal => p.sig,
        Group::Startup => p.refc,
        Group::Libc => p.ctl,
        Group::Other => p.muted,
    }
}

/// The FreeRTOS heap, the fault record and the stack reserve stand out in RAM.
fn chunk_color(p: &Pal, c: &Chunk) -> Color32 {
    if c.name == "ucHeap" {
        p.dig
    } else if c.section == ".noinit" {
        p.warn
    } else if c.section == "._user_heap_stack" {
        p.pwr
    } else {
        group_color(p, c.group)
    }
}

fn reload(sh: &Shared, st: &mut St) {
    let Some(elf) = &sh.st.elf else {
        if st.key.is_some() {
            *st = St { tab: st.tab, ..Default::default() };
        }
        return;
    };
    let key = (elf.path.clone(), sh.st.elf_mtime);
    if st.key.as_ref() == Some(&key) {
        return;
    }
    let same_file = st.key.as_ref().is_some_and(|k| k.0 == key.0);
    let old = std::mem::take(&mut st.chunks);
    let map = std::fs::read_to_string(elf.path.with_extension("map")).map(|t| layout::parse_map(&t)).unwrap_or_default();
    st.from_map = !map.is_empty();
    st.chunks = if st.from_map { map } else { layout::from_symbols(elf) };
    if same_file && !old.is_empty() {
        if !layout::diff(&old, &st.chunks).is_empty() {
            st.prev = old;
        }
    } else if !same_file {
        st.prev.clear();
    }
    let names: HashMap<u32, String> = sh.svd.iter().flat_map(|p| p.irqs.iter().map(|(n, v)| (*v, n.clone()))).collect();
    st.vectors = layout::vectors(elf, &names);
    st.sel = None;
    st.key = Some(key);
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    reload(sh, st);
    egui::ScrollArea::vertical().show(ui, |ui| {
        h1(
            ui,
            &p,
            "Память и векторы",
            "Где во Flash и в RAM лежит каждая функция и переменная этой сборки, и куда ядро переходит по сбросу и прерываниям.",
        );
        let Some(elf) = sh.st.elf.clone() else {
            hint(ui, &p, "ELF не найден. Соберите прошивку или укажите ELF на «Обзоре».");
            return;
        };
        ui.horizontal(|ui| {
            ui.selectable_value(&mut st.tab, Tab::Map, "Карта памяти");
            ui.selectable_value(&mut st.tab, Tab::Vectors, "Векторы прерываний");
            let src = if st.from_map {
                format!("{} + .map", elf.path.display())
            } else {
                format!("{} — файла .map рядом нет, показаны только символы ELF без модулей", elf.path.display())
            };
            ui.label(RichText::new(src).small().color(p.muted));
        });
        super::fresh::bar(ui, sh);
        match st.tab {
            Tab::Map => map_tab(ui, sh, st),
            Tab::Vectors => vectors_tab(ui, sh, st),
        }
    });
}

/// Memory as rows of coloured pieces. Returns the piece clicked.
fn strip(ui: &mut egui::Ui, p: &Pal, chunks: &[&Chunk], lo: u32, hi: u32, sel: Option<u32>) -> Option<u32> {
    const ROWS: u32 = 6;
    const RH: f32 = 20.0;
    const GAP: f32 = 3.0;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, ROWS as f32 * (RH + GAP) - GAP), Sense::click());
    let painter = ui.painter_at(rect);
    let per_row = ((hi - lo) as f32 / ROWS as f32).max(1.0);
    let row_rect = |r: u32| egui::Rect::from_min_size(rect.min + egui::vec2(0.0, r as f32 * (RH + GAP)), egui::vec2(w, RH));
    for r in 0..ROWS {
        painter.rect_filled(row_rect(r), 3, p.panel2);
        let a = lo + (r as f32 * per_row) as u32;
        painter.text(row_rect(r).left_center() + egui::vec2(4.0, 0.0), egui::Align2::LEFT_CENTER, format!("{a:08X}"), egui::FontId::monospace(9.0), mix(p.muted, 55.0, p.panel2));
    }
    for (i, c) in chunks.iter().enumerate() {
        let col = chunk_color(p, c);
        // neighbours of one colour get two shades, so their border is visible
        let fill = if i % 2 == 0 { col } else { mix(col, 78.0, p.panel2) };
        let (a, b) = ((c.addr.max(lo) - lo) as f32, (c.end().min(hi).max(lo) - lo) as f32);
        let (r0, r1) = ((a / per_row) as u32, (((b - 1.0).max(a)) / per_row) as u32);
        for r in r0..=r1.min(ROWS - 1) {
            let rr = row_rect(r);
            let x0 = ((a - r as f32 * per_row) / per_row).clamp(0.0, 1.0) * w;
            let x1 = ((b - r as f32 * per_row) / per_row).clamp(0.0, 1.0) * w;
            let piece = egui::Rect::from_min_max(egui::pos2(rr.left() + x0, rr.top()), egui::pos2(rr.left() + x1.max(x0 + 1.0), rr.bottom()));
            painter.rect_filled(piece, 0, fill);
            if sel == Some(c.addr) {
                painter.rect_stroke(piece, 0, Stroke::new(2.0, p.fg), egui::StrokeKind::Inside);
            }
        }
    }
    let at = resp.hover_pos().and_then(|pos| {
        let r = ((pos.y - rect.top()) / (RH + GAP)) as u32;
        if r >= ROWS || pos.y - rect.top() - r as f32 * (RH + GAP) > RH {
            return None;
        }
        let addr = lo + ((r as f32 + (pos.x - rect.left()) / w) * per_row) as u32;
        chunks.iter().find(|c| addr >= c.addr && addr < c.end()).map(|c| (*c, addr))
    });
    if let Some((c, _)) = at {
        resp.clone().on_hover_ui_at_pointer(|ui| {
            ui.label(RichText::new(&c.name).strong());
            ui.label(RichText::new(format!("0x{:08X} … 0x{:08X} · {} Б", c.addr, c.end() - 1, c.size)).monospace());
            ui.label(format!("{} · {} · {}", c.section, c.module, c.group.ru()));
        });
        if resp.clicked() {
            return Some(c.addr);
        }
    }
    None
}

fn legend(ui: &mut egui::Ui, p: &Pal, chunks: &[&Chunk], extra: &[(&str, Color32, u32)]) {
    // the pieces with their own colour (heap, g_diag, stack reserve) are listed in `extra`
    let owned: Vec<Chunk> = chunks.iter().filter(|c| chunk_color(p, c) == group_color(p, c.group)).map(|c| (*c).clone()).collect();
    ui.horizontal_wrapped(|ui| {
        for (name, col, bytes) in extra {
            badge(ui, p, &format!("{name} {}", fmt::bytes(*bytes)), *col);
        }
        for (g, bytes) in layout::totals(&owned, 0, u32::MAX, |c| c.group) {
            badge(ui, p, &format!("{} {}", g.ru(), fmt::bytes(bytes)), group_color(p, g));
        }
    });
}

fn region_card(ui: &mut egui::Ui, sh: &Shared, st: &mut St, title: &str, base: u32, size: u32) {
    let p = sh.pal;
    let inside: Vec<&Chunk> = st.chunks.iter().filter(|c| c.addr >= base && c.addr < base + size).collect();
    let used_end = inside.iter().map(|c| c.end()).max().unwrap_or(base);
    let used: u32 = inside.iter().map(|c| c.size).sum();
    let frac = used as f32 / size as f32;
    let mut clicked = None;
    card(ui, &p, &format!("{title}: занято {} из {} КБ ({:.1} %)", fmt::bytes(used), size / 1024, frac * 100.0), |ui| {
        let special = |name: &str| inside.iter().filter(|c| c.name == name || c.section == name).map(|c| c.size).sum::<u32>();
        let mut extra = vec![];
        if base == boards::cur().ram.0 {
            extra.push(("куча FreeRTOS (ucHeap)", p.dig, special("ucHeap")));
            extra.push(("g_diag (.noinit, переживает сброс)", p.warn, special(".noinit")));
            extra.push(("резерв стека MSP и кучи libc", p.pwr, special("._user_heap_stack")));
        }
        legend(ui, &p, &inside, &extra);
        ui.add_space(4.0);
        // only the used part by default: 26 КБ of 512 would be a thin line
        let hi = if st.whole { base + size } else { (used_end + 0xFF) & !0xFF };
        clicked = strip(ui, &p, &inside, base, hi.max(base + 0x100), st.sel);
        ui.horizontal_wrapped(|ui| {
            hint(ui, &p, &format!("показано 0x{:08X} … 0x{:08X}; наведите на кусок — имя, адрес, размер, модуль", base, hi - 1));
        });
        if base == boards::cur().ram.0 {
            if let Some(&estack) = sh.st.elf.as_ref().and_then(|e| e.vectors.first()) {
                let msp = sh.st.regs.iter().find(|r| r.0 == "msp").map(|r| r.1);
                let mut s = format!(
                    "Главный стек (MSP) растёт вниз от 0x{:08X} — вершины RAM. Между концом данных (0x{:08X}) и вершиной свободно {}.",
                    estack,
                    used_end,
                    fmt::bytes(estack.saturating_sub(used_end))
                );
                if let Some((_, st)) = sh.st.stats {
                    s += &format!(" Пик использования с загрузки: {} из {} Б.", st.msp_size - st.msp_free.min(st.msp_size), st.msp_size);
                }
                if let Some(m) = msp.filter(|m| *m <= estack && *m > base) {
                    s += &format!(" Сейчас MSP = 0x{:08X}: занято стеком {} Б.", m, estack - m);
                }
                para(ui, &p, &s);
                hint(ui, &p, "MSP используют прерывания и код до запуска планировщика. Стеки задач FreeRTOS лежат внутри ucHeap; их остаток — на экране FreeRTOS.");
            }
        }
    });
    if clicked.is_some() {
        st.sel = clicked;
    }
}

fn map_tab(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let b = boards::cur();
    ui.checkbox(&mut st.whole, "показать всю память, а не только занятую часть");
    region_card(ui, sh, st, "Flash", b.flash.0, b.flash.1);
    ui.add_space(8.0);
    region_card(ui, sh, st, "RAM", b.ram.0, b.ram.1);
    ui.add_space(8.0);
    let (flo, fhi, rlo, rhi) = (b.flash.0, b.flash.0 + b.flash.1, b.ram.0, b.ram.0 + b.ram.1);
    ui.columns(2, |cols| {
        card(&mut cols[0], &p, "Что занимает место", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut st.q).hint_text("имя, модуль или секция…").desired_width(200.0));
                ui.selectable_value(&mut st.place, Where::All, "всё");
                ui.selectable_value(&mut st.place, Where::Flash, "Flash");
                ui.selectable_value(&mut st.place, Where::Ram, "RAM");
                ui.checkbox(&mut st.by_addr, "по адресу");
            });
            let q = st.q.to_lowercase();
            let mut rows: Vec<&Chunk> = st
                .chunks
                .iter()
                .filter(|c| match st.place {
                    Where::All => true,
                    Where::Flash => c.addr >= flo && c.addr < fhi,
                    Where::Ram => c.addr >= rlo && c.addr < rhi,
                })
                .filter(|c| q.is_empty() || c.name.to_lowercase().contains(&q) || c.module.to_lowercase().contains(&q) || c.section.contains(&q))
                .collect();
            if !st.by_addr {
                rows.sort_by_key(|c| std::cmp::Reverse(c.size));
            }
            let total: u32 = rows.iter().map(|c| c.size).sum();
            hint(ui, &p, &format!("{} кусков, {} — {}", rows.len(), fmt::bytes(total), if st.by_addr { "по адресу" } else { "самые большие сверху" }));
            let mut pick = None;
            egui::ScrollArea::vertical().id_salt("mem_rows").max_height(380.0).show(ui, |ui| {
                egui::Grid::new("mem_grid").num_columns(5).striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                    for h in ["имя", "адрес", "размер", "секция", "модуль"] {
                        ui.label(RichText::new(h).small().strong().color(p.fg2));
                    }
                    ui.end_row();
                    for c in rows.iter().take(400) {
                        let sel = st.sel == Some(c.addr);
                        if ui.selectable_label(sel, RichText::new(&c.name).monospace().small().color(chunk_color(&p, c))).clicked() {
                            pick = Some(c.addr);
                        }
                        ui.label(RichText::new(format!("{:08X}", c.addr)).monospace().small());
                        ui.label(RichText::new(c.size.to_string()).monospace().small());
                        ui.label(RichText::new(&c.section).small().color(p.fg2));
                        ui.label(RichText::new(&c.module).small().color(p.fg2));
                        ui.end_row();
                    }
                });
            });
            if pick.is_some() {
                st.sel = pick;
            }
        });
        card(&mut cols[1], &p, "По модулям", |ui| {
            let fl = layout::totals(&st.chunks, flo, fhi, |c| c.module.clone());
            let ram: HashMap<String, u32> = layout::totals(&st.chunks, rlo, rhi, |c| c.module.clone()).into_iter().collect();
            let mut names: Vec<(String, u32, u32)> = fl.iter().map(|(m, f)| (m.clone(), *f, ram.get(m).copied().unwrap_or(0))).collect();
            for (m, r) in &ram {
                if !fl.iter().any(|(x, _)| x == m) {
                    names.push((m.clone(), 0, *r));
                }
            }
            names.sort_by_key(|x| std::cmp::Reverse(x.1 + x.2));
            let mut pick = None;
            egui::ScrollArea::vertical().id_salt("mem_mods").max_height(410.0).show(ui, |ui| {
                egui::Grid::new("mod_grid").num_columns(3).striped(true).spacing([14.0, 2.0]).show(ui, |ui| {
                    for h in ["модуль", "Flash, Б", "RAM, Б"] {
                        ui.label(RichText::new(h).small().strong().color(p.fg2));
                    }
                    ui.end_row();
                    for (m, f, r) in &names {
                        if ui.link(RichText::new(m).small()).on_hover_text("показать куски этого модуля").clicked() {
                            pick = Some(m.clone());
                        }
                        ui.label(RichText::new(if *f > 0 { f.to_string() } else { "—".into() }).monospace().small());
                        ui.label(RichText::new(if *r > 0 { r.to_string() } else { "—".into() }).monospace().small());
                        ui.end_row();
                    }
                });
            });
            if let Some(m) = pick {
                st.q = m;
                st.place = Where::All;
            }
        });
    });
    ui.add_space(8.0);
    card(ui, &p, "Изменения с прошлой сборки", |ui| {
        if st.prev.is_empty() {
            hint(ui, &p, "Пересоберите прошивку, не закрывая студию: здесь появится, какие функции и переменные выросли, уменьшились, появились или исчезли.");
            return;
        }
        let d = layout::diff(&st.prev, &st.chunks);
        let sum = |v: &[Chunk], lo: u32, hi: u32| v.iter().filter(|c| c.addr >= lo && c.addr < hi).map(|c| c.size as i64).sum::<i64>();
        let (df, dr) = (sum(&st.chunks, flo, fhi) - sum(&st.prev, flo, fhi), sum(&st.chunks, rlo, rhi) - sum(&st.prev, rlo, rhi));
        ui.label(RichText::new(format!("Flash {:+} Б · RAM {:+} Б · изменилось кусков: {}", df, dr, d.len())).strong());
        egui::ScrollArea::vertical().id_salt("mem_diff").max_height(200.0).show(ui, |ui| {
            egui::Grid::new("diff_grid").num_columns(4).striped(true).spacing([14.0, 2.0]).show(ui, |ui| {
                for (name, old, new) in d.iter().take(100) {
                    let delta = *new as i64 - *old as i64;
                    ui.label(RichText::new(name).monospace().small());
                    ui.label(RichText::new(if *old == 0 { "новое".to_string() } else { old.to_string() }).monospace().small().color(p.fg2));
                    ui.label(RichText::new(if *new == 0 { "удалено".to_string() } else { new.to_string() }).monospace().small().color(p.fg2));
                    ui.label(RichText::new(format!("{delta:+}")).monospace().small().color(if delta > 0 { p.warn } else { p.ok }));
                    ui.end_row();
                }
            });
        });
    });
}

/// Addresses to read for the live NVIC view.
fn nvic_addrs(n_irq: u32, flash: u32, words: u32) -> Vec<u32> {
    let mut a = vec![SCB_ICSR, SCB_VTOR, SCB_SHPR1, SCB_SHPR1 + 4, SCB_SHPR1 + 8, SCB_SHCSR];
    for i in 0..n_irq.div_ceil(32) {
        a.extend([SCS_ISER + 4 * i, SCS_ISPR + 4 * i, SCS_IABR + 4 * i]);
    }
    a.extend((0..n_irq.div_ceil(4)).map(|i| SCS_IPR + 4 * i));
    a.extend((0..words).map(|i| flash + 4 * i));
    a
}

struct Live {
    enabled: bool,
    pending: bool,
    active: bool,
    prio: u32,
}

fn live(sh: &Shared, v: &Vector) -> Option<Live> {
    let bit = |base: u32, n: u32| sh.mem(base + 4 * (n / 32)).map(|w| w >> (n % 32) & 1 == 1);
    if let Some(n) = v.irq() {
        let prio = sh.mem(SCS_IPR + 4 * (n / 4))? >> (8 * (n % 4)) & 0xFF;
        return Some(Live { enabled: bit(SCS_ISER, n)?, pending: bit(SCS_ISPR, n)?, active: bit(SCS_IABR, n)?, prio: prio >> 4 });
    }
    let icsr = sh.mem(SCB_ICSR)?;
    let shcsr = sh.mem(SCB_SHCSR)?;
    // SHPR1..3 hold the priorities of exceptions 4..15, one byte each
    let prio = if v.num >= 4 { (sh.mem(SCB_SHPR1 + 4 * ((v.num as u32 - 4) / 4))? >> (8 * ((v.num as u32 - 4) % 4)) & 0xFF) >> 4 } else { 0 };
    let enabled = match v.num {
        4 => shcsr >> 16 & 1 == 1,
        5 => shcsr >> 17 & 1 == 1,
        6 => shcsr >> 18 & 1 == 1,
        _ => true,
    };
    let pending = match v.num {
        14 => icsr >> 28 & 1 == 1,
        15 => icsr >> 26 & 1 == 1,
        _ => false,
    };
    Some(Live { enabled, pending, active: icsr & 0x1FF == v.num as u32, prio })
}

fn vectors_tab(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let b = boards::cur();
    if st.vectors.is_empty() {
        hint(ui, &p, "В ELF нет секции .isr_vector.");
        return;
    }
    let n_irq = st.vectors.len().saturating_sub(16) as u32;
    let sim = sh.st.swd.as_deref() == Some("виртуальная плата");
    let have_live = sh.mem(SCB_VTOR).is_some() && !sim;
    card(ui, &p, "", |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(sh.swd_ok() && !sim, egui::Button::new("⟳ Прочитать NVIC по SWD")).on_hover_text("включено ли прерывание, приоритет, ожидает / выполняется, и совпадает ли таблица во Flash платы с этим ELF").clicked() {
                sh.job(Job::ReadWords(nvic_addrs(n_irq, b.flash.0, st.vectors.len() as u32)));
            }
            ui.checkbox(&mut st.only_used, "только используемые");
            let own = st.vectors.iter().filter(|v| v.num > 0 && !v.reserved && !v.default).count();
            ui.label(RichText::new(format!("{} записей · своих обработчиков {} · остальные → Default_Handler", st.vectors.len(), own)).color(p.fg2));
        });
        if sim {
            hint(ui, &p, "Виртуальная плата NVIC не моделирует: таблица показана из ELF.");
        } else if !sh.swd_ok() {
            hint(ui, &p, "Таблица взята из ELF. Подключите ST-Link (SWD-отладка), чтобы увидеть, какие прерывания включены и с каким приоритетом.");
        }
        if have_live {
            let vtor = sh.mem(SCB_VTOR).unwrap_or(0);
            let active = sh.mem(SCB_ICSR).unwrap_or(0) & 0x1FF;
            let mismatch = st.vectors.iter().enumerate().filter(|(i, v)| sh.mem(b.flash.0 + 4 * *i as u32).is_some_and(|w| w != v.value)).count();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("VTOR = 0x{vtor:08X}")).monospace());
                if vtor == 0 {
                    ui.label(RichText::new("адрес 0 — отражение начала Flash при BOOT0 = 0").color(p.fg2));
                } else if vtor != b.flash.0 {
                    ui.label(RichText::new("таблица перенесена: ядро берёт векторы не из начала Flash").color(p.warn));
                }
                ui.label(match active {
                    0 => RichText::new("ядро сейчас в коде задач (Thread mode)").color(p.fg2),
                    n => RichText::new(format!("ядро сейчас в обработчике № {n}")).color(p.warn),
                });
                if mismatch > 0 {
                    ui.label(RichText::new(format!("Во Flash платы {mismatch} векторов отличаются от ELF — прошита другая сборка")).color(p.err));
                } else {
                    ui.label(RichText::new("таблица во Flash платы совпадает с ELF").color(p.ok));
                }
            });
        }
    });
    ui.add_space(8.0);
    card(ui, &p, "Таблица векторов", |ui| {
        hint(
            ui,
            &p,
            &format!(
                "По сбросу ядро берёт SP из записи 0 и PC из записи 1. По прерыванию — адрес из записи 16 + номер IRQ. \
                 Приоритет: меньше число — срочнее; 0…{} не маскируются FreeRTOS, из них нельзя вызывать …FromISR.",
                MAX_SYSCALL_PRIO - 1
            ),
        );
        egui::Grid::new("vec_grid").num_columns(8).striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
            for h in ["№", "IRQ", "имя", "адрес во Flash", "значение", "обработчик", "вкл · приоритет", "замечание"] {
                ui.label(RichText::new(h).small().strong().color(p.fg2));
            }
            ui.end_row();
            for v in &st.vectors {
                let l = if have_live { live(sh, v) } else { None };
                let enabled = l.as_ref().is_some_and(|l| l.enabled);
                if st.only_used && (v.reserved || (v.default && !(enabled && v.irq().is_some()))) {
                    continue;
                }
                let dim = v.reserved || v.default;
                let c = if dim { p.muted } else { p.fg };
                ui.label(RichText::new(v.num.to_string()).monospace().small().color(c));
                ui.label(RichText::new(v.irq().map(|n| n.to_string()).unwrap_or_default()).monospace().small().color(c));
                let name = ui.label(RichText::new(&v.name).small().strong().color(if dim { p.muted } else { p.accent }));
                if !v.what.is_empty() {
                    name.on_hover_text(v.what);
                }
                ui.label(RichText::new(format!("{:08X}", b.flash.0 + 4 * v.num as u32)).monospace().small().color(p.fg2));
                ui.label(RichText::new(if v.reserved { "—".into() } else { format!("{:08X}", v.value) }).monospace().small().color(c));
                ui.label(RichText::new(&v.handler).monospace().small().color(c));
                let mut note: Option<(String, Color32)> = None;
                if sh.mem(b.flash.0 + 4 * v.num as u32).is_some_and(|w| w != v.value) && have_live {
                    note = Some((format!("во Flash платы {:08X}", sh.mem(b.flash.0 + 4 * v.num as u32).unwrap_or(0)), p.err));
                }
                match &l {
                    Some(l) if v.num >= 4 && !v.reserved => {
                        let state = if l.active {
                            " · выполняется"
                        } else if l.pending {
                            " · ожидает"
                        } else {
                            ""
                        };
                        let text = format!("{} · {}{}", if l.enabled { "вкл" } else { "выкл" }, l.prio, state);
                        ui.label(RichText::new(text).monospace().small().color(if l.enabled && !v.default { p.ok } else { p.muted }));
                        if v.irq().is_some() && note.is_none() {
                            if l.enabled && v.default {
                                note = Some(("включено, а обработчика нет: уйдёт в вечный цикл Default_Handler".into(), p.err));
                            } else if l.enabled && l.prio < MAX_SYSCALL_PRIO {
                                note = Some((format!("приоритет срочнее порога FreeRTOS ({MAX_SYSCALL_PRIO}): …FromISR вызывать нельзя"), p.warn));
                            } else if !l.enabled && !v.default {
                                note = Some(("обработчик есть, прерывание выключено".into(), p.muted));
                            }
                        }
                    }
                    _ => {
                        ui.label("");
                    }
                }
                if note.is_none() && !v.what.is_empty() && !v.reserved {
                    note = Some((v.what.to_string(), p.muted));
                }
                match note {
                    Some((t, col)) => ui.label(RichText::new(t).small().color(col)),
                    None => ui.label(""),
                };
                ui.end_row();
            }
        });
    });
}

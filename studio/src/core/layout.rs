//! Where the firmware's code and data live: input sections from the linker
//! map (`build/*.map`, one piece per function or variable with
//! -ffunction-sections), grouped by the object that brought them, and the
//! vector table taken from the ELF.

use super::elf::Elf;
use std::collections::HashMap;

/// Who a piece of memory belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    App,
    Rtos,
    Hal,
    Startup,
    Libc,
    Other,
}

impl Group {
    pub fn ru(self) -> &'static str {
        match self {
            Group::App => "приложение (Core/Src)",
            Group::Rtos => "FreeRTOS",
            Group::Hal => "HAL / CMSIS",
            Group::Startup => "startup",
            Group::Libc => "libc / libgcc",
            Group::Other => "прочее",
        }
    }

    fn of(object: &str) -> Group {
        let o = object.replace('\\', "/");
        if o.contains(".a(") || o.contains("/crt") {
            Group::Libc
        } else if o.contains("FreeRTOS") {
            Group::Rtos
        } else if o.to_lowercase().contains("startup") {
            Group::Startup
        } else if o.contains("/Drivers/") || o.contains("HAL_Driver") {
            Group::Hal
        } else if o.contains("Core/Src/") {
            Group::App
        } else {
            Group::Other
        }
    }
}

/// One input section placed by the linker.
#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub addr: u32,
    pub size: u32,
    /// Output section: ".text", ".bss" …
    pub section: String,
    /// Function / variable name, or the input section when it has no own name.
    pub name: String,
    /// "main", "heap_4", "libc_nano" — object file without path and extension.
    pub module: String,
    pub group: Group,
}

impl Chunk {
    pub fn end(&self) -> u32 {
        self.addr + self.size
    }
}

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.strip_prefix("0x")?, 16).ok()
}

fn module_of(object: &str) -> String {
    let o = object.replace('\\', "/");
    // libc_nano.a(libc_a-strlen.o) → libc_nano
    let file = match o.split_once(".a(") {
        Some((lib, _)) => lib.rsplit('/').next().unwrap_or(lib).to_string(),
        None => o.rsplit('/').next().unwrap_or(&o).trim_end_matches(".o").to_string(),
    };
    file
}

/// ".text.HAL_Init" → "HAL_Init"; ".rodata.str1.4" and bare ".text" keep the
/// section name with the module, since they have no symbol of their own.
fn chunk_name(input: &str, module: &str) -> String {
    // merged string literals: ".rodata.main.str1.4"
    if let Some(i) = input.find(".str1.") {
        let owner = input[..i].trim_start_matches(".rodata").trim_start_matches('.');
        return format!("строки: {}", if owner.is_empty() { module } else { owner });
    }
    for pre in [".text.", ".rodata.", ".data.", ".bss.", ".noinit."] {
        if let Some(rest) = input.strip_prefix(pre) {
            if !rest.is_empty() && !rest.starts_with("str") && !rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return rest.to_string();
            }
        }
    }
    format!("{input} ({module})")
}

/// Input sections with a real address from a GNU ld map file.
pub fn parse_map(text: &str) -> Vec<Chunk> {
    let Some(start) = text.find("Linker script and memory map") else { return vec![] };
    let mut out: Vec<Chunk> = vec![];
    let mut section = String::new();
    // an input section whose name was too long to share the line with its address
    let mut pending: Option<String> = None;
    // (name, addr, size) of an output section that got no input section yet
    let mut bare: Option<(String, u32, u32)> = None;
    let flush_bare = |bare: &mut Option<(String, u32, u32)>, out: &mut Vec<Chunk>| {
        if let Some((name, addr, size)) = bare.take() {
            if size > 0 && addr >= 0x0800_0000 {
                out.push(Chunk { addr, size, section: name.clone(), name, module: "компоновщик".into(), group: Group::Other });
            }
        }
    };
    for line in text[start..].lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        if t.is_empty() {
            continue;
        }
        if line.starts_with('.') {
            // output section: ".text  0x080001a0  0x612c [load address 0x…]", or the name alone
            flush_bare(&mut bare, &mut out);
            section = t[0].to_string();
            pending = None;
            if let (Some(a), Some(s)) = (t.get(1).and_then(|x| hex(x)), t.get(2).and_then(|x| hex(x))) {
                bare = Some((section.clone(), a, s));
                if let Some(i) = t.iter().position(|x| *x == "address") {
                    // initial values of .data stored in Flash
                    if let (Some(load), true) = (t.get(i + 1).and_then(|x| hex(x)), section == ".data") {
                        out.push(Chunk {
                            addr: load,
                            size: s,
                            section: ".data (образ)".into(),
                            name: "начальные значения .data".into(),
                            module: "компоновщик".into(),
                            group: Group::Other,
                        });
                    }
                }
            } else {
                pending = Some(String::new()); // its address is on the next line
            }
            continue;
        }
        if !line.starts_with(' ') {
            continue;
        }
        let input = line.starts_with(' ') && !line.starts_with("  ") && (t[0].starts_with('.') || t[0] == "COMMON");
        let (name, rest): (String, &[&str]) = if input {
            if t.len() == 1 {
                pending = Some(t[0].to_string());
                continue;
            }
            (t[0].to_string(), &t[1..])
        } else if let Some(p) = pending.take() {
            if p.is_empty() {
                // address of an output section named on the previous line
                if let (Some(a), Some(s)) = (hex(t[0]), t.get(1).and_then(|x| hex(x))) {
                    bare = Some((section.clone(), a, s));
                }
                continue;
            }
            (p, &t[..])
        } else {
            continue;
        };
        let (Some(addr), Some(size)) = (rest.first().and_then(|x| hex(x)), rest.get(1).and_then(|x| hex(x))) else { continue };
        if size == 0 || addr < 0x0800_0000 || rest.len() < 3 {
            continue;
        }
        let object = rest[2..].join(" ");
        let module = module_of(&object);
        bare = None;
        out.push(Chunk { addr, size, section: section.clone(), name: chunk_name(&name, &module), group: Group::of(&object), module });
    }
    flush_bare(&mut bare, &mut out);
    out.sort_by_key(|c| (c.addr, std::cmp::Reverse(c.size)));
    out.dedup_by(|b, a| a.addr == b.addr && a.size == b.size && a.section == b.section);
    // The linker merges equal string literals and still prints the sizes from
    // before the merge, so such pieces overlap: cut each at the next one.
    for i in 0..out.len().saturating_sub(1) {
        let next = out[i + 1].addr;
        if out[i].end() > next {
            out[i].size = next - out[i].addr;
        }
    }
    out.retain(|c| c.size > 0);
    out
}

/// Without a map file: the ELF's sized symbols, module unknown.
pub fn from_symbols(elf: &Elf) -> Vec<Chunk> {
    let mut v: Vec<Chunk> = elf
        .syms
        .iter()
        .map(|s| Chunk {
            addr: s.addr,
            size: s.size,
            section: elf.sections.iter().find(|x| s.addr >= x.addr && s.addr < x.addr + x.size).map(|x| x.name.clone()).unwrap_or_default(),
            name: s.name.clone(),
            module: "—".into(),
            group: Group::Other,
        })
        .collect();
    v.sort_by_key(|c| (c.addr, std::cmp::Reverse(c.size)));
    v.dedup_by(|b, a| a.addr == b.addr);
    v
}

/// Bytes per key inside `[lo, hi)`, biggest first.
pub fn totals<K: std::hash::Hash + Eq + Clone>(chunks: &[Chunk], lo: u32, hi: u32, key: impl Fn(&Chunk) -> K) -> Vec<(K, u32)> {
    let mut m: HashMap<K, u32> = HashMap::new();
    for c in chunks.iter().filter(|c| c.addr >= lo && c.addr < hi) {
        *m.entry(key(c)).or_default() += c.size;
    }
    let mut v: Vec<(K, u32)> = m.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    v
}

/// Size changes against an older build, by (section, name): (name, old, new), biggest change first.
pub fn diff(old: &[Chunk], new: &[Chunk]) -> Vec<(String, u32, u32)> {
    let sum = |v: &[Chunk]| {
        let mut m: HashMap<(String, String), u32> = HashMap::new();
        for c in v {
            *m.entry((c.section.clone(), c.name.clone())).or_default() += c.size;
        }
        m
    };
    let (a, b) = (sum(old), sum(new));
    let mut out: Vec<(String, u32, u32)> = vec![];
    for (k, &n) in &b {
        let o = a.get(k).copied().unwrap_or(0);
        if o != n {
            out.push((k.1.clone(), o, n));
        }
    }
    for (k, &o) in &a {
        if !b.contains_key(k) {
            out.push((k.1.clone(), o, 0));
        }
    }
    out.sort_by_key(|x| (std::cmp::Reverse((x.2 as i64 - x.1 as i64).abs()), x.0.clone()));
    out
}

/// One entry of the vector table.
#[derive(Clone, Debug, PartialEq)]
pub struct Vector {
    /// Exception number: 0 = initial SP, 1 = Reset, 16 = IRQ0.
    pub num: usize,
    pub name: String,
    pub what: &'static str,
    /// Word in the table (bit 0 = Thumb for handlers).
    pub value: u32,
    /// Function the vector points to, "" for SP and reserved entries.
    pub handler: String,
    /// Points to `Default_Handler`: nobody wrote a handler.
    pub default: bool,
    pub reserved: bool,
}

impl Vector {
    pub fn irq(&self) -> Option<u32> {
        self.num.checked_sub(16).map(|n| n as u32)
    }
}

const SYSTEM: [(&str, &str); 16] = [
    ("Начальный SP", "значение главного стека MSP после сброса (_estack, верх RAM)"),
    ("Reset", "точка входа после сброса: Reset_Handler"),
    ("NMI", "немаскируемое прерывание (сбой HSE при включённой CSS)"),
    ("HardFault", "любой сбой, который не обработан своим обработчиком"),
    ("MemManage", "нарушение защиты памяти (MPU), исполнение из запрещённой области"),
    ("BusFault", "ошибка шины: обращение по несуществующему адресу"),
    ("UsageFault", "неверная инструкция, деление на ноль, невыровненный доступ"),
    ("—", "зарезервировано"),
    ("—", "зарезервировано"),
    ("—", "зарезервировано"),
    ("—", "зарезервировано"),
    ("SVCall", "инструкция SVC: FreeRTOS запускает ею первую задачу"),
    ("DebugMon", "отладочный монитор"),
    ("—", "зарезервировано"),
    ("PendSV", "переключение задач FreeRTOS (самый низкий приоритет)"),
    ("SysTick", "системный таймер: тик FreeRTOS 1 мс"),
];

/// Vector table from the ELF's `.isr_vector`; `irq_names` come from the SVD.
pub fn vectors(elf: &Elf, irq_names: &HashMap<u32, String>) -> Vec<Vector> {
    elf.vectors
        .iter()
        .enumerate()
        .map(|(num, &value)| {
            let (name, what) = match SYSTEM.get(num) {
                Some((n, w)) => (n.to_string(), *w),
                None => (irq_names.get(&(num as u32 - 16)).cloned().unwrap_or_else(|| format!("IRQ{}", num - 16)), ""),
            };
            let target = value & !1;
            let reserved = num != 0 && value == 0;
            let default = num > 1 && !reserved && Some(target) == elf.default_handler;
            let handler = if num == 0 || reserved {
                String::new()
            } else if default {
                "Default_Handler".to_string()
            } else {
                // several symbols can share the address: take the one named after the vector
                let want = format!("{name}_IRQHandler");
                elf.syms
                    .iter()
                    .filter(|s| s.func && s.addr == target)
                    .map(|s| s.name.as_str())
                    .max_by_key(|n| *n == want)
                    .or_else(|| elf.func_at(target).map(|s| s.name.as_str()))
                    .unwrap_or("?")
                    .to_string()
            };
            Vector { num, name, what, value, handler, default, reserved }
        })
        .collect()
}

/// FreeRTOS: interrupts with a priority number below this (= more urgent) are
/// never masked by the kernel and must not call its API (`…FromISR`).
pub const MAX_SYSCALL_PRIO: u32 = 5;

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = "\
Discarded input sections

 .text._getpid  0x00000000        0x4 build/Core/Src/syscalls.o

Linker script and memory map

.isr_vector     0x08000000      0x198
 *(.isr_vector)
 .isr_vector    0x08000000      0x198 build/Core/Startup/startup_stm32f411ceux.o
                0x08000000                g_pfnVectors

.text           0x080001a0     0x612c
 .text          0x080001a0       0x10 /opt/arm/lib/libc_nano.a(libc_a-strlen.o)
                0x080001a0                strlen
 .text.calculate_crc_for_2_bytes
                0x080001b0       0x40 build/Core/Src/crc16.o
 .text.main     0x080001f0      0x100 build/Core/Src/main.o
                0x080001f0                main
 .text.xTaskCreate
                0x080002f0       0x80 build/Middlewares/Third_Party/FreeRTOS/Source/tasks.o
 .text.HAL_Init
                0x08000370       0x30 build/Drivers/STM32F4xx_HAL_Driver/Src/stm32f4xx_hal.o
 .text._kill    0x00000000       0x10 build/Core/Src/syscalls.o

.data           0x20000000       0x60 load address 0x080065ec
 .data.uwTickPrio
                0x20000000        0x4 build/Drivers/STM32F4xx_HAL_Driver/Src/stm32f4xx_hal.o

.bss            0x20000060     0x5844 load address 0x0800664c
 .bss.ucHeap    0x20002f54     0x2800 build/Middlewares/Third_Party/FreeRTOS/Source/portable/MemMang/heap_4.o

.noinit         0x200058a4       0x60 load address 0x0800664c
 .noinit        0x200058a4       0x60 build/Core/Src/diag.o
                0x200058a4                g_diag

._user_heap_stack
                0x20005904      0x604 load address 0x0800664c
                0x20005908                        . = ALIGN (0x8)

.ARM.attributes
                0x00000000       0x30
";

    #[test]
    fn map_pieces_and_groups() {
        let c = parse_map(MAP);
        let get = |n: &str| c.iter().find(|x| x.name == n).unwrap_or_else(|| panic!("{n}: {:?}", c.iter().map(|x| &x.name).collect::<Vec<_>>()));
        assert_eq!((get("main").addr, get("main").size, get("main").group), (0x0800_01f0, 0x100, Group::App));
        assert_eq!(get("main").module, "main");
        assert_eq!(get("calculate_crc_for_2_bytes").size, 0x40);
        assert_eq!(get("xTaskCreate").group, Group::Rtos);
        assert_eq!(get("HAL_Init").group, Group::Hal);
        assert_eq!(get(".text (libc_nano)").group, Group::Libc);
        assert_eq!(get(".isr_vector (startup_stm32f411ceux)").group, Group::Startup);
        assert_eq!((get("ucHeap").size, get("ucHeap").section.as_str()), (0x2800, ".bss"));
        assert_eq!(get(".noinit (diag)").addr, 0x2000_58a4);
        assert_eq!(get("начальные значения .data").addr, 0x0800_65ec);
        assert_eq!(get("._user_heap_stack").size, 0x604);
        // discarded sections (address 0) are not on the map
        assert!(!c.iter().any(|x| x.name == "_kill" || x.name == "_getpid"));
        let by_group = totals(&c, 0x0800_0000, 0x0810_0000, |x| x.group);
        assert_eq!(by_group[0], (Group::Startup, 0x198));
    }

    #[test]
    fn diff_lists_grown_new_and_removed() {
        let old = parse_map(MAP);
        let mut new = old.clone();
        new.iter_mut().find(|c| c.name == "main").unwrap().size += 0x20;
        new.retain(|c| c.name != "HAL_Init");
        let d = diff(&old, &new);
        assert_eq!(d[0], ("HAL_Init".to_string(), 0x30, 0));
        assert_eq!(d[1], ("main".to_string(), 0x100, 0x120));
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn vector_names_and_default_handler() {
        use crate::core::elf::Sym;
        let f = |name: &str, addr: u32| Sym { name: name.into(), addr, size: 8, func: true };
        let elf = Elf {
            syms: vec![f("Reset_Handler", 0x0800_0100), f("USART1_IRQHandler", 0x0800_0200), f("ADC_IRQHandler", 0x0800_0300)],
            default_handler: Some(0x0800_0300),
            vectors: {
                let mut v = vec![0x2002_0000, 0x0800_0101];
                v.resize(16, 0);
                v.push(0x0800_0301); // IRQ0: default
                v.push(0x0800_0201); // IRQ1
                v
            },
            ..Default::default()
        };
        let names: HashMap<u32, String> = [(1, "USART1".to_string())].into();
        let v = vectors(&elf, &names);
        assert_eq!((v[0].name.as_str(), v[0].value), ("Начальный SP", 0x2002_0000));
        assert_eq!(v[1].handler, "Reset_Handler");
        assert!(v[7].reserved);
        assert_eq!((v[16].name.as_str(), v[16].default, v[16].irq()), ("IRQ0", true, Some(0)));
        assert_eq!((v[17].name.as_str(), v[17].handler.as_str(), v[17].default), ("USART1", "USART1_IRQHandler", false));
    }

    /// `PILL_MAP=../../F411CE/build/primGPT_F411.map cargo test --release real_map -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_map() {
        let Ok(p) = std::env::var("PILL_MAP") else { return };
        let c = parse_map(&std::fs::read_to_string(p).unwrap());
        for (lo, hi, n) in [(0x0800_0000u32, 0x0900_0000u32, "flash"), (0x2000_0000, 0x3000_0000, "ram")] {
            println!("{n}: {} pieces, {:?}", c.iter().filter(|x| x.addr >= lo && x.addr < hi).count(), totals(&c, lo, hi, |x| x.group));
            println!("  {:?}", totals(&c, lo, hi, |x| x.module.clone()).into_iter().take(8).collect::<Vec<_>>());
        }
        let overlap = c.windows(2).filter(|w| w[0].end() > w[1].addr).count();
        for w in c.windows(2).filter(|w| w[0].end() > w[1].addr) {
            println!("  overlap {:08x}+{:x} {} / {:08x} {}", w[0].addr, w[0].size, w[0].name, w[1].addr, w[1].name);
        }
        println!("overlaps: {overlap}");
    }
}

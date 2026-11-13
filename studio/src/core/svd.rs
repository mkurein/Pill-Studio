//! CMSIS-SVD of the board's family (repo root `STM32F103.svd` / `STM32F411.svd`,
//! embedded at compile time, see `boards.rs`) → peripherals / registers / fields
//! for the register browser.

use std::collections::HashMap;


#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub desc: String,
    pub offset: u32,
    pub width: u32,
    pub access: String,
}

impl Field {
    pub fn mask(&self) -> u32 {
        let m = if self.width >= 32 { u32::MAX } else { (1u32 << self.width) - 1 };
        m << self.offset
    }
    pub fn get(&self, v: u32) -> u32 {
        (v & self.mask()) >> self.offset
    }
}

#[derive(Clone, Debug)]
pub struct Register {
    pub name: String,
    pub desc: String,
    pub offset: u32,
    pub size: u32,
    pub access: String,
    pub reset: u32,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug)]
pub struct Peripheral {
    pub name: String,
    pub desc: String,
    pub group: String,
    pub base: u32,
    pub registers: Vec<Register>,
    pub irqs: Vec<(String, u32)>,
    /// Physically present in this exact part (the SVD covers the whole line).
    pub in_part: bool,
}

impl Peripheral {
    pub fn addr(&self, r: &Register) -> u32 {
        self.base + r.offset
    }
}

/// Reading this register changes hardware state (clears flags, pops FIFO):
/// never read it automatically, the firmware would lose data.
pub fn read_has_side_effect(periph: &str, reg: &str) -> bool {
    let p = periph.to_ascii_uppercase();
    let r = reg.to_ascii_uppercase();
    let comm = p.starts_with("USART") || p.starts_with("UART") || p.starts_with("SPI") || p.starts_with("I2C");
    (comm && (r == "DR" || r == "SR" || r == "SR1" || r == "SR2"))
        || (p.starts_with("ADC") && (r == "DR" || r.starts_with("JDR")))
        || (p == "CAN" && (r.starts_with("RF") || r.starts_with("RI")))
}

fn text<'a>(n: roxmltree::Node<'a, 'a>, tag: &str) -> Option<&'a str> {
    n.children().find(|c| c.has_tag_name(tag)).and_then(|c| c.text())
}

fn num(s: Option<&str>) -> Option<u32> {
    let s = s?.trim();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(h, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn clean(s: Option<&str>) -> String {
    s.unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Peripherals of the board's SVD, those present in the part first.
pub fn parse_board(b: &super::boards::Profile) -> Vec<Peripheral> {
    parse(b.svd, b.present).unwrap_or_default()
}

pub fn parse(svd: &str, present: &[&str]) -> Result<Vec<Peripheral>, String> {
    let doc = roxmltree::Document::parse(svd).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    let periphs = root.children().find(|c| c.has_tag_name("peripherals")).ok_or("в SVD нет <peripherals>")?;

    let mut out: Vec<Peripheral> = vec![];
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for p in periphs.children().filter(|c| c.has_tag_name("peripheral")) {
        let name = clean(text(p, "name"));
        let base = num(text(p, "baseAddress")).unwrap_or(0);
        let derived = p.attribute("derivedFrom").and_then(|d| by_name.get(d).map(|&i| out[i].clone()));
        let mut registers = vec![];
        if let Some(regs) = p.children().find(|c| c.has_tag_name("registers")) {
            for r in regs.children().filter(|c| c.has_tag_name("register")) {
                let mut fields = vec![];
                if let Some(fs) = r.children().find(|c| c.has_tag_name("fields")) {
                    for f in fs.children().filter(|c| c.has_tag_name("field")) {
                        fields.push(Field {
                            name: clean(text(f, "name")),
                            desc: clean(text(f, "description")),
                            offset: num(text(f, "bitOffset")).unwrap_or(0),
                            width: num(text(f, "bitWidth")).unwrap_or(1),
                            access: clean(text(f, "access")),
                        });
                    }
                }
                fields.sort_by_key(|f| std::cmp::Reverse(f.offset));
                registers.push(Register {
                    name: clean(text(r, "name")),
                    desc: clean(text(r, "description")),
                    offset: num(text(r, "addressOffset")).unwrap_or(0),
                    size: num(text(r, "size")).unwrap_or(32),
                    access: clean(text(r, "access")),
                    reset: num(text(r, "resetValue")).unwrap_or(0),
                    fields,
                });
            }
        }
        let irqs: Vec<(String, u32)> = p
            .children()
            .filter(|c| c.has_tag_name("interrupt"))
            .map(|i| (clean(text(i, "name")), num(text(i, "value")).unwrap_or(0)))
            .collect();
        let mut per = Peripheral {
            in_part: present.contains(&name.as_str()),
            name: name.clone(),
            desc: clean(text(p, "description")),
            group: clean(text(p, "groupName")),
            base,
            registers,
            irqs,
        };
        if let Some(d) = derived {
            if per.registers.is_empty() {
                per.registers = d.registers;
            }
            if per.desc.is_empty() {
                per.desc = d.desc;
            }
            if per.group.is_empty() {
                per.group = d.group;
            }
        }
        per.registers.sort_by_key(|r| r.offset);
        by_name.insert(name, out.len());
        out.push(per);
    }
    out.sort_by(|a, b| b.in_part.cmp(&a.in_part).then(a.base.cmp(&b.base)));
    Ok(out)
}

/// RM0008 GPIO CRL/CRH 4-bit pin config → Russian.
pub fn gpio_mode_ru(mode: u32, cnf: u32) -> &'static str {
    match (mode, cnf) {
        (0, 0) => "вход аналоговый",
        (0, 1) => "вход плавающий",
        (0, 2) => "вход с подтяжкой (ODR: 1 вверх / 0 вниз)",
        (0, 3) => "резерв",
        (_, 0) => "выход push-pull",
        (_, 1) => "выход open-drain",
        (_, 2) => "альт. функция push-pull",
        (_, _) => "альт. функция open-drain",
    }
}

/// RM0383 §8.4: 2-bit fields of MODER / OSPEEDR / PUPDR → Russian.
pub fn gpio_f4_ru(reg: &str, v: u32) -> &'static str {
    match (reg, v & 3) {
        ("MODER", 0) => "вход",
        ("MODER", 1) => "выход",
        ("MODER", 2) => "альт. функция",
        ("MODER", _) => "аналог",
        ("OSPEEDR", 0) => "низкая скорость",
        ("OSPEEDR", 1) => "средняя скорость",
        ("OSPEEDR", 2) => "быстрая",
        ("OSPEEDR", _) => "очень быстрая",
        ("PUPDR", 0) => "без подтяжки",
        ("PUPDR", 1) => "подтяжка вверх",
        ("PUPDR", 2) => "подтяжка вниз",
        ("PUPDR", _) => "резерв",
        _ => "",
    }
}

pub fn gpio_speed_ru(mode: u32) -> &'static str {
    match mode {
        1 => "10 МГц",
        2 => "2 МГц",
        3 => "50 МГц",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_embedded_svd() {
        let ps = parse_board(&super::super::boards::BLUE_PILL);
        assert!(ps.len() > 40);
        let usart1 = ps.iter().find(|p| p.name == "USART1").unwrap();
        assert_eq!(usart1.base, 0x4001_3800);
        let brr = usart1.registers.iter().find(|r| r.name == "BRR").unwrap();
        assert_eq!(brr.offset, 8);
        // derived peripheral inherits registers
        let gpioc = ps.iter().find(|p| p.name == "GPIOC").unwrap();
        assert_eq!(gpioc.base, 0x4001_1000);
        let odr = gpioc.registers.iter().find(|r| r.name == "ODR").unwrap();
        assert_eq!(odr.offset, 0x0C);
        assert!(odr.fields.iter().any(|f| f.name == "ODR13" && f.offset == 13));
        let crl = gpioc.registers.iter().find(|r| r.name == "CRL").unwrap();
        assert_eq!(crl.reset, 0x4444_4444);
        assert!(gpioc.in_part);
        assert!(!ps.iter().find(|p| p.name == "TIM8").map(|p| p.in_part).unwrap_or(true));
    }

    #[test]
    fn parses_f411_svd() {
        let ps = parse_board(&super::super::boards::BLACK_PILL);
        let gpioc = ps.iter().find(|p| p.name == "GPIOC").unwrap();
        assert_eq!(gpioc.base, 0x4002_0800);
        let odr = gpioc.registers.iter().find(|r| r.name == "ODR").unwrap();
        assert_eq!(odr.offset, 0x14);
        assert!(gpioc.registers.iter().any(|r| r.name == "MODER"));
        assert_eq!(ps.iter().find(|p| p.name == "USART6").unwrap().base, 0x4001_1400);
        assert!(!ps.iter().find(|p| p.name == "GPIOD").unwrap().in_part);
        assert!(ps.iter().find(|p| p.name == "SPI5").unwrap().in_part);
    }

    #[test]
    fn field_math() {
        let f = Field { name: "X".into(), desc: String::new(), offset: 4, width: 2, access: String::new() };
        assert_eq!(f.mask(), 0x30);
        assert_eq!(f.get(0x20), 2);
    }

    #[test]
    fn side_effects() {
        assert!(read_has_side_effect("USART1", "DR"));
        assert!(!read_has_side_effect("USART1", "BRR"));
        assert!(!read_has_side_effect("GPIOC", "ODR"));
    }
}

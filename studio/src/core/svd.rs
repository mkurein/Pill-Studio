//! CMSIS-SVD of the STM32F103xx family (repo root `STM32F103.svd`, embedded at
//! compile time) → peripherals / registers / fields for the register browser.

use std::collections::HashMap;

pub const SVD_TEXT: &str = include_str!("../../../STM32F103.svd");

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
    /// Present in the STM32F103C8 (medium density, LQFP48).
    pub in_c8: bool,
}

impl Peripheral {
    pub fn addr(&self, r: &Register) -> u32 {
        self.base + r.offset
    }
}

/// Peripherals that physically exist in STM32F103C8 (DS5319 Table 2 / Figure 1).
pub const C8_PERIPHERALS: &[&str] = &[
    "RCC", "FLASH", "PWR", "BKP", "RTC", "IWDG", "WWDG", "GPIOA", "GPIOB", "GPIOC", "GPIOD", "AFIO", "EXTI",
    "DMA1", "TIM1", "TIM2", "TIM3", "TIM4", "USART1", "USART2", "USART3", "SPI1", "SPI2", "I2C1", "I2C2",
    "ADC1", "ADC2", "CAN", "USB", "CRC", "DBG", "DBGMCU", "NVIC", "SCB", "STK", "MPU", "SCB_ACTRL",
    "NVIC_STIR",
];

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

pub fn parse(svd: &str) -> Result<Vec<Peripheral>, String> {
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
            in_c8: C8_PERIPHERALS.contains(&name.as_str()),
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
    out.sort_by(|a, b| b.in_c8.cmp(&a.in_c8).then(a.base.cmp(&b.base)));
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
        let ps = parse(SVD_TEXT).unwrap();
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
        assert!(gpioc.in_c8);
        assert!(!ps.iter().find(|p| p.name == "TIM8").map(|p| p.in_c8).unwrap_or(true));
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

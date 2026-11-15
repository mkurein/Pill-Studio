//! Domain logic without any UI: protocol, SVD, chip knowledge, SWD, ELF,
//! serial links and the virtual board. Unit-tested with `cargo test`.

pub mod boards;
pub mod boot;
pub mod config;
pub mod debug;
pub mod elf;
pub mod faults;
pub mod layout;
pub mod link;
pub mod mcu;
pub mod mcu_f411;
pub mod openocd;
pub mod proto;
pub mod sim;
pub mod svd;

/// `g_diag` from Core/Inc/diag.h (24 words, .noinit RAM). Keep in sync.
#[derive(Clone, Debug, Default)]
pub struct Diag {
    pub magic: u32,
    pub boot_count: u32,
    pub reset_cause: u32,
    pub heartbeat: u32,
    pub fault_magic: u32,
    pub error_code: u32,
    pub fault_pc: u32,
    pub fault_lr: u32,
    pub fault_cfsr: u32,
    pub fault_hfsr: u32,
    pub fault_bfar: u32,
    pub fault_mmfar: u32,
    pub fault_task: String,
    pub rx_frames: u32,
    pub rx_crc_err: u32,
    pub rx_overflow: u32,
    pub uart_err: u32,
    pub queue_full: u32,
    pub last_cmd: u32,
    pub fram_ok: u32,
    pub fram_fail: u32,
    /// Source line of a failed configASSERT (file name is in `fault_task`).
    pub fault_line: u32,
    pub rst_soft: u32,
    pub rst_pin: u32,
    pub rst_power: u32,
    pub rst_wdg: u32,
    pub queue_peak: u32,
    /// Tasks that stopped checking in before a watchdog reset: 1 default, 2 LED, 4 UART.
    pub wdg_missing: u32,
    /// Supply dips below 2.9 V since power-on, and the HAL tick of the last one.
    pub pvd_count: u32,
    pub pvd_ms: u32,
}

pub const DIAG_MAGIC: u32 = 0xD1A6_0004;
/// Firmware before the reset counters: the first 24 words are the same.
pub const DIAG_MAGIC_V2: u32 = 0xD1A6_0002;
pub const DIAG_FAULT_MAGIC: u32 = 0x0FA0_17ED;
pub const DIAG_WORDS: u32 = 33;

impl Diag {
    pub fn from_words(w: &[u32]) -> Option<Diag> {
        if w.len() < 24 {
            return None;
        }
        // words 24.. exist only in the v3 record
        let x = |i: usize| if w[0] == DIAG_MAGIC { w.get(i).copied().unwrap_or(0) } else { 0 };
        let task: Vec<u8> = w[12..16].iter().flat_map(|x| x.to_le_bytes()).collect();
        let task = String::from_utf8_lossy(task.split(|&b| b == 0).next().unwrap_or(&[])).to_string();
        Some(Diag {
            magic: w[0],
            boot_count: w[1],
            reset_cause: w[2],
            heartbeat: w[3],
            fault_magic: w[4],
            error_code: w[5],
            fault_pc: w[6],
            fault_lr: w[7],
            fault_cfsr: w[8],
            fault_hfsr: w[9],
            fault_bfar: w[10],
            fault_mmfar: w[11],
            fault_task: task,
            rx_frames: w[16],
            rx_crc_err: w[17],
            rx_overflow: w[18],
            uart_err: w[19],
            queue_full: w[20],
            last_cmd: w[21],
            fram_ok: w[22],
            fram_fail: w[23],
            fault_line: x(24),
            rst_soft: x(25),
            rst_pin: x(26),
            rst_power: x(27),
            rst_wdg: x(28),
            queue_peak: x(29),
            wdg_missing: x(30),
            pvd_count: x(31),
            pvd_ms: x(32),
        })
    }
    pub fn valid(&self) -> bool {
        self.magic == DIAG_MAGIC || self.magic == DIAG_MAGIC_V2
    }
    /// The record has reset counters and the assert line (firmware with TASKS).
    pub fn has_counters(&self) -> bool {
        self.magic == DIAG_MAGIC
    }
    /// Names of the tasks in `wdg_missing`.
    pub fn wdg_missing_ru(&self) -> String {
        let v: Vec<&str> = [(1, "defaultTask"), (2, "LedTask"), (4, "Uart1Task")].iter().filter(|(b, _)| self.wdg_missing & b != 0).map(|(_, n)| *n).collect();
        if v.is_empty() {
            "не определена (завис сам контроль, сбой с выключенными прерываниями или проверка assert)".into()
        } else {
            v.join(", ")
        }
    }
    pub fn has_fault(&self) -> bool {
        self.fault_magic == DIAG_FAULT_MAGIC
    }
    pub fn error_name(&self) -> &'static str {
        match self.error_code {
            0 => "нет",
            1 => "Error_Handler (ошибка инициализации HAL)",
            2 => "HardFault",
            3 => "MemManage",
            4 => "BusFault",
            5 => "UsageFault",
            6 => "переполнение стека задачи",
            7 => "нехватка кучи FreeRTOS",
            8 => "configASSERT — проверка в коде FreeRTOS не прошла",
            _ => "неизвестно",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diag_decoding() {
        let mut w = vec![0u32; 24];
        w[0] = DIAG_MAGIC;
        w[4] = DIAG_FAULT_MAGIC;
        w[5] = 6;
        w[12] = u32::from_le_bytes(*b"Uart");
        w[13] = u32::from_le_bytes(*b"1Tas");
        w[14] = u32::from_le_bytes([b'k', 0, 0, 0]);
        let d = Diag::from_words(&w).unwrap();
        assert!(d.valid() && d.has_fault());
        assert_eq!(d.fault_task, "Uart1Task");
        assert!(d.error_name().contains("стека"));
        assert!(d.has_counters());
        // an older record: counters read as zero
        w.resize(33, 7);
        w[0] = DIAG_MAGIC;
        let new = Diag::from_words(&w).unwrap();
        assert_eq!((new.wdg_missing, new.pvd_count), (7, 7));
        assert_eq!(new.wdg_missing_ru(), "defaultTask, LedTask, Uart1Task");
        w[0] = DIAG_MAGIC_V2;
        let old = Diag::from_words(&w).unwrap();
        assert!(old.valid() && !old.has_counters());
        assert_eq!((old.rst_soft, old.queue_peak), (0, 0));
    }
}

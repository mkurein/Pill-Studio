//! Domain logic without any UI: protocol, SVD, chip knowledge, SWD, ELF,
//! serial links and the virtual board. Unit-tested with `cargo test`.

pub mod boards;
pub mod config;
pub mod debug;
pub mod elf;
pub mod faults;
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
}

pub const DIAG_MAGIC: u32 = 0xD1A6_0002;
pub const DIAG_FAULT_MAGIC: u32 = 0x0FA0_17ED;
pub const DIAG_WORDS: u32 = 24;

impl Diag {
    pub fn from_words(w: &[u32]) -> Option<Diag> {
        if w.len() < DIAG_WORDS as usize {
            return None;
        }
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
        })
    }
    pub fn valid(&self) -> bool {
        self.magic == DIAG_MAGIC
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
    }
}

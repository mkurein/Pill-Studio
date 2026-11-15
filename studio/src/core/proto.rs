//! USART1 binary protocol of the firmware. Mirror of `Core/Inc/DataFile.h`,
//! `docs/PROTOCOL.md` and `tools/hil.py` — keep all four in sync.

use serde::Serialize;

pub const PROTO_BAUD: u32 = 38400;
pub const LOG_BAUD: u32 = 115200;
pub const ID_BU: u8 = 0x01;
pub const UART_BUF_SIZE: usize = 100;

pub const CMD_PING: u8 = 0x01;
pub const CMD_FRAM: u8 = 0x02;
pub const CMD_STATUS: u8 = 0x03;
pub const CMD_TASKS: u8 = 0x04;
pub const CMD_STATS: u8 = 0x05;
pub const CMD_RESET: u8 = 0x0F;
pub const CMD_LED: u8 = 0x70;
pub const CMD_LED_GET: u8 = 0x71;
/// Fault injection: 0 clear the record, 1 stall the UART task, 2 assert, 3 HardFault, 4 fake PVD.
pub const CMD_FAULT: u8 = 0x7F;

pub const ST_OK: u8 = 0x00;
pub const ST_FAIL: u8 = 0x01;
pub const ST_BAD_ARG: u8 = 0x02;
pub const ST_BAD_CRC: u8 = 0xE1;
pub const ST_UNKNOWN_CMD: u8 = 0xE2;
pub const ST_OVERFLOW: u8 = 0xE3;

/// CRC-16/MODBUS: poly 0xA001 (reflected), init 0xFFFF. Sent LSB first.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
        }
    }
    crc
}

/// Append CRC16 to a request body.
pub fn frame(body: &[u8]) -> Vec<u8> {
    let c = crc16(body);
    let mut v = body.to_vec();
    v.push(c as u8);
    v.push((c >> 8) as u8);
    v
}

pub fn status_name(st: u8) -> &'static str {
    match st {
        ST_OK => "OK",
        ST_FAIL => "FAIL",
        ST_BAD_ARG => "BAD_ARG",
        ST_BAD_CRC => "BAD_CRC",
        ST_UNKNOWN_CMD => "UNKNOWN_CMD",
        ST_OVERFLOW => "OVERFLOW",
        _ => "?",
    }
}

pub fn status_ru(st: u8) -> &'static str {
    match st {
        ST_OK => "выполнено",
        ST_FAIL => "проверка не прошла",
        ST_BAD_ARG => "неверный аргумент",
        ST_BAD_CRC => "неверная CRC кадра",
        ST_UNKNOWN_CMD => "неизвестная команда",
        ST_OVERFLOW => "кадр длиннее 100 байт",
        _ => "неизвестный статус",
    }
}

/// Short Russian name of a request, for the transaction log.
pub fn describe_request(body: &[u8]) -> String {
    let arg = body.get(1).copied();
    match (body.first().copied(), arg) {
        (None, _) => "пустой кадр".into(),
        (Some(CMD_PING), _) => "PING — версия прошивки".into(),
        (Some(CMD_STATUS), _) => "STATUS — состояние RTOS".into(),
        (Some(CMD_TASKS), _) => "TASKS — задачи FreeRTOS".into(),
        (Some(CMD_STATS), _) => "STATS — прерывания и главный стек".into(),
        (Some(CMD_RESET), _) => "RESET — программный сброс".into(),
        (Some(CMD_LED), Some(0)) => "LED выкл (PC13 = 1)".into(),
        (Some(CMD_LED), Some(1)) => "LED вкл (PC13 = 0)".into(),
        (Some(CMD_LED), Some(2)) => "LED авто-мигание".into(),
        (Some(CMD_LED), _) => "LED (аргумент?)".into(),
        (Some(CMD_LED_GET), _) => "LED_GET — состояние PC13".into(),
        (Some(CMD_FAULT), Some(0)) => "FAULT 0 — стереть запись о сбое".into(),
        (Some(CMD_FAULT), Some(1)) => "FAULT 1 — остановить задачу UART (сторож)".into(),
        (Some(CMD_FAULT), Some(2)) => "FAULT 2 — configASSERT".into(),
        (Some(CMD_FAULT), Some(3)) => "FAULT 3 — HardFault".into(),
        (Some(CMD_FAULT), Some(4)) => "FAULT 4 — имитация просадки (PVD)".into(),
        (Some(CMD_FAULT), _) => "FAULT (аргумент?)".into(),
        (Some(CMD_FRAM), Some(0) | None) => "FRAM: запись/чтение \"KUREIN\"".into(),
        (Some(CMD_FRAM), Some(1)) => "FRAM: проверка присутствия (WEL)".into(),
        (Some(CMD_FRAM), Some(2)) => "FRAM: тест 64 байт".into(),
        (Some(c), _) => format!("команда 0x{:02X}", c),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Reply {
    pub id: u8,
    pub status: u8,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Decoded {
    Ok(Reply),
    /// Nothing came back within the timeout.
    Silent,
    /// Bytes came back but do not form a valid reply.
    Garbage(String),
}

pub fn decode_reply(raw: &[u8]) -> Decoded {
    if raw.is_empty() {
        return Decoded::Silent;
    }
    if raw.len() < 4 {
        return Decoded::Garbage(format!("ответ короче 4 байт ({} B)", raw.len()));
    }
    if crc16(raw) != 0 {
        return Decoded::Garbage("CRC ответа не сходится".into());
    }
    Decoded::Ok(Reply { id: raw[0], status: raw[1], payload: raw[2..raw.len() - 2].to_vec() })
}

/// STATS reply: main stack high-water mark and activity counters since boot.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Stats {
    /// Bytes of the watched main-stack area never used, and the area's size.
    pub msp_free: u16,
    pub msp_size: u16,
    pub ctx_switches: u32,
    pub irq_systick: u32,
    pub irq_tick: u32,
    pub irq_usart1: u32,
    pub pvd_count: u32,
}

impl Stats {
    pub fn decode(p: &[u8]) -> Option<Stats> {
        if p.len() != 24 {
            return None;
        }
        let u = |i: usize| u32::from_le_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]);
        Some(Stats {
            msp_free: u16::from_le_bytes([p[0], p[1]]),
            msp_size: u16::from_le_bytes([p[2], p[3]]),
            ctx_switches: u(4),
            irq_systick: u(8),
            irq_tick: u(12),
            irq_usart1: u(16),
            pvd_count: u(20),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut v = self.msp_free.to_le_bytes().to_vec();
        v.extend(self.msp_size.to_le_bytes());
        for x in [self.ctx_switches, self.irq_systick, self.irq_tick, self.irq_usart1, self.pvd_count] {
            v.extend(x.to_le_bytes());
        }
        v
    }

    /// (name, what, total) rows for the table, in a fixed order.
    pub fn rows(&self) -> [(&'static str, &'static str, u32); 5] {
        [
            ("SysTick", "тик FreeRTOS, должен быть 1000 в секунду", self.irq_systick),
            ("TIM1", "тик HAL, должен быть 1000 в секунду", self.irq_tick),
            ("USART1", "принятые байты команд и ошибки линии", self.irq_usart1),
            ("PVD", "просадки питания ниже 2.9 В", self.pvd_count),
            ("переключения задач", "сколько раз планировщик сменил задачу", self.ctx_switches),
        ]
    }
}

/// One task of the TASKS reply.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskInfo {
    /// First 8 characters of the FreeRTOS task name.
    pub name: String,
    /// eTaskState: 0 running, 1 ready, 2 blocked, 3 suspended, 4 deleted.
    pub state: u8,
    pub prio: u8,
    /// Least stack ever free, bytes.
    pub stack_free: u16,
    /// CPU time of the task, microseconds (wraps).
    pub run_us: u32,
}

/// TASKS reply: total run time, then up to 5 tasks.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Tasks {
    pub run_us: u32,
    pub tasks: Vec<TaskInfo>,
}

impl Tasks {
    pub fn decode(p: &[u8]) -> Option<Tasks> {
        let n = *p.get(4)? as usize;
        if p.len() != 5 + 16 * n {
            return None;
        }
        let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let tasks = p[5..]
            .chunks_exact(16)
            .map(|r| TaskInfo {
                name: String::from_utf8_lossy(r[..8].split(|b| *b == 0).next().unwrap_or(&[])).into_owned(),
                state: r[8],
                prio: r[9],
                stack_free: u16::from_le_bytes([r[10], r[11]]),
                run_us: u32le(&r[12..16]),
            })
            .collect();
        Some(Tasks { run_us: u32le(&p[..4]), tasks })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut v = self.run_us.to_le_bytes().to_vec();
        v.push(self.tasks.len() as u8);
        for t in &self.tasks {
            let mut name = [0u8; 8];
            let b = t.name.as_bytes();
            name[..b.len().min(8)].copy_from_slice(&b[..b.len().min(8)]);
            v.extend(name);
            v.extend([t.state, t.prio]);
            v.extend(t.stack_free.to_le_bytes());
            v.extend(t.run_us.to_le_bytes());
        }
        v
    }

    /// Share of CPU time of each task between `old` and `self`, 0..1, by name.
    pub fn cpu_since(&self, old: &Tasks) -> Vec<(String, f32)> {
        let dt = self.run_us.wrapping_sub(old.run_us);
        if dt == 0 {
            return vec![];
        }
        self.tasks
            .iter()
            .filter_map(|t| {
                let o = old.tasks.iter().find(|o| o.name == t.name)?;
                Some((t.name.clone(), (t.run_us.wrapping_sub(o.run_us) as f32 / dt as f32).clamp(0.0, 1.0)))
            })
            .collect()
    }
}

pub fn task_state_ru(s: u8) -> &'static str {
    match s {
        0 => "выполняется",
        1 => "готова",
        2 => "ждёт",
        3 => "приостановлена",
        4 => "удалена",
        _ => "?",
    }
}

/// `status_reply_t` from main.c (packed, little-endian, 56 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Status {
    pub uptime_ms: u32,
    pub reset_cause: u32,
    pub boot_count: u32,
    pub heartbeat: u32,
    pub heap_free: u32,
    pub heap_min: u32,
    pub stack_free_default: u16,
    pub stack_free_led: u16,
    pub stack_free_uart: u16,
    pub rx_frames: u32,
    pub rx_crc_err: u32,
    pub uart_err: u32,
    pub queue_full: u32,
    pub fram_ok: u32,
    pub fram_fail: u32,
    pub led_odr: u8,
    pub led_manual: u8,
}

pub const STATUS_LEN: usize = 56;

impl Status {
    pub fn decode(p: &[u8]) -> Option<Status> {
        if p.len() != STATUS_LEN {
            return None;
        }
        let u32_at = |o: usize| u32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
        let u16_at = |o: usize| u16::from_le_bytes([p[o], p[o + 1]]);
        Some(Status {
            uptime_ms: u32_at(0),
            reset_cause: u32_at(4),
            boot_count: u32_at(8),
            heartbeat: u32_at(12),
            heap_free: u32_at(16),
            heap_min: u32_at(20),
            stack_free_default: u16_at(24),
            stack_free_led: u16_at(26),
            stack_free_uart: u16_at(28),
            rx_frames: u32_at(30),
            rx_crc_err: u32_at(34),
            uart_err: u32_at(38),
            queue_full: u32_at(42),
            fram_ok: u32_at(46),
            fram_fail: u32_at(50),
            led_odr: p[54],
            led_manual: p[55],
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(STATUS_LEN);
        for x in [self.uptime_ms, self.reset_cause, self.boot_count, self.heartbeat, self.heap_free, self.heap_min] {
            v.extend_from_slice(&x.to_le_bytes());
        }
        for x in [self.stack_free_default, self.stack_free_led, self.stack_free_uart] {
            v.extend_from_slice(&x.to_le_bytes());
        }
        for x in [self.rx_frames, self.rx_crc_err, self.uart_err, self.queue_full, self.fram_ok, self.fram_fail] {
            v.extend_from_slice(&x.to_le_bytes());
        }
        v.push(self.led_odr);
        v.push(self.led_manual);
        v
    }
}

/// RCC_CSR reset flags (RM0008 §7.3.10, RM0383 §6.3.21: same bits 26..31) → Russian list.
pub fn reset_cause_ru(csr: u32) -> String {
    let flags = [
        (31, "LPWR — выход из low-power"),
        (30, "WWDG — оконный сторож"),
        (29, "IWDG — независимый сторож"),
        (28, "SFT — программный сброс"),
        (27, "POR/PDR — включение питания"),
        (25, "BOR — просадка питания"),
        (26, "PIN — вывод NRST (кнопка, ST-Link)"),
    ];
    let v: Vec<&str> = flags.iter().filter(|(b, _)| csr & (1 << b) != 0).map(|(_, s)| *s).collect();
    if v.is_empty() {
        "флагов нет".into()
    } else {
        v.join(", ")
    }
}

/// Firmware tasks of the current board with their stack sizes (bytes) from main.c attributes.
pub fn tasks() -> &'static [(&'static str, &'static str, u32, &'static str); 3] {
    &super::boards::cur().tasks
}
/// configTOTAL_HEAP_SIZE of the current board's firmware.
pub fn heap_total() -> u32 {
    super::boards::cur().heap_total
}

/// One parsed line of the USART3 text log.
#[derive(Clone, Debug, PartialEq)]
pub enum LogKind {
    Boot,
    Tick(u32),
    Diag,
    Fault,
    PrevFault,
    Test,
    Fram,
    Other,
}

pub fn classify_log(line: &str) -> LogKind {
    let l = line.trim();
    if l.starts_with("BOOT ") {
        LogKind::Boot
    } else if let Some(v) = l.strip_prefix("Tick:") {
        LogKind::Tick(v.trim().parse().unwrap_or(0))
    } else if l.starts_with("DIAG ") || l.starts_with("HeapB") || l.starts_with("HeapU") || l.starts_with("Stk") {
        LogKind::Diag
    } else if l.starts_with("FAULT") || l.starts_with("!!!") {
        LogKind::Fault
    } else if l.starts_with("PREV_FAULT") {
        LogKind::PrevFault
    } else if l.starts_with("TEST ") {
        LogKind::Test
    } else if l.starts_with("FRAM ") {
        LogKind::Fram
    } else {
        LogKind::Other
    }
}

/// `key=value` pairs of a log line (`DIAG heap_free=123 ...`).
pub fn log_kv(line: &str) -> Vec<(String, String)> {
    line.split_whitespace()
        .filter_map(|t| t.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
        .collect()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ")
}

/// Parse "70 01", "7001", "0x70,0x01" → bytes.
pub fn parse_hex(s: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = s.replace("0x", " ").replace("0X", " ").replace([',', ';', ':'], " ");
    let mut out = vec![];
    for tok in cleaned.split_whitespace() {
        if tok.len() % 2 != 0 {
            return Err(format!("«{}»: нечётное число hex-цифр", tok));
        }
        for i in (0..tok.len()).step_by(2) {
            out.push(u8::from_str_radix(&tok[i..i + 2], 16).map_err(|_| format!("«{}» — не hex", tok))?);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_reference_and_known_frames() {
        assert_eq!(crc16(b"123456789"), 0x4B37);
        // examples from docs/PROTOCOL.md
        assert_eq!(frame(&[0x70, 0x01]), [0x70, 0x01, 0xE5, 0xB0]);
        assert_eq!(frame(&[0x70, 0x00]), [0x70, 0x00, 0x24, 0x70]);
        assert_eq!(frame(&[0x02, 0x00]), [0x02, 0x00, 0x00, 0xD0]);
        assert_eq!(frame(&[0x01, 0x00]), [0x01, 0x00, 0x00, 0x20]);
    }

    #[test]
    fn stats_round_trip() {
        let s = Stats { msp_free: 416, msp_size: 1024, ctx_switches: 7, irq_systick: 1000, irq_tick: 1001, irq_usart1: 3, pvd_count: 1 };
        assert_eq!(Stats::decode(&s.encode()), Some(s));
        assert_eq!(Stats::decode(&[0; 23]), None);
    }

    #[test]
    fn tasks_round_trip_and_cpu_share() {
        let t = |name: &str, run| TaskInfo { name: name.into(), state: 2, prio: 24, stack_free: 420, run_us: run };
        let a = Tasks { run_us: u32::MAX - 500_000, tasks: vec![t("IDLE", 100), t("Uart1Tas", 50)] };
        assert_eq!(Tasks::decode(&a.encode()), Some(a.clone()));
        assert_eq!(Tasks::decode(&a.encode()[..20]), None);
        // the total wrapped between the two samples
        let b = Tasks { run_us: 499_999, tasks: vec![t("IDLE", 900_100), t("Uart1Tas", 100_050)] };
        let cpu = b.cpu_since(&a);
        assert_eq!(cpu[0].0, "IDLE");
        assert!((cpu[0].1 - 0.9).abs() < 0.001 && (cpu[1].1 - 0.1).abs() < 0.001);
    }

    #[test]
    fn reply_decoding() {
        assert_eq!(decode_reply(&[]), Decoded::Silent);
        assert!(matches!(decode_reply(&[1, 0, 0]), Decoded::Garbage(_)));
        assert!(matches!(decode_reply(&[1, 0, 0, 0x21]), Decoded::Garbage(_)));
        let ok = decode_reply(&frame(&[ID_BU, ST_OK, 0xAA]));
        assert_eq!(ok, Decoded::Ok(Reply { id: 1, status: 0, payload: vec![0xAA] }));
    }

    #[test]
    fn status_roundtrip() {
        let s = Status { uptime_ms: 1234, heap_free: 2000, stack_free_uart: 900, led_manual: 1, ..Default::default() };
        let b = s.encode();
        assert_eq!(b.len(), STATUS_LEN);
        assert_eq!(Status::decode(&b), Some(s));
    }

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("70 01").unwrap(), [0x70, 1]);
        assert_eq!(parse_hex("0x70,0x01").unwrap(), [0x70, 1]);
        assert_eq!(parse_hex("7001ff").unwrap(), [0x70, 1, 0xff]);
        assert!(parse_hex("7").is_err());
        assert!(parse_hex("zz").is_err());
    }

    #[test]
    fn log_lines() {
        assert_eq!(classify_log("Tick:1500\r"), LogKind::Tick(1500));
        assert_eq!(classify_log("BOOT fw=x rst=0x0C000000 boot=1"), LogKind::Boot);
        assert_eq!(classify_log("FAULT code=0x2"), LogKind::Fault);
        let kv = log_kv("DIAG heap_free=123 heap_min=100");
        assert_eq!(kv[0], ("heap_free".to_string(), "123".to_string()));
    }

    #[test]
    fn reset_flags() {
        assert!(reset_cause_ru(1 << 26).contains("NRST"));
        assert!(reset_cause_ru((1 << 27) | (1 << 26)).contains("питания"));
    }
}

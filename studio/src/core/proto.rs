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
pub const CMD_RESET: u8 = 0x0F;
pub const CMD_LED: u8 = 0x70;
pub const CMD_LED_GET: u8 = 0x71;

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
        (Some(CMD_RESET), _) => "RESET — программный сброс".into(),
        (Some(CMD_LED), Some(0)) => "LED выкл (PC13 = 1)".into(),
        (Some(CMD_LED), Some(1)) => "LED вкл (PC13 = 0)".into(),
        (Some(CMD_LED), Some(2)) => "LED авто-мигание".into(),
        (Some(CMD_LED), _) => "LED (аргумент?)".into(),
        (Some(CMD_LED_GET), _) => "LED_GET — состояние PC13".into(),
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

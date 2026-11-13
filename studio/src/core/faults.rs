//! Fault injection for USART1: frames sent in parts with pauses, with a wrong
//! line format (parity, speed, stop/data bits) or a BREAK, and a set of cases
//! that checks how the firmware survives them. The board reads 8N1 at 38400
//! and ends a frame after a 20 ms gap (Uart1Task, UART_TIMEOUT_MS).

use super::proto::*;

/// Line format for the bytes that follow. The board always reads 8N1, 38400.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Normal,
    ParityEven,
    ParityOdd,
    TwoStop,
    SevenBits,
    Baud(u32),
}

impl Line {
    pub const ALL: [Line; 8] = [
        Line::Normal,
        Line::ParityEven,
        Line::ParityOdd,
        Line::TwoStop,
        Line::SevenBits,
        Line::Baud(19200),
        Line::Baud(57600),
        Line::Baud(115200),
    ];

    pub fn label(self) -> String {
        match self {
            Line::Normal => "8N1, 38400 (норма)".into(),
            Line::ParityEven => "8E1 — бит чётности".into(),
            Line::ParityOdd => "8O1 — бит нечётности".into(),
            Line::TwoStop => "8N2 — два стоп-бита".into(),
            Line::SevenBits => "7N1 — 7 бит данных".into(),
            Line::Baud(b) => format!("8N1, {} бод (неверная скорость)", b),
        }
    }

    pub fn short(self) -> String {
        match self {
            Line::Normal => "8N1".into(),
            Line::ParityEven => "8E1".into(),
            Line::ParityOdd => "8O1".into(),
            Line::TwoStop => "8N2".into(),
            Line::SevenBits => "7N1".into(),
            Line::Baud(b) => format!("{} бод", b),
        }
    }
}

/// One step of a transmission plan.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Line(Line),
    Bytes(Vec<u8>),
    /// Silence on the line, ms. 20 ms or more ends the frame on the board.
    Pause(u32),
    /// Line held low, ms (framing error on the board).
    Break(u32),
}

/// All bytes of the plan in order (for the journal).
pub fn tx_bytes(steps: &[Step]) -> Vec<u8> {
    steps.iter().flat_map(|s| if let Step::Bytes(b) = s { b.clone() } else { vec![] }).collect()
}

/// Total of pauses and breaks, ms.
pub fn wait_ms(steps: &[Step]) -> u64 {
    steps.iter().map(|s| match s {
        Step::Pause(ms) | Step::Break(ms) => *ms as u64,
        _ => 0,
    }).sum()
}

/// True when the plan is just "8N1 bytes" — the ordinary path can send it.
pub fn is_plain(steps: &[Step]) -> bool {
    steps.iter().all(|s| matches!(s, Step::Bytes(_) | Step::Line(Line::Normal)))
}

/// What to do with the CRC of a hand-typed frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrcMode {
    Add,
    AsIs,
    Corrupt,
}

pub fn build(body: &[u8], crc: CrcMode) -> Vec<u8> {
    match crc {
        CrcMode::AsIs => body.to_vec(),
        CrcMode::Add => frame(body),
        CrcMode::Corrupt => {
            let mut f = frame(body);
            let n = f.len();
            f[n - 2] ^= 0x55;
            f
        }
    }
}

/// Hand-made transmission: line format, gap between bytes, repeats with a
/// 30 ms gap between frames.
pub fn manual_plan(bytes: &[u8], line: Line, gap_ms: u32, repeat: u32) -> Vec<Step> {
    let mut v = vec![];
    if line != Line::Normal {
        v.push(Step::Line(line));
    }
    for r in 0..repeat.max(1) {
        if r > 0 {
            v.push(Step::Pause(30));
        }
        if gap_ms == 0 {
            v.push(Step::Bytes(bytes.to_vec()));
        } else {
            for (i, b) in bytes.iter().enumerate() {
                if i > 0 {
                    v.push(Step::Pause(gap_ms));
                }
                v.push(Step::Bytes(vec![*b]));
            }
        }
    }
    v
}

/// Replies glued together on the wire → separate replies plus the count of
/// bytes that did not form one.
pub fn split_replies(rx: &[u8]) -> (Vec<Reply>, usize) {
    let mut out = vec![];
    let mut junk = 0;
    let mut i = 0;
    while i < rx.len() {
        let end = (i + 4..=rx.len()).find(|&j| rx[i] == 0x01 && crc16(&rx[i..j]) == 0);
        match end {
            Some(j) => {
                out.push(Reply { id: rx[i], status: rx[i + 1], payload: rx[i + 2..j - 2].to_vec() });
                i = j;
            }
            None => {
                junk += 1;
                i += 1;
            }
        }
    }
    (out, junk)
}

/// Short text of what came back: "BAD_CRC", "OK ×10", "нет ответа", "+3 Б мусора".
pub fn summary(rx: &[u8]) -> String {
    if rx.is_empty() {
        return "нет ответа".into();
    }
    let (r, junk) = split_replies(rx);
    let mut parts: Vec<String> = vec![];
    let mut i = 0;
    while i < r.len() {
        let st = r[i].status;
        let n = r[i..].iter().take_while(|x| x.status == st).count();
        parts.push(if n > 1 { format!("{} ×{}", status_name(st), n) } else { status_name(st).to_string() });
        i += n;
    }
    if junk > 0 {
        parts.push(format!("{} Б мусора", junk));
    }
    parts.join(", ")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    /// Exactly one reply with this status.
    One(u8),
    /// `n` replies, all with this status.
    Many(u8, usize),
    /// The line itself is broken: any reaction is fine as long as the board
    /// keeps working (STATUS after the case answers and boot_count is the same).
    Survive,
}

impl Expect {
    pub fn label(self) -> String {
        match self {
            Expect::One(st) => status_name(st).to_string(),
            Expect::Many(st, n) => format!("{} ×{}", status_name(st), n),
            Expect::Survive => "любой, плата жива".into(),
        }
    }

    pub fn met(self, rx: &[u8]) -> bool {
        let (r, junk) = split_replies(rx);
        match self {
            Expect::One(st) => junk == 0 && r.len() == 1 && r[0].status == st,
            Expect::Many(st, n) => junk == 0 && r.len() == n && r.iter().all(|x| x.status == st),
            Expect::Survive => true,
        }
    }
}

pub struct Case {
    pub title: &'static str,
    pub what: &'static str,
    pub steps: Vec<Step>,
    pub expect: Expect,
}

const UNKNOWN: u8 = 0x55;

pub fn cases() -> Vec<Case> {
    let ping = frame(&[CMD_PING]);
    let status = frame(&[CMD_STATUS]);
    let bad_crc = build(&[CMD_PING], CrcMode::Corrupt);
    let mut max = vec![UNKNOWN];
    max.resize(UART_BUF_SIZE - 2, 0);
    let mut over = vec![UNKNOWN];
    over.resize(UART_BUF_SIZE - 1, 0);
    // fixed pseudo-random noise (xorshift), same every run
    let mut x: u32 = 0x1234_5678;
    let noise: Vec<u8> = (0..40)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect();
    let mut burst = vec![];
    for i in 0..10 {
        if i > 0 {
            burst.push(Step::Pause(30));
        }
        burst.push(Step::Bytes(ping.clone()));
    }
    let mut slow = vec![];
    for (i, b) in status.iter().enumerate() {
        if i > 0 {
            slow.push(Step::Pause(10));
        }
        slow.push(Step::Bytes(vec![*b]));
    }
    let on = |l: Line, b: &[u8]| vec![Step::Line(l), Step::Bytes(b.to_vec())];
    vec![
        Case { title: "Неверная CRC", what: "PING с испорченным байтом CRC", steps: vec![Step::Bytes(bad_crc)], expect: Expect::One(ST_BAD_CRC) },
        Case { title: "Кадр без CRC", what: "один байт 0x03 (STATUS без CRC)", steps: vec![Step::Bytes(vec![CMD_STATUS])], expect: Expect::One(ST_BAD_CRC) },
        Case { title: "Неизвестная команда", what: "0x55 с верной CRC", steps: vec![Step::Bytes(frame(&[UNKNOWN]))], expect: Expect::One(ST_UNKNOWN_CMD) },
        Case { title: "Неверный аргумент", what: "LED 9 (допустимо 0, 1, 2)", steps: vec![Step::Bytes(frame(&[CMD_LED, 9]))], expect: Expect::One(ST_BAD_ARG) },
        Case { title: "Нет аргумента", what: "LED без аргумента", steps: vec![Step::Bytes(frame(&[CMD_LED]))], expect: Expect::One(ST_BAD_ARG) },
        Case { title: "Кадр 100 байт", what: "ровно размер буфера: команда 0x55 и нули", steps: vec![Step::Bytes(frame(&max))], expect: Expect::One(ST_UNKNOWN_CMD) },
        Case { title: "Кадр 101 байт", what: "на байт больше буфера", steps: vec![Step::Bytes(frame(&over))], expect: Expect::One(ST_OVERFLOW) },
        Case {
            title: "Разрыв кадра",
            what: "PING: CMD, пауза 50 мс, CRC — плата видит два кадра",
            steps: vec![Step::Bytes(ping[..1].to_vec()), Step::Pause(50), Step::Bytes(ping[1..].to_vec())],
            expect: Expect::Many(ST_BAD_CRC, 2),
        },
        Case { title: "Медленные байты", what: "STATUS по байту с паузой 10 мс (< 20 мс — один кадр)", steps: slow, expect: Expect::One(ST_OK) },
        Case {
            title: "Два кадра слитно",
            what: "PING+PING без паузы — для платы это один кадр с неверной CRC",
            steps: vec![Step::Bytes([ping.clone(), ping.clone()].concat())],
            expect: Expect::One(ST_BAD_CRC),
        },
        Case { title: "Поток 10 PING", what: "кадры через 30 мс, не дожидаясь ответов", steps: burst, expect: Expect::Many(ST_OK, 10) },
        Case { title: "Шум 40 байт", what: "случайные байты", steps: vec![Step::Bytes(noise)], expect: Expect::Survive },
        Case { title: "Ошибка чётности 8E1", what: "PING с битом чётности: на месте стоп-бита у платы", steps: on(Line::ParityEven, &ping), expect: Expect::Survive },
        Case { title: "Ошибка чётности 8O1", what: "то же с нечётностью", steps: on(Line::ParityOdd, &ping), expect: Expect::Survive },
        Case { title: "Два стоп-бита 8N2", what: "лишний стоп-бит приёмник должен принять", steps: on(Line::TwoStop, &ping), expect: Expect::One(ST_OK) },
        Case { title: "7 бит данных", what: "PING в формате 7N1", steps: on(Line::SevenBits, &ping), expect: Expect::Survive },
        Case { title: "Скорость 19200", what: "PING вдвое медленнее", steps: on(Line::Baud(19200), &ping), expect: Expect::Survive },
        Case { title: "Скорость 57600", what: "PING в 1.5 раза быстрее", steps: on(Line::Baud(57600), &ping), expect: Expect::Survive },
        Case { title: "BREAK 50 мс", what: "линия в нуле дольше байта — ошибка кадра", steps: vec![Step::Break(50)], expect: Expect::Survive },
        Case {
            title: "BREAK внутри кадра",
            what: "CMD, BREAK 5 мс, CRC",
            steps: vec![Step::Bytes(ping[..1].to_vec()), Step::Break(5), Step::Bytes(ping[1..].to_vec())],
            expect: Expect::Survive,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_modes() {
        assert_eq!(build(&[0x70, 0x01], CrcMode::Add), [0x70, 0x01, 0xE5, 0xB0]);
        assert_eq!(build(&[0x70, 0x01], CrcMode::AsIs), [0x70, 0x01]);
        let bad = build(&[0x70, 0x01], CrcMode::Corrupt);
        assert_eq!(bad.len(), 4);
        assert_ne!(crc16(&bad), 0);
    }

    #[test]
    fn glued_replies_are_split() {
        let a = [0x01, ST_BAD_CRC];
        let ra = frame(&a);
        let rb = frame(&[0x01, ST_OK, b'v', b'1']);
        let rx = [ra.clone(), rb.clone(), vec![0xAA]].concat();
        let (r, junk) = split_replies(&rx);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].status, ST_BAD_CRC);
        assert_eq!(r[1].payload, b"v1");
        assert_eq!(junk, 1);
        assert_eq!(summary(&rx), "BAD_CRC, OK, 1 Б мусора");
        assert!(Expect::Many(ST_BAD_CRC, 2).met(&[ra.clone(), ra.clone()].concat()));
        assert!(!Expect::One(ST_BAD_CRC).met(&rx));
    }

    #[test]
    fn manual_plan_gaps_and_repeats() {
        let p = manual_plan(&[1, 2], Line::ParityEven, 5, 2);
        assert_eq!(p[0], Step::Line(Line::ParityEven));
        assert_eq!(tx_bytes(&p), [1, 2, 1, 2]);
        assert_eq!(wait_ms(&p), 5 + 30 + 5);
        assert!(is_plain(&manual_plan(&[1, 2], Line::Normal, 0, 1)));
    }

    #[test]
    fn case_frames_have_the_intended_sizes() {
        let c = cases();
        let size = |t: &str| tx_bytes(&c.iter().find(|x| x.title == t).unwrap().steps).len();
        assert_eq!(size("Кадр 100 байт"), UART_BUF_SIZE);
        assert_eq!(size("Кадр 101 байт"), UART_BUF_SIZE + 1);
    }
}

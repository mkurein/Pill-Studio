//! Virtual board: answers the same USART1 frames as the firmware (main.c
//! `handle_frame`), produces the text log and plausible register values for
//! the SWD views. Lets the studio be explored and tested without hardware.

use super::faults::{Line, Step};
use super::proto::*;
use super::boards::{self, Family};
use super::svd::Peripheral;
use std::time::Instant;

pub struct SimBoard {
    t0: Instant,
    boot_at: Instant,
    pub led_manual: bool,
    /// PC13 output register bit (0 = LED on).
    pub led_odr: bool,
    pub boot_count: u32,
    reset_cause: u32,
    fram: Vec<u8>,
    pub fram_present: bool,
    rx_frames: u32,
    rx_crc_err: u32,
    rx_overflow: u32,
    uart_err: u32,
    fram_ok: u32,
    fram_fail: u32,
    heap_used_peak: u32,
    last_cmd: u32,
    last_tick_s: u64,
    last_blink_s: u64,
    pending_log: Vec<String>,
    act_pin: bool,
}

pub const SIM_VERSION: &str = "sim-1.0 (виртуальная плата)";

impl Default for SimBoard {
    fn default() -> Self {
        Self::new()
    }
}

impl SimBoard {
    pub fn new() -> SimBoard {
        let now = Instant::now();
        let mut s = SimBoard {
            t0: now,
            boot_at: now,
            led_manual: false,
            led_odr: true,
            boot_count: 0,
            reset_cause: (1 << 27) | (1 << 26),
            fram: vec![0u8; 0x8000],
            fram_present: true,
            rx_frames: 0,
            rx_crc_err: 0,
            rx_overflow: 0,
            uart_err: 0,
            fram_ok: 0,
            fram_fail: 0,
            heap_used_peak: 0,
            last_cmd: 0,
            last_tick_s: 0,
            last_blink_s: 0,
            pending_log: vec![],
            act_pin: false,
        };
        s.boot();
        s
    }

    fn uptime_ms(&self) -> u32 {
        self.boot_at.elapsed().as_millis() as u32
    }

    fn boot(&mut self) {
        self.boot_at = Instant::now();
        self.boot_count += 1;
        self.rx_frames = 0;
        self.rx_crc_err = 0;
        self.rx_overflow = 0;
        self.uart_err = 0;
        self.fram_ok = 0;
        self.fram_fail = 0;
        self.led_manual = false;
        self.last_tick_s = 0;
        self.last_blink_s = 0;
        self.pending_log.push(format!(
            "BOOT fw={} rst=0x{:08X} boot={}",
            SIM_VERSION, self.reset_cause, self.boot_count
        ));
        let nor = super::boards::cur().family == Family::F4;
        self.pending_log.push(format!(
            "FRAM probe={} sr={} mem={} jedec={}",
            if self.fram_present { "OK" } else { "FAIL" },
            if self.fram_present { "02/00" } else { "FF/FF" },
            if nor && self.fram_present { "W25Q64" } else { "FRAM" },
            if nor && self.fram_present { "EF4017" } else { "000000" }
        ));
        self.pending_log.push("Default Task running...".into());
        self.pending_log.push("Uart1Task started".into());
    }

    /// Advance time: LED blinking, Tick/DIAG lines. Returns new log lines.
    pub fn poll_log(&mut self) -> Vec<String> {
        let secs = (self.uptime_ms() / 1000) as u64;
        while self.last_blink_s < secs {
            self.last_blink_s += 1;
            if !self.led_manual {
                self.led_odr = !self.led_odr;
            }
        }
        while self.last_tick_s < secs {
            self.last_tick_s += 1;
            let ms = self.last_tick_s as u32 * 1000 + 3;
            self.pending_log.push(format!("Tick:{}", ms));
            if self.last_tick_s.is_multiple_of(5) {
                let st = self.status();
                self.pending_log.push(format!(
                    "DIAG heap_free={} heap_min={} heap_total={} stk_free D={} L={} U={} rx={} crc_err={} uart_err={}",
                    st.heap_free,
                    st.heap_min,
                    heap_total(),
                    st.stack_free_default,
                    st.stack_free_led,
                    st.stack_free_uart,
                    st.rx_frames,
                    st.rx_crc_err,
                    st.uart_err
                ));
            }
        }
        std::mem::take(&mut self.pending_log)
    }

    pub fn status(&self) -> Status {
        let t = self.t0.elapsed().as_secs_f64();
        let wobble = ((t * 0.7).sin() * 24.0) as i32;
        let heap_free = (heap_total() as i32 - 7_840 - wobble.abs()) as u32;
        Status {
            uptime_ms: self.uptime_ms(),
            reset_cause: self.reset_cause,
            boot_count: self.boot_count,
            heartbeat: self.uptime_ms() / 1000,
            heap_free,
            heap_min: heap_free.min(heap_total() - 7_890 - self.heap_used_peak),
            stack_free_default: 612,
            stack_free_led: 388,
            stack_free_uart: (1_404 - (self.rx_frames.min(40) * 2)) as u16,
            rx_frames: self.rx_frames,
            rx_crc_err: self.rx_crc_err,
            uart_err: self.uart_err,
            queue_full: 0,
            fram_ok: self.fram_ok,
            fram_fail: self.fram_fail,
            led_odr: self.led_odr as u8,
            led_manual: self.led_manual as u8,
        }
    }

    fn reply(status: u8, payload: &[u8]) -> Vec<u8> {
        let mut b = vec![ID_BU, status];
        b.extend_from_slice(payload);
        frame(&b)
    }

    /// One request frame in, reply bytes out (same rules as Uart1Task).
    /// A transmission plan with pauses and line faults, received the way
    /// USART1 + Uart1Task do it: 8N1 at 38400, a gap of 20 ms or more ends
    /// the frame, framing/parity errors count in `uart_err`.
    pub fn xfer_plan(&mut self, steps: &[Step]) -> Vec<u8> {
        let mut buf: Vec<u8> = vec![];
        let mut out = vec![];
        let mut line = Line::Normal;
        for st in steps {
            match st {
                Step::Line(l) => line = *l,
                Step::Bytes(b) => match line {
                    Line::Normal | Line::TwoStop => buf.extend(b),
                    // the parity bit lands on the stop bit: 0 there = framing
                    // error, the byte itself still arrives
                    Line::ParityEven | Line::ParityOdd => {
                        let odd = line == Line::ParityOdd;
                        for &x in b {
                            if (x.count_ones() % 2 == 1) == odd {
                                self.uart_err += 1;
                            }
                            buf.push(x);
                        }
                    }
                    Line::SevenBits => buf.extend(b.iter().map(|x| x | 0x80)),
                    Line::Baud(baud) => {
                        // wrong speed: other bytes and framing errors
                        let n = (b.len() as u64 * baud as u64 / PROTO_BAUD as u64).max(1) as usize;
                        self.uart_err += (b.len() / 2).max(1) as u32;
                        buf.extend(b.iter().cycle().take(n).map(|x| x.rotate_left(3) ^ 0xA5));
                    }
                },
                Step::Pause(ms) => {
                    if *ms >= 20 && !buf.is_empty() {
                        out.extend(self.xfer(&buf));
                        buf.clear();
                    }
                }
                Step::Break(_) => {
                    self.uart_err += 1;
                    buf.push(0);
                }
            }
        }
        if !buf.is_empty() {
            out.extend(self.xfer(&buf));
        }
        out
    }

    pub fn xfer(&mut self, req: &[u8]) -> Vec<u8> {
        self.act_pin = !self.act_pin;
        if req.len() > UART_BUF_SIZE {
            self.rx_overflow += 1;
            return Self::reply(ST_OVERFLOW, &[]);
        }
        if req.len() < 3 || crc16(req) != 0 {
            self.rx_crc_err += 1;
            return Self::reply(ST_BAD_CRC, &[]);
        }
        self.rx_frames += 1;
        let body = &req[..req.len() - 2];
        let nargs = body.len() - 1;
        self.last_cmd = body[0] as u32;
        match body[0] {
            CMD_PING => Self::reply(ST_OK, SIM_VERSION.as_bytes()),
            CMD_LED => {
                if nargs < 1 || body[1] > 2 {
                    return Self::reply(ST_BAD_ARG, &[]);
                }
                if body[1] == 2 {
                    self.led_manual = false;
                } else {
                    self.led_manual = true;
                    self.led_odr = body[1] == 0;
                }
                Self::reply(ST_OK, &[])
            }
            CMD_LED_GET => Self::reply(ST_OK, &[self.led_odr as u8, self.led_manual as u8]),
            CMD_STATUS => {
                self.heap_used_peak = self.heap_used_peak.max(16);
                Self::reply(ST_OK, &self.status().encode())
            }
            CMD_TASKS => {
                // mostly idle; the UART task's share grows with the frames it handled
                let up = self.uptime_ms().wrapping_mul(1000);
                let uart = self.rx_frames.wrapping_mul(180);
                let t = |name: &str, state, prio, free, run| TaskInfo { name: name.into(), state, prio, stack_free: free, run_us: run };
                let tasks = Tasks {
                    run_us: up,
                    tasks: vec![
                        t("Uart1Tas", 0, 24, (1_404 - self.rx_frames.min(40) * 2) as u16, uart),
                        t("IDLE", 1, 0, 300, up.wrapping_sub(uart).wrapping_sub(up / 200)),
                        t("defaultT", 2, 24, 612, up / 400),
                        t("LedTask", 2, 8, 388, up / 400),
                        t("Tmr Svc", 2, 2, 800, 0),
                    ],
                };
                Self::reply(ST_OK, &tasks.encode())
            }
            CMD_FAULT => match body.get(1).copied() {
                Some(0 | 4) => Self::reply(ST_OK, &[]),
                Some(what @ 1..=3) => {
                    // the watchdog brings the board back; here at once
                    self.reset_cause = 1 << 29 | 1 << 26;
                    self.pending_log.push(format!("TEST fault={what}"));
                    self.boot();
                    Self::reply(ST_OK, &[])
                }
                _ => Self::reply(ST_BAD_ARG, &[]),
            },
            CMD_RESET => {
                self.reset_cause = 1 << 28;
                self.pending_log.push("RESET by command".into());
                self.boot();
                Self::reply(ST_OK, &[])
            }
            CMD_FRAM => {
                let sub = if nargs > 0 { body[1] } else { 0 };
                let (st, payload) = match sub {
                    0 => {
                        let w = b"KUREIN\0";
                        if self.fram_present {
                            self.fram[0x150..0x157].copy_from_slice(w);
                            (ST_OK, self.fram[0x150..0x157].to_vec())
                        } else {
                            (ST_FAIL, vec![0xFF; 7])
                        }
                    }
                    1 => {
                        if self.fram_present {
                            (ST_OK, vec![0x02, 0x00])
                        } else {
                            (ST_FAIL, vec![0xFF, 0xFF])
                        }
                    }
                    2 => {
                        if self.fram_present {
                            (ST_OK, vec![0, 0xFF, 0, 0])
                        } else {
                            (ST_FAIL, vec![64, 0, 0x2A, 0xFF])
                        }
                    }
                    _ => return Self::reply(ST_BAD_ARG, &[]),
                };
                if st == ST_OK {
                    self.fram_ok += 1;
                } else {
                    self.fram_fail += 1;
                }
                self.pending_log.push(format!(
                    "TEST fram sub={} result={}",
                    sub,
                    if st == ST_OK { "OK" } else { "FAIL" }
                ));
                Self::reply(st, &payload)
            }
            _ => Self::reply(ST_UNKNOWN_CMD, &[]),
        }
    }

    /// Simulated memory read for the SWD views: GPIO reflects the LED, a few
    /// registers carry the project's configuration, the rest = reset values.
    pub fn read_word(&self, periphs: &[Peripheral], addr: u32) -> u32 {
        let pc13 = (self.led_odr as u32) << 13;
        let pc15 = (self.act_pin as u32) << 15;
        let reset = || {
            periphs
                .iter()
                .find_map(|p| p.registers.iter().find(|r| p.base + r.offset == addr).map(|r| r.reset))
                .unwrap_or(0)
        };
        if boards::cur().family == Family::F4 {
            return match addr {
                0x4002_3800 => 0x0303_7583, // RCC_CR: HSE, PLL on and ready, HSI on
                0x4002_3804 => 0x2440_3019, // RCC_PLLCFGR: M 25, N 192, P /2, Q 4, src HSE
                0x4002_3808 => 0x0000_100A, // RCC_CFGR: SYSCLK = PLL, APB1 /2
                0x4002_3830 => 0x0000_0087, // RCC_AHB1ENR: GPIOA/B/C/H
                0x4002_3844 => 0x0000_5031, // RCC_APB2ENR: TIM1, USART1, USART6, SPI1
                0x4002_3874 => self.reset_cause,
                // GPIOA: PA4 out; PA5-7, PA9-12 AF; PA13-15 debug AF (reset)
                0x4002_0000 => 0xAAA8_A900,
                0x4002_0020 => 0x5550_0000, // AFRL: SPI1 AF5 on PA5..7
                0x4002_0024 => 0x0008_8770, // AFRH: PA9/10 AF7, PA11/12 AF8
                0x4002_0400 => 0x0000_0280, // GPIOB: PB3/PB4 debug AF after reset
                0x4002_0800 => 0x4400_0000, // GPIOC_MODER: PC13, PC15 out
                0x4002_0810 => 0x0000_8000 | pc13 | pc15,
                0x4002_0814 => pc13 | pc15,
                0x4001_1000 => 0x0000_00C0, // USART1_SR: TXE, TC
                0x4001_1008 => 0x0000_09C4, // USART1_BRR 38400 @ 96 MHz
                0x4001_100C => 0x0000_202C, // USART1_CR1: UE, RXNEIE, TE, RE
                0x4001_1408 => 0x0000_0341, // USART6_BRR 115200 @ 96 MHz
                0x4001_140C => 0x0000_200C,
                0x4001_3000 => 0x0000_0374, // SPI1_CR1: master, /128, SSM, SPE
                0xE004_2000 => 0x1000_6431, // DBGMCU_IDCODE: F411, rev A
                _ => reset(),
            };
        }
        match addr {
            0x4002_1000 => 0x0303_0083, // RCC_CR: HSE, PLL on and ready
            0x4002_1004 => 0x001D_040A, // RCC_CFGR: PLL×9 from HSE, APB1 /2, SYSCLK = PLL
            0x4002_1018 => 0x0000_583D, // RCC_APB2ENR
            0x4002_101C => 0x1004_0000, // RCC_APB1ENR
            0x4002_1024 => self.reset_cause,
            0x4001_0800 => 0xB4B2_4444, // GPIOA_CRL: PA4 out, PA5/PA7 AF, PA6 in
            0x4001_0804 => 0x4444_44B4, // GPIOA_CRH: PA9 AF, PA10 in
            0x4001_0C04 => 0x4444_4B44, // GPIOB_CRH: PB10 AF, PB11 in
            0x4001_1004 => 0x2424_4444, // GPIOC_CRH: PC13, PC15 out 2 MHz
            0x4001_1008 => 0x0000_8000 | pc13 | pc15,
            0x4001_100C => pc13 | pc15,
            0x4001_3800 => 0x0000_00C0, // USART1_SR: TXE, TC
            0x4001_3808 => 0x0000_0753, // USART1_BRR 38400 @ 72 MHz
            0x4001_380C => 0x0000_202C, // USART1_CR1: UE, RXNEIE, TE, RE
            0x4000_4808 => 0x0000_0138, // USART3_BRR 115200 @ 36 MHz
            0x4000_480C => 0x0000_200C,
            0x4001_3000 => 0x0000_035C, // SPI1_CR1: master, /16, SSM, SPE
            0xE004_2000 => 0x2000_0410, // DBGMCU_IDCODE: medium density, rev X
            _ => reset(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_reply_decodes() {
        let mut b = SimBoard::new();
        let r = b.xfer(&frame(&[CMD_TASKS]));
        let t = Tasks::decode(&r[2..r.len() - 2]).unwrap();
        assert_eq!(t.tasks.len(), 5);
        assert!(t.tasks.iter().any(|x| x.name == "IDLE"));
        assert!(r.len() <= UART_BUF_SIZE);
    }

    #[test]
    fn reliability_cases_on_the_virtual_board() {
        let mut b = SimBoard::new();
        for c in crate::core::faults::cases() {
            let before = b.status();
            let rx = b.xfer_plan(&c.steps);
            assert!(c.expect.met(&rx), "{}: {}", c.title, crate::core::faults::summary(&rx));
            // still alive, same boot
            let r = b.xfer(&frame(&[CMD_STATUS]));
            assert_eq!(r[1], ST_OK, "{}", c.title);
            assert_eq!(b.status().boot_count, before.boot_count);
        }
        assert!(b.status().uart_err > 0);
    }

    fn cmd(s: &mut SimBoard, body: &[u8]) -> Reply {
        match decode_reply(&s.xfer(&frame(body))) {
            Decoded::Ok(r) => r,
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn behaves_like_firmware() {
        let mut s = SimBoard::new();
        assert_eq!(cmd(&mut s, &[CMD_PING]).status, ST_OK);
        assert_eq!(cmd(&mut s, &[CMD_LED, 1]).payload, Vec::<u8>::new());
        assert_eq!(cmd(&mut s, &[CMD_LED_GET]).payload, vec![0, 1]);
        assert_eq!(cmd(&mut s, &[CMD_LED, 9]).status, ST_BAD_ARG);
        assert_eq!(cmd(&mut s, &[0x55]).status, ST_UNKNOWN_CMD);
        let mut bad = frame(&[CMD_PING]);
        bad[1] ^= 0x55;
        assert_eq!(decode_reply(&s.xfer(&bad)), Decoded::Ok(Reply { id: 1, status: ST_BAD_CRC, payload: vec![] }));
        let st = Status::decode(&cmd(&mut s, &[CMD_STATUS]).payload).unwrap();
        assert_eq!(st.rx_crc_err, 1);
        assert_eq!(st.led_manual, 1);
        assert_eq!(cmd(&mut s, &[CMD_FRAM, 0]).payload, b"KUREIN\0".to_vec());
        let boot = st.boot_count;
        cmd(&mut s, &[CMD_RESET]);
        let st2 = Status::decode(&cmd(&mut s, &[CMD_STATUS]).payload).unwrap();
        assert_eq!(st2.boot_count, boot + 1);
        assert!(s.poll_log().iter().any(|l| l.starts_with("BOOT")));
    }
}

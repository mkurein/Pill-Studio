//! What a halted Cortex-M3 looks like: disassembly lines, hardware breakpoints
//! and a short call stack (current PC, Link Register, or the frame stacked by
//! an exception). No DWARF unwinder — F103 frames are regular enough to read
//! from SP.

use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StepKind {
    /// Next source line, entering `bl` / `blx`.
    Into,
    /// Next source line, running calls to completion.
    Over,
    /// Run until the return address in LR (or the stacked PC in a handler).
    Out,
}

/// Thumb `bl` / `blx`, not a conditional branch (`bls`, `ble`, `b`).
/// Opcode halfwords (`f000`) are skipped; `bl` itself is only two letters.
pub fn is_call(text: &str) -> bool {
    let Some(m) = text.split_whitespace().find(|w| {
        let w = w.trim_end_matches(':');
        let opcode = w.len() >= 4 && w.chars().all(|c| c.is_ascii_hexdigit());
        !opcode && !w.starts_with("0x") && w.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    }) else {
        return false;
    };
    let m = m.trim_end_matches(':').to_ascii_lowercase();
    matches!(m.as_str(), "bl" | "blx" | "bl.w" | "blx.w")
}

/// Where «Выйти» should stop. `None` when LR is an exception token and there is no stacked PC.
pub fn step_out_target(lr: u32, frames: &[u32]) -> Option<u32> {
    let a = if lr >> 24 != 0xFF { lr & !1 } else { frames.get(1).copied()? & !1 };
    code_addr(a).then_some(a)
}

#[derive(Clone, Debug, PartialEq)]
pub struct AsmLine {
    pub addr: u32,
    pub text: String,
}

/// OpenOCD `arm disassemble` text. Lines that are not instructions are skipped.
pub fn parse_disasm(text: &str) -> Vec<AsmLine> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        let Some(tok) = t.split_whitespace().next() else { continue };
        let hex = tok.trim_end_matches(':').trim_start_matches("0x").trim_start_matches("0X");
        if hex.len() < 4 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let Ok(addr) = u32::from_str_radix(hex, 16) else { continue };
        if addr < 0x0800_0000 {
            continue;
        }
        let text = t[tok.len()..].trim().to_string();
        // A symbol label (`08003c10 <func>:`) is not an instruction.
        if text.is_empty() || text.starts_with('<') {
            continue;
        }
        out.push(AsmLine { addr: addr & !1, text });
    }
    out
}

/// Addresses from OpenOCD `bp` output.
pub fn parse_breakpoints(text: &str) -> Vec<u32> {
    let mut v = Vec::new();
    for line in text.lines() {
        for tok in line.split_whitespace() {
            let Some(hex) = tok.strip_prefix("0x").or_else(|| tok.strip_prefix("0X")) else { continue };
            let hex = hex.trim_end_matches([',', ';']);
            if let Ok(a) = u32::from_str_radix(hex, 16) {
                if a >= 0x0800_0000 {
                    v.push(a & !1);
                }
            }
        }
    }
    v.sort_unstable();
    v.dedup();
    v
}

fn code_addr(a: u32) -> bool {
    (0x0800_0000..0x0801_0000).contains(&a) || (0x2000_0000..0x2000_5000).contains(&a)
}

/// PC, then the caller. An EXC_RETURN in LR means the real return address is
/// the stacked PC (R0, R1, R2, R3, R12, LR, PC, xPSR at SP).
pub fn cortex_frames(pc: u32, lr: u32, stack: &[u32]) -> Vec<u32> {
    let mut f = vec![pc & !1];
    let mut seen = HashSet::from([pc & !1]);
    let push = |f: &mut Vec<u32>, seen: &mut HashSet<u32>, a: u32| {
        let a = a & !1;
        if code_addr(a) && seen.insert(a) {
            f.push(a);
        }
    };
    if lr >> 24 == 0xFF {
        if stack.len() >= 7 {
            push(&mut f, &mut seen, stack[6]);
            push(&mut f, &mut seen, stack[5]);
        }
    } else {
        push(&mut f, &mut seen, lr);
    }
    f
}

/// Symbol name or `0x........` → Thumb address (bit 0 cleared).
pub fn resolve_addr(text: &str, sym: Option<u32>) -> Option<u32> {
    if let Some(a) = sym {
        return Some(a & !1);
    }
    let t = text.trim();
    let hex = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok().map(|a| a & !1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disasm_and_breakpoints() {
        let lines = parse_disasm(
            "0x08000160  0xb510      push {r4, lr}\n08000162:  4801  ldr r0, [pc, #4]\nnot an insn\n",
        );
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].addr, 0x0800_0160);
        assert!(lines[0].text.contains("push"));
        let objdump = parse_disasm(
            "08003c10 <prvCheckTasksWaitingTermination>:\n 8003c12:\tldr\tr3, [r3, #0]\n 8003c18:\tbl\t8004e64 <vPortEnterCritical>\n",
        );
        assert_eq!(objdump.len(), 2);
        assert_eq!(objdump[0].addr, 0x0800_3c12);
        assert!(is_call(&objdump[1].text));
        assert!(!is_call(&objdump[0].text));
        let bps = parse_breakpoints("#  Type  Enabled  Address  Length\n1  hw  y  0x08001234  2\n");
        assert_eq!(bps, vec![0x0800_1234]);
    }

    #[test]
    fn exception_frame_and_plain_lr() {
        let stack = [0, 0, 0, 0, 0, 0x0800_0201, 0x0800_0301, 0x6100_0000];
        let f = cortex_frames(0x0800_0100, 0xFFFF_FFFD, &stack);
        assert_eq!(f, vec![0x0800_0100, 0x0800_0300, 0x0800_0200]);
        assert_eq!(cortex_frames(0x0800_0100, 0x0800_0401, &[]), vec![0x0800_0100, 0x0800_0400]);
    }

    #[test]
    fn resolve_hex_or_symbol() {
        assert_eq!(resolve_addr("0x08001235", None), Some(0x0800_1234));
        assert_eq!(resolve_addr("nope", None), None);
        assert_eq!(resolve_addr("main", Some(0x0800_0111)), Some(0x0800_0110));
    }

    #[test]
    fn calls_and_step_out() {
        assert!(is_call("f000 f801  bl uart_write"));
        assert!(is_call("blx r3"));
        assert!(!is_call("b.n 0x08000100"));
        assert!(!is_call("bls 0x08000100"));
        assert_eq!(step_out_target(0x0800_0441, &[]), Some(0x0800_0440));
        assert_eq!(step_out_target(0xFFFF_FFFD, &[0x0800_0100, 0x0800_0200]), Some(0x0800_0200));
        assert_eq!(step_out_target(0xFFFF_FFFD, &[0x0800_0100]), None);
    }
}

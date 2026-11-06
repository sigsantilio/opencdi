// OS-9 trap tracer: with CDI_OS9_TRACE=<file> set, logs every TRAP #n call word and its return registers.
use crate::bus::CdiBus;
use m68k::CpuCore;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufWriter, Write};

// OS-9 calls that take a name or path in a0 (F$Link, F$Load, F$Fork, F$Chain, F$TLink, F$DatMod, I$Attach..I$Delete).
const NAME_CALLS: &[u16] = &[0x00, 0x01, 0x03, 0x05, 0x21, 0x25, 0x80, 0x83, 0x84, 0x85, 0x86, 0x87];
// Calls that never come back to the caller's next instruction (F$Exit, F$RTE).
const NO_RETURN: &[u16] = &[0x06, 0x1E];
const MAX_PENDING: usize = 256;

struct Pending {
    seq: u64,
    ret_pc: u32,
    sp: u32,
    sup: bool,
}

pub struct Os9Trace {
    out: BufWriter<File>,
    seq: u64,
    frame: u64,
    pending: Vec<Pending>,
    modules: Vec<(u32, u32, String)>,
}

impl Os9Trace {
    /// Opens the trace file named by `CDI_OS9_TRACE`; `None` when unset or the file cannot be created.
    pub fn from_env() -> Option<Box<Self>> {
        let path = std::env::var_os("CDI_OS9_TRACE")?;
        let file = File::create(&path).map_err(|e| eprintln!("CDI_OS9_TRACE: {e}")).ok()?;
        let mut out = BufWriter::new(file);
        let _ = writeln!(out, "# os9 trace: C=call R=return; regs before/after; str=a0 text for name calls; mem=32 bytes at a0 for status calls");
        Some(Box::new(Self { out, seq: 0, frame: 0, pending: Vec::new(), modules: Vec::new() }))
    }

    pub fn next_frame(&mut self) {
        self.frame += 1;
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Call at a surfaced TRAP before the exception is taken; `cpu.pc` points at the call word.
    pub fn on_trap(&mut self, cpu: &CpuCore, bus: &CdiBus, trap: u8) {
        self.seq += 1;
        let code = bus.peek_word(cpu.pc).unwrap_or(0xFFFF);
        let sup = cpu.s_flag != 0;
        let module = self.module_at(bus, cpu.ppc);
        let mut line = format!(
            "C {} f={} t={} code={:02X} pc={:06X} mod={} sup={} ",
            self.seq, self.frame, trap, code, cpu.ppc, module, sup as u8
        );
        push_regs(&mut line, cpu, true);
        if trap == 0 && NAME_CALLS.contains(&code) {
            let _ = write!(line, " str=\"{}\"", read_text(bus, cpu.a(0), 64));
        }
        if trap == 0 && code == 0x53 && matches!(cpu.d(1) & 0x7FFF, 0 | 2 | 3) {
            let _ = write!(line, " str=\"{}\"", read_text(bus, cpu.a(0), 32));
        }
        if trap == 0 && (code == 0x8D || code == 0x8E) {
            line.push_str(" mem=");
            for i in 0..32 {
                match bus.peek_byte(cpu.a(0).wrapping_add(i)) {
                    Some(b) => { let _ = write!(line, "{b:02X}"); }
                    None => break,
                }
            }
        }
        if trap == 0 && (code == 0x8A || code == 0x8C) {
            let n = cpu.d(1).min(32);
            let _ = write!(line, " buf=\"{}\"", read_text(bus, cpu.a(0), n as usize));
        }
        let _ = writeln!(self.out, "{line}");
        if !(trap == 0 && NO_RETURN.contains(&code)) {
            if self.pending.len() >= MAX_PENDING {
                self.pending.remove(0);
            }
            self.pending.push(Pending { seq: self.seq, ret_pc: cpu.pc.wrapping_add(2), sp: cpu.dar[15], sup });
        }
    }

    /// Per-instruction check while calls are pending; logs registers when a caller resumes after its call word.
    pub fn check_return(&mut self, cpu: &CpuCore) {
        let pc = cpu.pc;
        let Some(i) = self.pending.iter().rposition(|p| p.ret_pc == pc && p.sp == cpu.dar[15] && p.sup == (cpu.s_flag != 0))
        else {
            return;
        };
        let p = self.pending.remove(i);
        let cc = cpu.get_ccr() & 1;
        let mut line = format!("R {} f={} cc={} err={:04X} ", p.seq, self.frame, cc, cpu.d(1) & 0xFFFF);
        push_regs(&mut line, cpu, false);
        let _ = writeln!(self.out, "{line}");
    }

    // Name+offset of the OS-9 module holding `pc`, found by its header (sync word, parity, size).
    fn module_at(&mut self, bus: &CdiBus, pc: u32) -> String {
        if let Some((b, _, n)) = self.modules.iter().find(|(b, e, _)| pc >= *b && pc < *e) {
            return format!("{}+{:X}", n, pc - b);
        }
        let mut a = pc & !1;
        let floor = pc.saturating_sub(0x40000);
        while a >= floor && a > 0 {
            if bus.peek_word(a) == Some(0x4AFC) && header_ok(bus, a) {
                let size = bus.peek_long(a + 4).unwrap_or(0);
                if pc < a.wrapping_add(size) {
                    let name_off = bus.peek_long(a + 0x0C).unwrap_or(0);
                    let name = read_text(bus, a.wrapping_add(name_off), 32);
                    self.modules.retain(|(b, _, _)| *b != a);
                    self.modules.push((a, a + size, name.clone()));
                    return format!("{}+{:X}", name, pc - a);
                }
            }
            a -= 2;
        }
        "?".into()
    }
}

impl Drop for Os9Trace {
    fn drop(&mut self) {
        let _ = self.out.flush();
    }
}

fn header_ok(bus: &CdiBus, a: u32) -> bool {
    let mut x = 0u16;
    for i in 0..24 {
        match bus.peek_word(a + i * 2) {
            Some(w) => x ^= w,
            None => return false,
        }
    }
    x == 0xFFFF
}

fn push_regs(line: &mut String, cpu: &CpuCore, with_a: bool) {
    for r in 0..8 {
        let _ = write!(line, "d{}={:08X} ", r, cpu.d(r));
    }
    let n = if with_a { 8 } else { 3 };
    for r in 0..n {
        let _ = write!(line, "a{}={:08X} ", r, cpu.a(r));
    }
    line.pop();
}

// Printable text at `addr` up to `max` bytes, stopping at NUL or a high-bit terminator (OS-9 name style).
fn read_text(bus: &CdiBus, addr: u32, max: usize) -> String {
    let mut s = String::new();
    for i in 0..max as u32 {
        let Some(b) = bus.peek_byte(addr.wrapping_add(i)) else { break };
        let c = b & 0x7F;
        if c == 0 {
            break;
        }
        if (0x20..0x7F).contains(&c) && c != b'"' && c != b'\\' {
            s.push(c as char);
        } else {
            let _ = write!(s, "\\x{c:02x}");
        }
        if b & 0x80 != 0 {
            break;
        }
    }
    s
}

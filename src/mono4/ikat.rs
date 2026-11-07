// IKAT system controller at 0x310000: registers on the odd byte addresses (low byte lane), four command ports A-D.
use super::Loud;
use std::collections::VecDeque;

pub const PORT_NAMES: [char; 4] = ['A', 'B', 'C', 'D'];
const REG_MODE: u32 = 0x1D;
const REG_PENDING: u32 = 0x19;
const REG_IMR: u32 = 0x1B;
// Status: bit 4 no reply waiting, bit 0 ready for a command byte; bit 1 stays clear.
const STATUS_EMPTY: u8 = 0x11;
const STATUS_REPLY: u8 = 0x01;

#[derive(Default, Clone)]
pub struct Port {
    pub cmd: Vec<u8>,
    pub reply: VecDeque<u8>,
}

/// Drive request from a port D command, applied to the CIAP by the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveEvent {
    Seek(u32),
    Stop,
}

#[derive(Default)]
pub struct Ikat {
    pub mode: u8,
    pub imr: u8,
    pub ports: [Port; 4],
    pub pointer_enabled: bool,
    pub pal: bool, // video standard reported by the identification reply; set by the bus before each write
    pub disc_present: bool,
    pub status_polls: u8, // drive status polls answered since the disc appeared
    pub drive_event: Option<DriveEvent>,
    pub loud: Loud,
    pub trace: bool,
}

/// Pending bit of a port in the pending and enable registers: A 0x02, B 0x08, C 0x20, D 0x80.
#[inline]
pub fn pending_bit(port: usize) -> u8 {
    0x02 << (2 * port)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02X}", x)).collect::<Vec<_>>().join(" ")
}

impl Ikat {
    pub fn reset(&mut self) {
        let (pal, trace, disc_present) = (self.pal, self.trace, self.disc_present);
        let loud = std::mem::take(&mut self.loud);
        *self = Self { pal, trace, disc_present, loud, ..Self::default() };
    }

    /// Disc inserted or removed; the status replies restart their spin-up sequence.
    pub fn set_disc_present(&mut self, present: bool) {
        self.disc_present = present;
        self.status_polls = 0;
    }

    /// Ports holding an unread reply, as pending bits.
    pub fn pending(&self) -> u8 {
        (0..4).filter(|&p| !self.ports[p].reply.is_empty()).fold(0, |a, p| a | pending_bit(p))
    }

    /// The level-2 request: a pending port whose enable bit is set.
    pub fn irq(&self) -> bool {
        self.pending() & self.imr != 0
    }

    // Command length from the port and its first byte; unknown commands are one byte.
    fn command_length(port: usize, first: u8) -> usize {
        match (port, first) {
            (0, 0x80 | 0x81) => 3,
            (1, 0x90) => 16,
            (1, 0x96) => 8,
            (1, 0x9A) => 6,
            (3, b) if b & 0x80 != 0 => 4,
            _ => 1,
        }
    }

    /// Side-effect-free register read for traces.
    pub fn peek(&self, reg: u32) -> u8 {
        match reg {
            0x09 | 0x0B | 0x0D | 0x0F => self.ports[((reg - 0x09) >> 1) as usize].reply.front().copied().unwrap_or(0xFF),
            0x11 | 0x13 | 0x15 | 0x17 => {
                if self.ports[((reg - 0x11) >> 1) as usize].reply.is_empty() { STATUS_EMPTY } else { STATUS_REPLY }
            }
            REG_PENDING => self.pending(),
            REG_IMR => self.imr,
            _ => 0xFF,
        }
    }

    /// Register read; `reg` is the byte offset in the window (odd for real registers).
    pub fn read(&mut self, reg: u32, pc: u32) -> u8 {
        match reg {
            0x09 | 0x0B | 0x0D | 0x0F => {
                let p = ((reg - 0x09) >> 1) as usize;
                let irq = self.irq();
                let v = self.ports[p].reply.pop_front().unwrap_or(0xFF);
                self.trace_irq(irq);
                v
            }
            0x11 | 0x13 | 0x15 | 0x17 | REG_PENDING | REG_IMR => self.peek(reg),
            _ => {
                if self.loud.first(reg) {
                    eprintln!("IKAT unmodelled read reg {:02X} pc~{:06X}", reg, pc);
                }
                0xFF
            }
        }
    }

    pub fn write(&mut self, reg: u32, value: u8, pc: u32) {
        match reg {
            0x01 | 0x03 | 0x05 | 0x07 => {
                let p = ((reg - 0x01) >> 1) as usize;
                let irq = self.irq();
                self.ports[p].cmd.push(value);
                let cmd = &self.ports[p].cmd;
                if cmd.len() >= Self::command_length(p, cmd[0]) {
                    let cmd = std::mem::take(&mut self.ports[p].cmd);
                    if self.trace {
                        eprintln!("IKAT PORT {} <= {} pc~{:06X}", PORT_NAMES[p], hex(&cmd), pc);
                    }
                    self.execute(p, &cmd, pc);
                }
                self.trace_irq(irq);
            }
            REG_IMR => {
                let irq = self.irq();
                if self.trace {
                    eprintln!("IKAT ICTL <= {:02X} pc~{:06X}", value, pc);
                }
                self.imr = value;
                self.trace_irq(irq);
            }
            REG_MODE => self.mode = value,
            _ => {
                if self.loud.first(0x1_0000 | reg) {
                    eprintln!("IKAT unmodelled write reg {:02X} <- {:02X} pc~{:06X}", reg, value, pc);
                }
            }
        }
    }

    fn trace_irq(&self, before: bool) {
        if self.trace && self.irq() != before {
            eprintln!("IKAT IRQ {} (pending {:02X}, enable {:02X})", self.irq() as u8, self.pending(), self.imr);
        }
    }

    // A port holds one reply at a time; a new reply replaces an unread one.
    fn respond(&mut self, port: usize, bytes: &[u8]) {
        if self.trace {
            eprintln!("IKAT PORT {} => {}", PORT_NAMES[port], hex(bytes));
        }
        self.ports[port].reply = bytes.iter().copied().collect();
    }

    fn execute(&mut self, port: usize, cmd: &[u8], pc: u32) {
        match port {
            2 => self.execute_c(cmd[0], pc),
            3 => self.execute_d(cmd, pc),
            _ => {}
        }
    }

    // Port C: identification and set-up, single-byte commands.
    fn execute_c(&mut self, c: u8, pc: u32) {
        match c {
            0xF0 | 0xF1 | 0xF2 | 0xF4 | 0xF8 => self.respond(2, &[0xA5, c, 0x00]),
            0xF3 => self.respond(2, &[0xA5, 0xF3, 0x4B, 0x4D]),
            0xF6 => self.respond(2, &[0xA5, 0xF6, if self.pal { 0x02 } else { 0x01 }, 0xFF]),
            0xF7 => self.respond(2, &[0xA5, 0xF7, 0x00, 0xFF]),
            0x80 => self.pointer_enabled = true,
            _ => {
                if self.loud.first(0x2_0000 | c as u32) {
                    eprintln!("IKAT unmodelled port C command {:02X} pc~{:06X}", c, pc);
                }
            }
        }
    }

    // Port D: drive commands, four bytes, command byte first with bit 7 set.
    fn execute_d(&mut self, cmd: &[u8], pc: u32) {
        match cmd[0] {
            // Status: without a disc all zero; with one, two spin-up replies (0x610, 0x60E), then ready (0x215).
            0xB0 | 0xA1 => {
                let st: [u8; 2] = match (self.disc_present, self.status_polls) {
                    (false, _) => [0x00, 0x00],
                    (true, 0) => [0x06, 0x10],
                    (true, 1) => [0x06, 0x0E],
                    (true, _) => [0x02, 0x15],
                };
                if self.disc_present {
                    self.status_polls = self.status_polls.saturating_add(1);
                }
                self.respond(3, &[0xB0, 0x00, st[0], st[1]]);
            }
            0xB1 => self.respond(3, &[0xB1, 0x00, 0x02, 0x00]),
            0xB2 => self.respond(3, if self.disc_present { &[0xB2, 0x20, 0x00, 0x01] } else { &[0xB2, 0x00, 0x00, 0x00] }),
            // Seek to a BCD time; the next sector delivered is that time minus 150 frames. No reply.
            0xE0 | 0xE1 => {
                let bcd = |b: u8| ((b >> 4) * 10 + (b & 0x0F)) as u32;
                let frames = (bcd(cmd[1]) * 60 + bcd(cmd[2])) * 75 + bcd(cmd[3]);
                self.drive_event = Some(DriveEvent::Seek(frames.saturating_sub(150)));
            }
            0xC2 | 0xC3 => self.drive_event = Some(DriveEvent::Stop),
            0xA2 | 0xA6 | 0xA7 | 0xC0 | 0xC1 | 0xC4 | 0xC5 => {}
            c => {
                if self.loud.first(0x3_0000 | c as u32) {
                    eprintln!("IKAT unmodelled port D command {} pc~{:06X}", hex(cmd), pc);
                }
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identification_reply_and_level2_line() {
        let mut k = Ikat { pal: true, ..Ikat::default() };
        assert_eq!(k.read(0x11, 0), 0x11);
        k.write(0x1B, 0x20, 0);
        k.write(0x05, 0xF6, 0);
        assert!(k.irq());
        assert_eq!((k.read(0x15, 0), k.read(0x19, 0)), (0x01, 0x20));
        let r: Vec<u8> = (0..5).map(|_| k.read(0x0D, 0)).collect();
        assert_eq!(r, [0xA5, 0xF6, 0x02, 0xFF, 0xFF]);
        assert!(!k.irq());
        for b in [0x80, 0x4E] {
            k.write(0x01, b, 0);
        }
        assert_eq!(k.ports[0].cmd, [0x80, 0x4E]);
        k.write(0x01, 0x73, 0);
        assert!(k.ports[0].cmd.is_empty());
    }

    #[test]
    fn drive_status_sequence_and_seek() {
        let mut k = Ikat::default();
        k.set_disc_present(true);
        let mut status = || {
            for b in [0xB0, 0, 0, 0] {
                k.write(0x07, b, 0);
            }
            (0..4).map(|_| k.read(0x0F, 0)).collect::<Vec<u8>>()
        };
        assert_eq!(status(), [0xB0, 0x00, 0x06, 0x10]);
        assert_eq!(status(), [0xB0, 0x00, 0x06, 0x0E]);
        assert_eq!(status(), [0xB0, 0x00, 0x02, 0x15]);
        for b in [0xE1, 0x00, 0x02, 0x13] {
            k.write(0x07, b, 0);
        }
        assert_eq!(k.drive_event, Some(DriveEvent::Seek(13)));
    }
}

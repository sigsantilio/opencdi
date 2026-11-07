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
    pub len: usize, // length of the frame collected in `cmd`, set by its first byte or by arming from another port
}

/// Drive request from a port D command, applied to the CIAP by the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveEvent {
    Seek(u32),
    Stop,
    Toc,
}

pub struct Ikat {
    pub mode: u8,
    pub imr: u8,
    pub ports: [Port; 4],
    pub keyboard_enabled: bool, // port C 80: key events on port C (not modelled yet)
    pub pal: bool, // video standard reported by the identification reply; set by the bus before each write
    pub disc_present: bool,
    pub drive_busy: bool, // drive seeking or reading; set by the bus before each write
    pub drive_event: Option<DriveEvent>,
    pub reset_request: bool,
    pub toc_reply: bool, // a C0 is waiting for its transition-end status reply
    pub pointer: (i32, i32, u8), // host pointer: x 0..767, y 0..559, buttons (1, 2, 4 = third button)
    pub loud: Loud,
    pub trace: bool,
}

impl Default for Ikat {
    fn default() -> Self {
        Self {
            mode: 0,
            imr: 0,
            ports: Default::default(),
            keyboard_enabled: false,
            pal: true,
            disc_present: false,
            drive_busy: false,
            drive_event: None,
            reset_request: false,
            toc_reply: false,
            pointer: (384, 280, 0),
            loud: Loud::default(),
            trace: false,
        }
    }
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
        let (pal, trace, disc_present, pointer) = (self.pal, self.trace, self.disc_present, self.pointer);
        let loud = std::mem::take(&mut self.loud);
        *self = Self { pal, trace, disc_present, pointer, loud, ..Self::default() };
    }

    /// Disc inserted or removed.
    pub fn set_disc_present(&mut self, present: bool) {
        self.disc_present = present;
    }

    /// Drive status reply: 0 without a disc, else 0x210, or 0x20E right after a TOC request.
    pub fn drive_report(&mut self) {
        // After a table of contents request the reply carries the transition-end code 0x0E.
        let st: [u8; 2] = match self.disc_present {
            false => [0x00, 0x00],
            true if std::mem::take(&mut self.toc_reply) => [0x02, 0x0E],
            true => [0x02, 0x10],
        };
        self.respond(3, &[0xB0, 0x00, st[0], st[1]]);
    }

    /// Host pointer position and buttons; reported on port B on every change while pointer reports are enabled (enable bit 0x08).
    pub fn update_pointer(&mut self, x: i32, y: i32, buttons: u8) {
        let p = (x.clamp(0, 767), y.clamp(0, 559), buttons);
        if p != self.pointer {
            self.pointer = p;
            if self.imr & pending_bit(1) != 0 {
                self.send_pointer();
            }
        }
    }

    // Absolute report: x and y scaled to 0..1023, second byte bit 4 always set (never read by the BIOS); the third button presses buttons 1 and 2.
    fn send_pointer(&mut self) {
        let (x, y, b) = self.pointer;
        let (x, y) = ((x * 0x3FF / 0x2FF) as u16, (y * 0x3FF / 0x22F) as u16);
        let third = b & 4 != 0;
        let (b1, b2) = (b & 1 != 0 || third, b & 2 != 0 || third);
        let r = [
            0x40 | if b1 { 0x20 } else { 0 } | if b2 { 0x10 } else { 0 } | ((x >> 6) & 0x0F) as u8,
            0x10 | ((y >> 6) & 0x0F) as u8,
            (x & 0x3F) as u8,
            (y & 0x3F) as u8 | 0x80,
        ];
        self.respond(1, &r);
    }

    /// Ports holding an unread reply, as pending bits.
    pub fn pending(&self) -> u8 {
        (0..4).filter(|&p| !self.ports[p].reply.is_empty()).fold(0, |a, p| a | pending_bit(p))
    }

    /// The level-2 request: a pending port whose enable bit is set.
    pub fn irq(&self) -> bool {
        self.pending() & self.imr != 0
    }

    // Frame length from the port and its first byte; unknown commands are one byte.
    fn command_length(port: usize, first: u8) -> usize {
        match (port, first) {
            (0, 0x80 | 0x81) => 4,
            (1, 0x8D) => 2,
            (3, 0x91..=0x93 | 0xA1 | 0xA2 | 0xA4 | 0xA6 | 0xA7 | 0xB0..=0xB2 | 0xC0..=0xC5 | 0xE0 | 0xE1) => 4,
            _ => 1,
        }
    }

    // Port A 90/96/9A and port C 8D make port B collect a frame for that command; 8C and 8E end it.
    fn arm_b(&mut self, c: u8) {
        let data = match c {
            0x90 => 16,
            0x96 => 8,
            0x9A => 6,
            0x8D => 1,
            _ => 0,
        };
        let b = &mut self.ports[1];
        b.cmd = if data == 0 { Vec::new() } else { vec![c] };
        b.len = 1 + data;
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
                let port = &mut self.ports[p];
                if port.cmd.is_empty() {
                    port.len = Self::command_length(p, value);
                }
                port.cmd.push(value);
                if port.cmd.len() >= port.len {
                    let cmd = std::mem::take(&mut port.cmd);
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
                if value & pending_bit(1) != 0 {
                    self.send_pointer();
                }
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
        match (port, cmd[0]) {
            // Display set-up frames from the boot code; no reply.
            (0, 0x80 | 0x81) => {}
            (0, 0x90 | 0x96 | 0x9A) => self.arm_b(cmd[0]),
            // Front-panel text (9A: three characters as 16-bit segment patterns, low byte first) and the other display frames.
            (1, 0x90 | 0x96 | 0x9A) => {}
            // Port B loopback test: echo the data byte and stay armed.
            (1, 0x8D) => {
                self.respond(1, &[cmd[1]]);
                self.arm_b(0x8D);
            }
            (2, _) => self.execute_c(cmd[0], pc),
            (3, _) => self.execute_d(cmd, pc),
            _ => {
                if self.loud.first(((port as u32 + 4) << 16) | cmd[0] as u32) {
                    eprintln!("IKAT unmodelled port {} command {} pc~{:06X}", PORT_NAMES[port], hex(cmd), pc);
                }
            }
        }
    }

    // Port C: identification and set-up, single-byte commands.
    fn execute_c(&mut self, c: u8, pc: u32) {
        match c {
            // IKAT release 0x72.
            0xF0 => self.respond(2, &[0xA5, 0xF0, 0x72]),
            0xF1 | 0xF2 | 0xF4 => self.respond(2, &[0xA5, c, 0x00]),
            0xF3 => self.respond(2, &[0xA5, 0xF3, 0x4B, 0x4D]),
            0xF6 => self.respond(2, &[0xA5, 0xF6, if self.pal { 0x02 } else { 0x01 }, 0xFF]),
            0xF7 => self.respond(2, &[0xA5, 0xF7, 0x00, 0xFF]),
            // Video encoder query: version 1 gives the same state as no reply without the 100-tick wait.
            0xF9 => self.respond(2, &[0xA5, 0xF9, 0x01]),
            // Known command without a reply.
            0xFA => {}
            0x80 => self.keyboard_enabled = true,
            // Set-up commands without a reply.
            0x84..=0x86 | 0x8F => {}
            0x8C..=0x8E => self.arm_b(c),
            // System reset request: the board resets the player as after power-on, RAM kept.
            0x88 => {
                self.reset_request = true;
                if self.trace || self.loud.first(0x2_0000 | c as u32) {
                    eprintln!("IKAT system reset request (port C 88) pc~{:06X}", pc);
                }
            }
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
            // Status: no reply while the drive seeks or reads; the start-up status is not answered.
            0xA1 => {}
            0xB0 if self.drive_busy => {
                if self.trace || self.loud.first(0x5_0000) {
                    eprintln!("IKAT PORT D status request {} dropped, drive busy pc~{:06X}", hex(cmd), pc);
                }
            }
            0xB0 => self.drive_report(),
            0xB1 => self.respond(3, &[0xB1, 0x00, 0x02, 0x00]),
            0xB2 => self.respond(3, if self.disc_present { &[0xB2, 0x20, 0x00, 0x01] } else { &[0xB2, 0x00, 0x00, 0x00] }),
            // Seek to a BCD time; the next sector delivered is that time minus 150 frames. No reply.
            0xE0 | 0xE1 => {
                let bcd = |b: u8| ((b >> 4) * 10 + (b & 0x0F)) as u32;
                let frames = (bcd(cmd[1]) * 60 + bcd(cmd[2])) * 75 + bcd(cmd[3]);
                self.drive_event = Some(DriveEvent::Seek(frames.saturating_sub(150)));
            }
            0xC2 | 0xC3 => self.drive_event = Some(DriveEvent::Stop),
            // Table of contents read: the next status reply ends the transition, then the CIAP reports the entries.
            0xC0 if self.disc_present => {
                self.toc_reply = true;
                self.drive_event = Some(DriveEvent::Toc);
            }
            // Accepted without reply or effect; logged once so a run shows they arrived.
            0xA2 | 0xA6 | 0xA7 | 0xC0 | 0xC1 | 0xC4 | 0xC5 => {
                if self.loud.first(0x3_0000 | cmd[0] as u32) {
                    eprintln!("IKAT port D command {} accepted, no effect modelled pc~{:06X}", hex(cmd), pc);
                }
            }
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
        k.write(0x05, 0xF0, 0);
        let r: Vec<u8> = (0..3).map(|_| k.read(0x0D, 0)).collect();
        assert_eq!(r, [0xA5, 0xF0, 0x72]);
        k.write(0x05, 0xF9, 0);
        let r: Vec<u8> = (0..3).map(|_| k.read(0x0D, 0)).collect();
        assert_eq!(r, [0xA5, 0xF9, 0x01]);
        for c in [0xF8, 0xFA, 0x84, 0x85, 0x86, 0x88, 0x8F, 0x91] {
            k.write(0x05, c, 0);
            assert_eq!(k.pending(), 0);
        }
        k.write(0x05, 0x80, 0);
        assert!(k.keyboard_enabled);
    }

    #[test]
    fn port_a_frames_and_port_b_arming() {
        let mut k = Ikat::default();
        for b in [0x80, 0x4E, 0x73] {
            k.write(0x01, b, 0);
        }
        assert_eq!(k.ports[0].cmd, [0x80, 0x4E, 0x73]);
        k.write(0x01, 0x00, 0);
        assert!(k.ports[0].cmd.is_empty());
        k.write(0x01, 0x9A, 0);
        assert_eq!(k.ports[1].cmd, [0x9A]);
        for b in [0x0F, 0x50, 0x0B, 0x10, 0x00] {
            k.write(0x03, b, 0);
        }
        assert_eq!(k.ports[1].cmd.len(), 6);
        k.write(0x03, 0x00, 0);
        assert!(k.ports[1].cmd.is_empty() && k.ports[1].reply.is_empty());
        k.write(0x05, 0x8D, 0);
        for b in [0x01, 0x80] {
            k.write(0x03, b, 0);
            assert_eq!(k.read(0x0B, 0), b);
        }
        k.write(0x05, 0x8E, 0);
        assert!(k.ports[1].cmd.is_empty());
        for b in [0x91, 0x00, 0x00] {
            k.write(0x07, b, 0);
        }
        assert_eq!(k.ports[3].cmd.len(), 3);
        k.write(0x07, 0x00, 0);
        assert!(k.ports[3].cmd.is_empty());
    }

    #[test]
    fn pointer_report_on_enable_and_change() {
        let mut k = Ikat { pointer: (384, 280, 0), ..Ikat::default() };
        k.write(0x1B, 0x08, 0);
        let r: Vec<u8> = (0..4).map(|_| k.read(0x0B, 0)).collect();
        assert_eq!(r, [0x48, 0x18, 0x00, 0x80]);
        k.write(0x1B, 0x88, 0);
        assert_eq!(k.pending(), 0x08);
        k.update_pointer(0, 0, 1);
        let r: Vec<u8> = (0..4).map(|_| k.read(0x0B, 0)).collect();
        assert_eq!(r, [0x60, 0x10, 0x00, 0x80]);
        k.update_pointer(767, 559, 4);
        let r: Vec<u8> = (0..4).map(|_| k.read(0x0B, 0)).collect();
        assert_eq!(r, [0x7F, 0x1F, 0x3F, 0xBF]);
        k.write(0x1B, 0x80, 0);
        k.update_pointer(10, 10, 0);
        assert_eq!(k.pending(), 0);
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
        assert_eq!(status(), [0xB0, 0x00, 0x02, 0x10]);
        assert_eq!(status(), [0xB0, 0x00, 0x02, 0x10]);
        for b in [0xE1, 0x00, 0x02, 0x13] {
            k.write(0x07, b, 0);
        }
        assert_eq!(k.drive_event, Some(DriveEvent::Seek(13)));
        k.drive_busy = true;
        for b in [0xB0, 0, 0, 0] {
            k.write(0x07, b, 0);
        }
        assert!(k.ports[3].reply.is_empty());
        k.drive_report();
        assert_eq!(k.ports[3].reply.len(), 4);
    }
}

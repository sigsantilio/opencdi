// CIAP disc and audio interface at 0x300000: RAM and program memory around a block of word registers at 0x2580.
use super::Loud;
use crate::cdrom::{CdRom, CD_SECTOR_RAW_SIZE};

/// Size of the CIAP window: RAM 0x0000-0x257F, registers 0x2580-0x25FF, program memory 0x2600-0x3FFF.
pub const WINDOW_SIZE: usize = 0x4000;
pub const REG_BASE: u32 = 0x2580;
pub const REG_END: u32 = 0x25FF;

pub const REG_IER: u32 = 0x04;
pub const REG_ISR: u32 = 0x06;
pub const REG_BMAN: u32 = 0x14;
pub const REG_CCR: u32 = 0x16;
pub const REG_APCR: u32 = 0x26;
pub const REG_ASTAT: u32 = 0x2A;
pub const REG_ICR: u32 = 0x40;
pub const REG_DMACTL: u32 = 0x42;
pub const REG_ID: u32 = 0x44;
pub const REG_TEST: u32 = 0x46;
pub const CHIP_ID: u16 = 0xCD02;

// Registers that are stored and read back as written.
const STORED: &[u32] = &[0x08, 0x0A, 0x0C, 0x0E, 0x10, 0x12, 0x1A, 0x20, 0x22, 0x24, 0x28, 0x34, 0x7E];

pub const ISR_DATA: u16 = 0x0001;
pub const ISR_AUDIO_EVENT: u16 = 0x0008;
pub const ISR_TRIGGER: u16 = 0x0200;
pub const ISR_EOF: u16 = 0x0400;
/// Buffer manager at power-on: data buffer 0, Q buffer 0 and subcode buffer 0 current, no audio buffer queued.
pub const BMAN_RESET: u16 = 0x0054;

// Data buffers in CIAP RAM and their buffer-manager bits; the set bit names the buffer the chip fills.
const DATA_BUFS: [(u16, usize); 2] = [(0x04, 0x1200), (0x08, 0x1BC2)];
const DATA_LEN: usize = 2340;

pub struct Ciap {
    pub ram: Vec<u8>,
    pub regs: [u16; 64],
    pub running: bool,
    pub q_mode: bool,
    pub lba: u32,
    pub drive_active: bool,
    pub start_delay: u8, // sector periods after a seek before the first sector
    pub drive_stopped: bool, // the drive stopped because delivery was off; the board reports it
    pub cycles: u32,
    pub dma_request: Option<u16>,
    pub seeks: u64,
    pub sectors: u64,
    pub skipped: u64,
    pub dmas: u64,
    pub loud: Loud,
    pub trace: bool,
}

impl Default for Ciap {
    fn default() -> Self {
        Self { ram: vec![0; WINDOW_SIZE], regs: power_on_regs(), running: false, q_mode: false, lba: 0, drive_active: false, start_delay: 0, drive_stopped: false, cycles: 0, dma_request: None, seeks: 0, sectors: 0, skipped: 0, dmas: 0, loud: Loud::default(), trace: false }
    }
}

fn power_on_regs() -> [u16; 64] {
    let mut regs = [0; 64];
    regs[(REG_BMAN >> 1) as usize] = BMAN_RESET;
    regs
}

impl Ciap {
    pub fn reset(&mut self) {
        self.regs = power_on_regs();
        (self.running, self.q_mode, self.lba, self.drive_active) = (false, false, 0, false);
        (self.start_delay, self.drive_stopped) = (0, false);
    }

    /// Drive request from IKAT port D: a seek positions the next sector and starts the drive, a stop halts delivery.
    pub fn drive(&mut self, ev: super::ikat::DriveEvent) {
        match ev {
            super::ikat::DriveEvent::Seek(lba) => {
                (self.lba, self.drive_active, self.start_delay) = (lba, true, 1);
                self.seeks += 1;
            }
            super::ikat::DriveEvent::Stop => self.drive_active = false,
        }
        if self.trace {
            eprintln!("CIAP DRIVE {:?}", ev);
        }
    }

    /// True when a window offset falls on the register block.
    #[inline]
    pub fn is_reg(offset: u32) -> bool {
        (REG_BASE..=REG_END).contains(&offset)
    }

    #[inline]
    fn reg(&self, reg: u32) -> u16 {
        self.regs[(reg >> 1) as usize]
    }

    #[inline]
    fn set(&mut self, reg: u32, v: u16) {
        self.regs[(reg >> 1) as usize] = v;
    }

    /// The interrupt request: an enabled status bit is set.
    pub fn irq(&self) -> bool {
        self.reg(REG_IER) & self.reg(REG_ISR) != 0
    }

    /// Request level: the board wires the CIAP request to level 4; the interrupt control register is only stored.
    pub fn level(&self) -> u8 {
        4
    }

    /// Acknowledge vector: the on-chip vector of the request level.
    pub fn vector(&self) -> u32 {
        0x38 + self.level() as u32
    }

    /// Side-effect-free register read for traces and byte merges.
    pub fn peek_reg(&self, reg: u32) -> u16 {
        let reg = reg & 0x7E;
        match reg {
            REG_ID => CHIP_ID,
            REG_TEST => if self.reg(REG_TEST) == 2 { 0x0C71 } else { 0xFFFF },
            _ => self.reg(reg),
        }
    }

    /// Register read by byte offset from the register block (even, 0x00-0x7E); reading the status clears it.
    pub fn read_reg(&mut self, reg: u32, pc: u32) -> u16 {
        let reg = reg & 0x7E;
        let v = self.peek_reg(reg);
        match reg {
            REG_ISR => self.set(REG_ISR, 0),
            REG_IER | REG_BMAN | REG_CCR | REG_APCR | REG_ASTAT | REG_ICR | REG_DMACTL | REG_ID | REG_TEST => {}
            r if STORED.contains(&r) => {}
            _ => {
                if self.loud.first(reg) {
                    eprintln!("CIAP unmodelled read reg {:02X} -> {:04X} pc~{:06X}", reg, v, pc);
                }
            }
        }
        if self.trace {
            eprintln!("CIAP RD {:02X} = {:04X} pc~{:06X}", reg, v, pc);
        }
        v
    }

    pub fn write_reg(&mut self, reg: u32, value: u16, pc: u32) {
        let reg = reg & 0x7E;
        if self.trace {
            eprintln!("CIAP WR {:02X} = {:04X} pc~{:06X}", reg, value, pc);
        }
        match reg {
            REG_IER | REG_ICR | REG_TEST => self.set(reg, value),
            // A written 1 toggles the bit: swaps a data or Q pair, queues a CPU-filled audio buffer.
            REG_BMAN => self.set(REG_BMAN, self.reg(REG_BMAN) ^ value),
            REG_CCR => {
                self.set(REG_CCR, value);
                self.command(value, pc);
            }
            REG_APCR => {
                self.set(REG_APCR, value);
                if value == 0x20 || value == 0xA0 {
                    self.set(REG_ISR, self.reg(REG_ISR) | ISR_AUDIO_EVENT);
                    self.set(REG_ASTAT, self.reg(REG_ASTAT) | 0x80);
                }
            }
            // Bit 14 starts a one-shot copy through CPU DMA channel 0; the board runs it.
            REG_DMACTL => {
                self.set(REG_DMACTL, value);
                if value & 0x4000 != 0 {
                    self.dma_request = Some(value);
                }
            }
            r if STORED.contains(&r) => self.set(r, value),
            REG_ISR | REG_ID | REG_ASTAT => {
                if self.loud.first(0x100 | reg) {
                    eprintln!("CIAP write to read-only reg {:02X} <- {:04X} ignored pc~{:06X}", reg, value, pc);
                }
            }
            _ => {
                if self.loud.first(0x100 | reg) {
                    eprintln!("CIAP unmodelled write reg {:02X} <- {:04X} ignored pc~{:06X}", reg, value, pc);
                }
            }
        }
    }

    /// Byte read of one register lane (`hi`: the even address); a status byte read clears only that byte.
    pub fn read_reg_byte(&mut self, reg: u32, hi: bool, pc: u32) -> u8 {
        let reg = reg & 0x7E;
        let w = if reg == REG_ISR {
            let w = self.reg(REG_ISR);
            self.set(REG_ISR, w & if hi { 0x00FF } else { 0xFF00 });
            if self.trace {
                eprintln!("CIAP RD8 {:02X}{} = {:04X} pc~{:06X}", reg, if hi { "H" } else { "L" }, w, pc);
            }
            w
        } else {
            self.read_reg(reg, pc)
        };
        if hi { (w >> 8) as u8 } else { w as u8 }
    }

    /// Byte write of one register lane; buffer-manager bits act on that lane only, command registers are logged.
    pub fn write_reg_byte(&mut self, reg: u32, hi: bool, value: u8, pc: u32) {
        let reg = reg & 0x7E;
        let (mask, v) = if hi { (0xFF00u16, (value as u16) << 8) } else { (0x00FF, value as u16) };
        match reg {
            REG_BMAN => self.write_reg(reg, v, pc),
            REG_CCR | REG_APCR | REG_DMACTL => {
                if self.loud.first(0x400 | reg) {
                    eprintln!("CIAP byte write to command reg {:02X}{} <- {:02X} pc~{:06X}", reg, if hi { "H" } else { "L" }, value, pc);
                }
                let cur = self.reg(reg);
                self.write_reg(reg, (cur & !mask) | v, pc);
            }
            _ => {
                let cur = self.peek_reg(reg);
                self.write_reg(reg, (cur & !mask) | v, pc);
            }
        }
    }

    /// A finished DMA: the word address in the control register has advanced past the words moved.
    pub fn dma_done(&mut self, control: u16, words: u32) {
        self.set(REG_DMACTL, (control & 0xE000) | (control.wrapping_add(words as u16) & 0x1FFF));
        self.dmas += 1;
    }

    /// Advances the sector clock: one drive step every `CYCLES_PER_SECTOR`; the first sector comes on the second step after a seek, and a step with delivery off stops the drive.
    pub fn tick(&mut self, cycles: u32, sector_cycles: u32, cdrom: &mut Option<CdRom>) {
        self.cycles += cycles;
        while self.cycles >= sector_cycles {
            self.cycles -= sector_cycles;
            if !self.drive_active {
                continue;
            }
            if self.start_delay > 0 {
                self.start_delay -= 1;
            } else if !self.running {
                (self.drive_active, self.drive_stopped) = (false, true);
                if self.trace {
                    eprintln!("CIAP DRIVE stopped, delivery off");
                }
            } else if let Some(cd) = cdrom.as_mut() {
                self.sector(cd);
            }
        }
    }

    // Current data buffer: buffer 0 when its bit is set, else buffer 1 when its bit is set.
    fn data_buffer(&self) -> Option<usize> {
        let bman = self.reg(REG_BMAN);
        DATA_BUFS.iter().find(|&&(bit, _)| bman & bit != 0).map(|&(_, at)| at)
    }

    // Delivers the sector at the drive position into the current data buffer, then advances it; the buffer manager is left to the CPU.
    fn sector(&mut self, cd: &mut CdRom) {
        let lba = self.lba;
        self.lba += 1;
        let mut raw = [0u8; CD_SECTOR_RAW_SIZE];
        if !cd.read_sector(lba, &mut raw) {
            if self.loud.first(0x2_0000) {
                eprintln!("CIAP sector {} unreadable", lba);
            }
            return;
        }
        let (file, channel, submode) = (raw[16], raw[17], raw[18]);
        let taken = self.data_buffer().map(|at| self.ram[at..at + DATA_LEN].copy_from_slice(&raw[12..12 + DATA_LEN]));
        let mut flags = if taken.is_some() { ISR_DATA } else { 0 };
        if submode & 0x10 != 0 {
            flags |= ISR_TRIGGER;
        }
        if submode & 0x80 != 0 {
            flags |= ISR_EOF;
        }
        self.set(REG_ISR, self.reg(REG_ISR) | flags);
        if taken.is_some() {
            self.sectors += 1;
        } else {
            self.skipped += 1;
        }
        if self.trace {
            let what = if taken.is_some() { "taken" } else { "skipped" };
            eprintln!("CIAP SECTOR {} file {} channel {} submode {:02X} {} bman {:04X}", lba, file, channel, submode, what, self.reg(REG_BMAN));
        }
    }

    // Commands: 0x94, 0xC4, 0x44 and 0x80 start delivery, 0x100 stops it; Q reporting with 0x10, 0x44 and 0x94, not with 0xC4.
    fn command(&mut self, v: u16, pc: u32) {
        match v {
            0x0094 => (self.running, self.q_mode) = (true, true),
            0x00C4 => (self.running, self.q_mode) = (true, false),
            0x0100 => (self.running, self.q_mode) = (false, false),
            0x0044 => (self.running, self.q_mode) = (true, true),
            0x0080 => self.running = true,
            0x0010 => self.q_mode = true,
            // Accepted without a modelled effect; logged once so a run shows they arrived.
            0x0008 | 0x7000 | 0x3000 | 0x0200 | 0x0040 => {
                if self.loud.first(0x1_0000 | v as u32) {
                    eprintln!("CIAP command {:04X} accepted, no effect modelled pc~{:06X}", v, pc);
                }
            }
            _ => {
                if self.loud.first(0x1_0000 | v as u32) {
                    eprintln!("CIAP unmodelled command {:04X} pc~{:06X}", v, pc);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_level_and_clear_on_read() {
        let mut c = Ciap::default();
        c.write_reg(REG_ICR, 0x0000, 0);
        assert_eq!((c.vector(), c.level()), (0x3C, 4));
        c.write_reg(REG_ICR, 0x01E4, 0);
        assert_eq!(c.peek_reg(REG_ICR), 0x01E4);
        c.write_reg(REG_IER, 0x060B, 0);
        c.write_reg(REG_APCR, 0x20, 0);
        assert!(c.irq());
        assert_eq!(c.peek_reg(REG_ISR), ISR_AUDIO_EVENT);
        assert_eq!(c.read_reg(REG_ISR, 0), ISR_AUDIO_EVENT);
        assert!(!c.irq());
        c.regs[(REG_BMAN >> 1) as usize] = 0x0C;
        c.write_reg(REG_BMAN, 0x04, 0);
        assert_eq!(c.peek_reg(REG_BMAN), 0x08);
        assert_eq!(c.read_reg(REG_ID, 0), CHIP_ID);
    }

    #[test]
    fn byte_lanes_and_read_only_registers() {
        let mut c = Ciap::default();
        c.regs[(REG_ISR >> 1) as usize] = 0x0405;
        assert_eq!(c.read_reg_byte(REG_ISR, true, 0), 0x04);
        assert_eq!(c.peek_reg(REG_ISR), 0x0005);
        c.regs[(REG_BMAN >> 1) as usize] = 0x0C0C;
        c.write_reg_byte(REG_BMAN, false, 0x04, 0);
        assert_eq!(c.peek_reg(REG_BMAN) & 0xFF00, 0x0C00);
        c.write_reg(REG_ID, 0x1234, 0);
        c.write_reg(REG_ASTAT, 0x1234, 0);
        c.write_reg(0x30, 0x1234, 0);
        assert_eq!((c.peek_reg(REG_ID), c.peek_reg(REG_ASTAT), c.peek_reg(0x30)), (CHIP_ID, 0, 0));
    }

    #[test]
    fn rearm_commands_restart_delivery() {
        let mut c = Ciap::default();
        for v in [0x0100, 0x0200, 0x7000, 0x0044, 0x0080] {
            c.write_reg(REG_CCR, v, 0);
        }
        assert!(c.running && c.q_mode);
        for v in [0x0100, 0x0200, 0x3000, 0x0010, 0x0080] {
            c.write_reg(REG_CCR, v, 0);
        }
        assert!(c.running && c.q_mode);
    }

    #[test]
    fn drive_starts_late_and_stops_with_delivery_off() {
        let mut c = Ciap::default();
        let period = crate::mcd212::VideoMode::Pal.cycles_per_sector();
        c.drive(super::super::ikat::DriveEvent::Seek(13));
        c.tick(period, period, &mut None);
        assert!(c.drive_active && !c.drive_stopped);
        c.tick(period, period, &mut None);
        assert!(!c.drive_active && c.drive_stopped);
    }

    #[test]
    fn dma_control_address_advances() {
        let mut c = Ciap::default();
        c.write_reg(REG_DMACTL, 0x6906, 0);
        assert_eq!(c.dma_request.take(), Some(0x6906));
        c.dma_done(0x6906, 1024);
        assert_eq!((c.peek_reg(REG_DMACTL), c.dmas), (0x6D06, 1));
    }

    #[test]
    fn buffer_manager_toggles_on_write() {
        let mut c = Ciap::default();
        assert_eq!(c.peek_reg(REG_BMAN), BMAN_RESET);
        assert_eq!(c.data_buffer(), Some(0x1200));
        c.write_reg(REG_BMAN, 0x0C, 0);
        assert_eq!((c.peek_reg(REG_BMAN), c.data_buffer()), (0x0058, Some(0x1BC2)));
        c.write_reg(REG_BMAN, 0x0C, 0);
        assert_eq!(c.data_buffer(), Some(0x1200));
        c.write_reg(REG_BMAN, 0x01, 0);
        assert_eq!(c.peek_reg(REG_BMAN) & 0x03, 0x01);
        c.reset();
        assert_eq!(c.peek_reg(REG_BMAN), BMAN_RESET);
    }
}

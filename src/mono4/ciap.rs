// CIAP disc and audio interface at 0x300000: RAM and program memory around a block of word registers at 0x2580.
use super::Loud;

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

pub const ISR_AUDIO_EVENT: u16 = 0x0008;

pub struct Ciap {
    pub ram: Vec<u8>,
    pub regs: [u16; 64],
    pub running: bool,
    pub q_mode: bool,
    pub loud: Loud,
    pub trace: bool,
}

impl Default for Ciap {
    fn default() -> Self {
        Self { ram: vec![0; WINDOW_SIZE], regs: [0; 64], running: false, q_mode: false, loud: Loud::default(), trace: false }
    }
}

impl Ciap {
    pub fn reset(&mut self) {
        self.regs = [0; 64];
        (self.running, self.q_mode) = (false, false);
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

    /// Request level, the low three bits of the interrupt control register.
    pub fn level(&self) -> u8 {
        (self.reg(REG_ICR) & 7) as u8
    }

    /// Acknowledge vector, the interrupt control register shifted right by three.
    pub fn vector(&self) -> u32 {
        ((self.reg(REG_ICR) >> 3) & 0xFF) as u32
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
            // A written 1 hands the buffer back to the chip.
            REG_BMAN => self.set(REG_BMAN, self.reg(REG_BMAN) & !value),
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
            REG_DMACTL => {
                self.set(REG_DMACTL, value);
                if value & 0x4000 != 0 && self.loud.first(0x200 | reg) {
                    eprintln!("CIAP unmodelled DMA start {:04X} pc~{:06X}", value, pc);
                }
            }
            r if STORED.contains(&r) => self.set(r, value),
            _ => {
                if self.loud.first(0x100 | reg) {
                    eprintln!("CIAP unmodelled write reg {:02X} <- {:04X} pc~{:06X}", reg, value, pc);
                }
                self.set(reg, value);
            }
        }
    }

    // Commands: 0x94 and 0xC4 start delivery, 0x100 stops it; Q reporting with 0x10, 0x44 and 0x94, not with 0xC4.
    fn command(&mut self, v: u16, pc: u32) {
        match v {
            0x0094 => (self.running, self.q_mode) = (true, true),
            0x00C4 => (self.running, self.q_mode) = (true, false),
            0x0100 => (self.running, self.q_mode) = (false, false),
            0x0010 | 0x0044 => self.q_mode = true,
            0x0008 | 0x7000 | 0x3000 | 0x0080 | 0x0200 | 0x0040 => {}
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
        c.write_reg(REG_ICR, 0x01E4, 0);
        assert_eq!((c.vector(), c.level()), (0x3C, 4));
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
}

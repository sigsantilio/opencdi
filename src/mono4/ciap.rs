// CIAP disc and audio interface at 0x300000: RAM and program memory around a block of word registers at 0x2580.
use super::Loud;

/// Size of the CIAP window: RAM 0x0000-0x257F, registers 0x2580-0x25FF, program memory 0x2600-0x3FFF.
pub const WINDOW_SIZE: usize = 0x4000;
pub const REG_BASE: u32 = 0x2580;
pub const REG_END: u32 = 0x25FF;

pub const REG_ID: u32 = 0x44;
pub const CHIP_ID: u16 = 0xCD02;

pub struct Ciap {
    pub ram: Vec<u8>,
    pub regs: [u16; 64],
    pub loud: Loud,
    pub trace: bool,
}

impl Default for Ciap {
    fn default() -> Self {
        Self { ram: vec![0; WINDOW_SIZE], regs: [0; 64], loud: Loud::default(), trace: false }
    }
}

impl Ciap {
    pub fn reset(&mut self) {
        self.regs = [0; 64];
    }

    /// True when a window offset falls on the register block.
    #[inline]
    pub fn is_reg(offset: u32) -> bool {
        (REG_BASE..=REG_END).contains(&offset)
    }

    /// Register read by byte offset from the register block (even, 0x00-0x7E).
    pub fn read_reg(&mut self, reg: u32, pc: u32) -> u16 {
        let reg = reg & 0x7E;
        let v = match reg {
            REG_ID => CHIP_ID,
            _ => {
                if self.loud.first(reg) {
                    eprintln!("CIAP unmodelled read reg {:02X} -> {:04X} pc~{:06X}", reg, self.regs[(reg >> 1) as usize], pc);
                }
                self.regs[(reg >> 1) as usize]
            }
        };
        if self.trace {
            eprintln!("CIAP RD {:02X} = {:04X} pc~{:06X}", reg, v, pc);
        }
        v
    }

    /// Side-effect-free register read for traces and byte merges.
    pub fn peek_reg(&self, reg: u32) -> u16 {
        let reg = reg & 0x7E;
        match reg {
            REG_ID => CHIP_ID,
            _ => self.regs[(reg >> 1) as usize],
        }
    }

    pub fn write_reg(&mut self, reg: u32, value: u16, pc: u32) {
        let reg = reg & 0x7E;
        if self.trace {
            eprintln!("CIAP WR {:02X} = {:04X} pc~{:06X}", reg, value, pc);
        }
        if self.loud.first(0x100 | reg) {
            eprintln!("CIAP unmodelled write reg {:02X} <- {:04X} pc~{:06X}", reg, value, pc);
        }
        self.regs[(reg >> 1) as usize] = value;
    }
}

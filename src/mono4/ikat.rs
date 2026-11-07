// IKAT system controller at 0x310000: registers on the odd byte addresses (low byte lane).
use super::Loud;

#[derive(Default)]
pub struct Ikat {
    pub mode: u8,
    pub loud: Loud,
    pub trace: bool,
}

impl Ikat {
    pub fn reset(&mut self) {
        self.mode = 0;
    }

    /// Register read; `reg` is the byte offset in the window (odd for real registers).
    pub fn read(&mut self, reg: u32, pc: u32) -> u8 {
        if self.loud.first(reg) {
            eprintln!("IKAT unmodelled read reg {:02X} pc~{:06X}", reg, pc);
        }
        0xFF
    }

    pub fn write(&mut self, reg: u32, value: u8, pc: u32) {
        if reg == 0x1D {
            self.mode = value;
            return;
        }
        if self.loud.first(0x1_0000 | reg) {
            eprintln!("IKAT unmodelled write reg {:02X} <- {:02X} pc~{:06X}", reg, value, pc);
        }
    }
}

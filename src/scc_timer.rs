pub const TSR_OV0: u8 = 0x80;
pub const TSR_MA1: u8 = 0x40;
pub const TSR_CAP1: u8 = 0x20;
pub const TSR_OV1: u8 = 0x10;
pub const TSR_MA2: u8 = 0x08;
pub const TSR_CAP2: u8 = 0x04;
pub const TSR_OV2: u8 = 0x02;

pub const TCR_M1_NONE: u8 = 0x00;
pub const TCR_M1_MATCH: u8 = 0x10;
pub const TCR_M1_CAPTURE: u8 = 0x20;
pub const TCR_M1_COUNT: u8 = 0x30;

pub const TCR_M2_NONE: u8 = 0x00;
pub const TCR_M2_MATCH: u8 = 0x01;
pub const TCR_M2_CAPTURE: u8 = 0x02;
pub const TCR_M2_COUNT: u8 = 0x03;

pub struct SccTimer {
    pub tsr: u8,       // 0x80002020: Timer Status Register
    pub tcr: u8,       // 0x80002021: Timer Control Register
    pub reload: u16,   // 0x80002022: Reload Register (RR)
    pub timer0: u16,   // 0x80002024: Timer 0 (T0)
    pub timer1: u16,   // 0x80002026: Timer 1 (T1)
    pub timer2: u16,   // 0x80002028: Timer 2 (T2)
    pub picr1: u8,     // 0x80002045: Peripheral Interrupt Control Register 1
    pub picr2: u8,     // 0x80002047: Peripheral Interrupt Control Register 2
    pub lir: u8,       // 0x80001001: Line Interrupt Register (INT1 / INT2 control & status)
    pub int1_line: bool, // Physical INT1 line level from MCD212
    pub timer_irq: bool,
    pub cycle_accum: u32,
}

impl SccTimer {
    pub fn new() -> Self {
        Self {
            tsr: 0,
            tcr: 0,
            reload: 0,
            timer0: 0,
            timer1: 0,
            timer2: 0,
            picr1: 0,
            picr2: 0,
            lir: 0,
            int1_line: false,
            timer_irq: false,
            cycle_accum: 0,
        }
    }

    pub fn tick(&mut self, cycles: u32) {
        self.cycle_accum += cycles;
        // Timer clock frequency is CLKOUT / 96
        while self.cycle_accum >= 96 {
            self.cycle_accum -= 96;

            // Timer 0: Free-running timebase / reload timer
            self.timer0 = self.timer0.wrapping_add(1);
            if self.timer0 == 0 {
                self.timer0 = self.reload;
                self.tsr |= TSR_OV0;
            }

            // Timer 1:
            let m1 = self.tcr & 0x30;
            if m1 == TCR_M1_MATCH {
                if self.timer0 == self.timer1 {
                    self.tsr |= TSR_MA1;
                }
            } else if m1 == TCR_M1_COUNT {
                self.timer1 = self.timer1.wrapping_add(1);
                if self.timer1 == 0 {
                    self.timer1 = self.reload;
                    self.tsr |= TSR_OV1;
                }
            }

            // Timer 2:
            let m2 = self.tcr & 0x03;
            if m2 == TCR_M2_MATCH {
                if self.timer0 == self.timer2 {
                    self.tsr |= TSR_MA2;
                }
            } else if m2 == TCR_M2_COUNT {
                self.timer2 = self.timer2.wrapping_add(1);
                if self.timer2 == 0 {
                    self.timer2 = self.reload;
                    self.tsr |= TSR_OV2;
                }
            }

            // Assert timer IRQ if any event bit in TSR is set and PICR1 specifies an IPL > 0
            let lvl = self.picr1 & 7;
            if lvl > 0 && (self.tsr & 0xFE) != 0 {
                self.timer_irq = true;
            }
        }
    }

    pub fn read_byte(&self, reg: u32) -> u8 {
        match reg {
            0x1001 => self.lir & 0x77, // pending bits read as 0
            0x2020 => self.tsr,
            0x2021 => self.tcr,
            0x2022 => (self.reload >> 8) as u8,
            0x2023 => self.reload as u8,
            0x2024 => (self.timer0 >> 8) as u8,
            0x2025 => self.timer0 as u8,
            0x2026 => (self.timer1 >> 8) as u8,
            0x2027 => self.timer1 as u8,
            0x2028 => (self.timer2 >> 8) as u8,
            0x2029 => self.timer2 as u8,
            0x2045 => self.picr1,
            0x2047 => self.picr2,
            _ => 0,
        }
    }

    pub fn read_word(&self, reg: u32) -> u16 {
        match reg {
            0x2020 => ((self.tsr as u16) << 8) | (self.tcr as u16),
            0x2022 => self.reload,
            0x2024 => self.timer0,
            0x2026 => self.timer1,
            0x2028 => self.timer2,
            _ => {
                let hi = self.read_byte(reg) as u16;
                let lo = self.read_byte(reg + 1) as u16;
                (hi << 8) | lo
            }
        }
    }

    pub fn write_byte(&mut self, reg: u32, val: u8) {
        match reg {
            0x1001 => {
                // LIR: Writing bit 7 (0x80) acknowledges INT1 (MCD212 display)
                // Writing bit 3 (0x08) acknowledges INT2
                // Writing other values sets interrupt priority levels
                if (val & 0x80) != 0 {
                    self.lir &= 0x7F;
                }
                if (val & 0x08) != 0 {
                    self.lir &= 0xF7;
                }
                self.lir = (self.lir & 0x88) | (val & 0x77);
            }
            0x2020 => {
                // Writing 1 clears the corresponding bit
                self.tsr &= !val;
                if (self.tsr & 0xFE) == 0 {
                    self.timer_irq = false;
                }
            }
            0x2021 => self.tcr = val,
            0x2022 => self.reload = (self.reload & 0x00FF) | ((val as u16) << 8),
            0x2023 => self.reload = (self.reload & 0xFF00) | (val as u16),
            0x2024 => self.timer0 = (self.timer0 & 0x00FF) | ((val as u16) << 8),
            0x2025 => self.timer0 = (self.timer0 & 0xFF00) | (val as u16),
            0x2026 => self.timer1 = (self.timer1 & 0x00FF) | ((val as u16) << 8),
            0x2027 => self.timer1 = (self.timer1 & 0xFF00) | (val as u16),
            0x2028 => self.timer2 = (self.timer2 & 0x00FF) | ((val as u16) << 8),
            0x2029 => self.timer2 = (self.timer2 & 0xFF00) | (val as u16),
            0x2045 => {
                self.picr1 = val & 0x77;
                if (val & 0x08) != 0 || (self.picr1 & 7) == 0 || (self.tsr & 0xFE) == 0 {
                    self.timer_irq = false;
                }
            }
            0x2047 => self.picr2 = val & 0x77,
            _ => {}
        }
    }

    pub fn write_word(&mut self, reg: u32, val: u16) {
        match reg {
            0x2020 => {
                self.tsr &= !((val >> 8) as u8);
                if (self.tsr & 0xFE) == 0 {
                    self.timer_irq = false;
                }
                self.tcr = val as u8;
            }
            0x2022 => self.reload = val,
            0x2024 => self.timer0 = val,
            0x2026 => self.timer1 = val,
            0x2028 => self.timer2 = val,
            _ => {
                self.write_byte(reg, (val >> 8) as u8);
                self.write_byte(reg + 1, val as u8);
            }
        }
    }

    pub fn acknowledge_irq(&mut self, level: u8) -> bool {
        let expected_lvl = self.picr1 & 7;
        if self.timer_irq && (expected_lvl == 0 || level == expected_lvl) {
            self.timer_irq = false;
            true
        } else {
            false
        }
    }

    /// Level of a pending INT1 request; 0 when none is pending or LIR gives level 0.
    pub fn int1_level(&self) -> u8 {
        if (self.lir & 0x80) != 0 { (self.lir >> 4) & 7 } else { 0 }
    }

    /// Latches an INT1 request; at LIR level 0 the input is off and the request is dropped.
    pub fn raise_int1(&mut self) {
        if (self.lir & 0x70) != 0 {
            self.lir |= 0x80;
        }
    }

    pub fn set_int1_pending(&mut self, pending: bool) {
        if pending {
            self.lir |= 0x80;
        } else {
            self.lir &= 0x7F;
        }
    }

    pub fn int1_w(&mut self, state: bool) {
        if self.int1_line != state {
            if state {
                self.raise_int1();
            }
            self.int1_line = state;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lir_level_zero_drops_int1_and_pending_reads_zero() {
        let mut t = SccTimer::new();
        t.int1_w(true);
        t.int1_w(false);
        assert_eq!(t.int1_level(), 0);
        t.write_byte(0x1001, 0xD4);
        assert_eq!((t.int1_level(), t.read_byte(0x1001)), (0, 0x54));
        t.int1_w(true);
        assert_eq!((t.int1_level(), t.read_byte(0x1001)), (5, 0x54));
        t.write_byte(0x1001, 0x00);
        assert_eq!(t.int1_level(), 0);
    }
}

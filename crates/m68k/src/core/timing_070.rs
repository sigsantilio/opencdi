//! SCC68070 instruction timing from the SCC68070 datasheet tables 11-23.

use super::cpu::CpuCore;
use super::exceptions::vector;

/// Table 11 effective address calculation time.
fn ea(mode: u16, reg: u16, long: bool) -> i32 {
    let (word, long_col) = match mode & 7 {
        0 | 1 => (0, 0),
        2 | 3 => (4, 8),
        4 => (7, 11),
        5 => (11, 15),
        6 => (14, 18),
        _ => match reg & 7 {
            0 => (8, 12),
            1 => (12, 16),
            2 => (11, 15),
            3 => (14, 18),
            _ => (4, 8),
        },
    };
    if long { long_col } else { word }
}

/// Table 20 JMP and LEA clocks by addressing mode.
fn jmp_lea(mode: u16, reg: u16) -> i32 {
    match mode & 7 {
        2 => 7,
        5 => 14,
        6 => 17,
        _ => match reg & 7 {
            0 | 2 => 14,
            1 => 18,
            _ => 17,
        },
    }
}

/// Table 20 MOVEM base clocks, memory to register.
fn movem_to_reg_base(mode: u16, reg: u16) -> i32 {
    match mode & 7 {
        2 | 3 => 26,
        5 => 30,
        6 => 33,
        _ => match reg & 7 {
            1 => 34,
            3 => 33,
            _ => 30,
        },
    }
}

/// Table 20 MOVEM base clocks, register to memory.
fn movem_to_mem_base(mode: u16, reg: u16) -> i32 {
    match mode & 7 {
        2 | 4 => 23,
        5 => 27,
        6 => 30,
        _ => match reg & 7 {
            1 => 31,
            _ => 27,
        },
    }
}

/// Table 23 clocks for an exception vector, without any effective address term.
pub(crate) fn exception_clocks(vec: u32) -> i32 {
    match vec {
        vector::BUS_ERROR | vector::ADDRESS_ERROR => 158,
        vector::ZERO_DIVIDE => 64,
        vector::CHK => 70,
        24..=31 => 65,
        32..=47 => 52,
        vector::RESET_SSP | vector::RESET_PC => 43,
        _ => 55,
    }
}

/// Interrupt entry clocks (table 23).
pub(crate) const INTERRUPT_CLOCKS: i32 = 65;

/// Tables 14 logical and arithmetic ops with an effective address and a data register.
fn alu(op: u16, opmode: u16) -> i32 {
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    let long = opmode & 3 == 2;
    if opmode < 4 {
        7 + ea(mode, reg, long)
    } else if mode == 0 {
        7
    } else {
        (if long { 15 } else { 11 }) + ea(mode, reg, long)
    }
}

/// Table 15 immediate group (ORI, ANDI, SUBI, ADDI, EORI, CMPI) and the static bit ops.
fn group_0(op: u16) -> i32 {
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    if op & 0x0138 == 0x0108 {
        let long = op & 0x40 != 0;
        let to_mem = op & 0x80 != 0;
        return match (long, to_mem) {
            (false, true) => 25,
            (true, true) => 39,
            (false, false) => 22,
            (true, false) => 36,
        };
    }
    if op & 0x0100 != 0 {
        let test = (op >> 6) & 3 == 0;
        return if mode == 0 {
            if test { 7 } else { 10 }
        } else {
            (if test { 7 } else { 14 }) + ea(mode, reg, false)
        };
    }
    let sel = (op >> 9) & 7;
    match sel {
        4 => {
            let test = (op >> 6) & 3 == 0;
            if mode == 0 {
                if test { 14 } else { 17 }
            } else {
                (if test { 14 } else { 21 }) + ea(mode, reg, false)
            }
        }
        7 => 18 + ea(mode, reg, false),
        _ => {
            if mode == 7 && reg == 4 && matches!(sel, 0 | 1 | 5) {
                return 14;
            }
            let long = (op >> 6) & 3 == 2;
            if mode == 0 {
                return if long { 18 } else { 14 };
            }
            let base = match (sel, long) {
                (6, false) => 14,
                (6, true) => 18,
                (_, false) => 18,
                (1, true) => 24,
                (_, true) => 26,
            };
            base + ea(mode, reg, long)
        }
    }
}

fn group_4(op: u16) -> i32 {
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    let size = (op >> 6) & 3;
    match op {
        0x4E70 => return 154,
        0x4E71 => return 7,
        0x4E72 => return 17,
        0x4E73 => return 39,
        0x4E74 | 0x4E75 => return 15,
        0x4E76 => return 10,
        0x4E77 => return 22,
        0x4E7A | 0x4E7B => return 7,
        0x4AFC => return 55,
        _ => {}
    }
    if op & 0xFFF0 == 0x4E40 {
        return 52;
    }
    if op & 0xFFF8 == 0x4E50 {
        return 25;
    }
    if op & 0xFFF8 == 0x4E58 {
        return 15;
    }
    if op & 0xFFF0 == 0x4E60 {
        return 7;
    }
    if op & 0xFFC0 == 0x4E80 {
        return jmp_lea(mode, reg) + 11;
    }
    if op & 0xFFC0 == 0x4EC0 || op & 0xF1C0 == 0x41C0 {
        return jmp_lea(mode, reg);
    }
    if op & 0xF1C0 == 0x4180 {
        return 19 + ea(mode, reg, false);
    }
    if op & 0xFFF8 == 0x4848 {
        return 55;
    }
    if op & 0xFFF8 == 0x4840 || op & 0xFFF8 == 0x4880 || op & 0xFFF8 == 0x48C0 {
        return 7;
    }
    if op & 0xFFC0 == 0x4840 {
        return jmp_lea(mode, reg) + 11;
    }
    if op & 0xFB80 == 0x4880 {
        return 0;
    }
    if op & 0xFFC0 == 0x40C0 {
        return if mode == 0 { 7 } else { 11 + ea(mode, reg, false) };
    }
    if op & 0xFFC0 == 0x44C0 || op & 0xFFC0 == 0x46C0 {
        return 10 + ea(mode, reg, false);
    }
    if op & 0xFFC0 == 0x4800 {
        return if mode == 0 { 10 } else { 10 + ea(mode, reg, false) };
    }
    if op & 0xFFC0 == 0x4AC0 {
        return if mode == 0 { 10 } else { 11 + ea(mode, reg, false) };
    }
    let long = size == 2;
    let kind = (op >> 8) & 0xF;
    if size == 3 {
        return 7;
    }
    if kind == 0xA {
        return 7 + ea(mode, reg, long);
    }
    if mode == 0 {
        return 7;
    }
    let base = if long { 15 } else { 11 };
    let adjust = match (kind, long) {
        (2, false) => -4,
        (2, true) => -8,
        _ => 0,
    };
    base + ea(mode, reg, long) + adjust
}

fn group_5(cpu: &CpuCore, op: u16) -> i32 {
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    let size = (op >> 6) & 3;
    if size == 3 {
        if mode == 1 {
            return if cpu.test_condition(((op >> 8) & 0xF) as u8) {
                14
            } else {
                17
            };
        }
        return if mode == 0 { 13 } else { 17 + ea(mode, reg, false) };
    }
    if mode <= 1 {
        return 7;
    }
    let long = size == 2;
    (if long { 15 } else { 11 }) + ea(mode, reg, long)
}

fn group_6(op: u16) -> i32 {
    let word = op & 0xFF == 0;
    match ((op >> 8) & 0xF, word) {
        (1, false) => 21,
        (1, true) => 25,
        (_, false) => 13,
        (_, true) => 14,
    }
}

fn group_8_c(op: u16, div_mul: [i32; 2]) -> i32 {
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    let opmode = (op >> 6) & 7;
    match opmode {
        3 => div_mul[0] + ea(mode, reg, false),
        7 => div_mul[1] + ea(mode, reg, false),
        _ => alu(op, opmode),
    }
}

fn group_9_d(op: u16) -> i32 {
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    let opmode = (op >> 6) & 7;
    match opmode {
        3 | 7 => 7 + ea(mode, reg, opmode == 7),
        4..=6 if mode <= 1 => {
            if mode == 0 {
                7
            } else if opmode == 6 {
                40
            } else {
                28
            }
        }
        _ => alu(op, opmode),
    }
}

fn group_b(op: u16) -> i32 {
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    let opmode = (op >> 6) & 7;
    match opmode {
        0..=2 => 7 + ea(mode, reg, opmode == 2),
        3 | 7 => 7 + ea(mode, reg, opmode == 7),
        _ if mode == 1 => {
            if opmode == 6 {
                26
            } else {
                18
            }
        }
        _ => alu(op, opmode),
    }
}

fn group_e(cpu: &CpuCore, op: u16) -> i32 {
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    if (op >> 6) & 3 == 3 {
        return 14 + ea(mode, reg, false);
    }
    let count_field = ((op >> 9) & 7) as usize;
    let n = if op & 0x20 != 0 {
        cpu.dar_save[count_field] & 63
    } else if count_field == 0 {
        8
    } else {
        count_field as u32
    };
    13 + 3 * n as i32
}

impl CpuCore {
    /// SCC68070 clocks for the instruction that just retired (datasheet tables 11-23).
    pub(crate) fn cycles_070(&self, raw: i32) -> i32 {
        let op = self.ir as u16;
        if let Some(vec) = self.instruction_exception_vector {
            let mut clocks = exception_clocks(vec);
            if vec == vector::ZERO_DIVIDE {
                clocks += ea((op >> 3) & 7, op & 7, false);
            }
            return clocks;
        }
        let mode = (op >> 3) & 7;
        let reg = op & 7;
        match op >> 12 {
            0x0 => group_0(op),
            0x1 | 0x3 => 7 + ea(mode, reg, false) + ea((op >> 6) & 7, (op >> 9) & 7, false),
            0x2 => 7 + ea(mode, reg, true) + ea((op >> 6) & 7, (op >> 9) & 7, true),
            0x4 => match group_4(op) {
                0 => {
                    let long = op & 0x40 != 0;
                    let per = if long { 11 } else { 7 };
                    let base = if op & 0x0400 != 0 {
                        movem_to_reg_base(mode, reg)
                    } else {
                        movem_to_mem_base(mode, reg)
                    };
                    base + per * self.movem_count as i32
                }
                clocks => clocks,
            },
            0x5 => group_5(self, op),
            0x6 => group_6(op),
            0x7 => 7,
            0x8 => {
                if op & 0x01F0 == 0x0100 {
                    if op & 8 == 0 { 10 } else { 31 }
                } else {
                    group_8_c(op, [130, 169])
                }
            }
            0x9 | 0xD => group_9_d(op),
            0xB => group_b(op),
            0xC => {
                if op & 0x01F0 == 0x0100 {
                    if op & 8 == 0 { 10 } else { 31 }
                } else if matches!(op & 0x01F8, 0x0140 | 0x0148 | 0x0188) {
                    13
                } else {
                    group_8_c(op, [76, 76])
                }
            }
            0xE => group_e(self, op),
            0xA | 0xF => 55,
            _ => raw,
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::memory::{AddressBus, LinearMemoryBus};
    use crate::core::types::{CpuType, StepResult};

    fn machine(cpu_type: CpuType, words: &[u16]) -> (CpuCore, LinearMemoryBus) {
        let mut bus = LinearMemoryBus::new(0x10000);
        bus.write_long(0, 0x8000);
        bus.write_long(4, 0x1000);
        for (i, w) in words.iter().enumerate() {
            bus.write_word(0x1000 + 2 * i as u32, *w);
        }
        let mut cpu = CpuCore::new();
        cpu.set_cpu_type(cpu_type);
        cpu.reset(&mut bus);
        cpu.dar[8] = 0x4000;
        cpu.dar[9] = 0x6000;
        (cpu, bus)
    }

    fn step_cycles(cpu: &mut CpuCore, bus: &mut LinearMemoryBus) -> i32 {
        match cpu.step(bus) {
            StepResult::Ok { cycles } => cycles,
            other => panic!("step failed: {other:?}"),
        }
    }

    fn run_with(words: &[u16], setup: impl FnOnce(&mut CpuCore)) -> i32 {
        let (mut cpu, mut bus) = machine(CpuType::SCC68070, words);
        setup(&mut cpu);
        step_cycles(&mut cpu, &mut bus)
    }

    fn run(words: &[u16]) -> i32 {
        run_with(words, |_| {})
    }

    fn check(rows: &[(&str, &[u16], i32)]) {
        for (label, words, expected) in rows {
            assert_eq!(run(words), *expected, "{label}");
        }
    }

    #[test]
    fn table_12_move_byte_word() {
        check(&[
            ("MOVE.B D0,D1", &[0x1200], 7),
            ("MOVE.W (A0),D0", &[0x3010], 11),
            ("MOVE.W (A0)+,(A1)+", &[0x32D8], 15),
            ("MOVE.W d(A0),(A1)+", &[0x32E8, 4], 22),
            ("MOVE.W d(PC),D1", &[0x323A, 2], 18),
            ("MOVE.W (d8,A0,D0.W),D1", &[0x3230, 0x0000], 21),
            ("MOVE.W D0,(xxx).L", &[0x33C0, 0, 0x4000], 19),
            ("MOVE.W #imm,D0", &[0x303C, 1], 11),
        ]);
    }

    #[test]
    fn table_13_move_long() {
        check(&[
            ("MOVE.L D0,D1", &[0x2200], 7),
            ("MOVE.L (A0)+,(A1)+", &[0x22D8], 23),
            ("MOVE.L -(A0),-(A1)", &[0x2320], 29),
            ("MOVE.L (xxx).L,D0", &[0x2039, 0, 0x4000], 23),
            ("MOVEA.L (A0),A1", &[0x2250], 15),
        ]);
    }

    #[test]
    fn table_14_standard() {
        check(&[
            ("ADD.L (A0),D0", &[0xD090], 15),
            ("ADDA.L D0,A0", &[0xD1C0], 7),
            ("ADD.W D0,(A0)", &[0xD150], 15),
            ("ADD.L D0,(A0)", &[0xD190], 23),
            ("CMP.W (A0),D0", &[0xB050], 11),
            ("CMPA.L A0,A1", &[0xB3C8], 7),
            ("EOR.W D0,(A0)", &[0xB150], 15),
            ("MULU.W D0,D1", &[0xC2C0], 76),
            ("MULS.W D0,D1", &[0xC3C0], 76),
        ]);
        let ones = |cpu: &mut CpuCore| cpu.set_d(0, 1);
        assert_eq!(run_with(&[0x82C0], ones), 130, "DIVU.W D0,D1");
        assert_eq!(run_with(&[0x83C0], ones), 169, "DIVS.W D0,D1");
    }

    #[test]
    fn table_15_immediate() {
        check(&[
            ("ADDI.W #1,D0", &[0x0640, 1], 14),
            ("ADDI.L #1,D0", &[0x0680, 0, 1], 18),
            ("ADDI.W #1,(A0)", &[0x0650, 1], 22),
            ("ANDI.L #1,(A0)", &[0x0290, 0, 1], 32),
            ("CMPI.W #1,(A0)", &[0x0C50, 1], 18),
            ("ADDQ.W #1,D0", &[0x5240], 7),
            ("ADDQ.L #1,A0", &[0x5288], 7),
            ("ADDQ.W #1,(A0)", &[0x5250], 15),
            ("SUBQ.L #1,(A0)", &[0x5390], 23),
            ("MOVEQ #0,D0", &[0x7000], 7),
        ]);
    }

    #[test]
    fn table_16_single_operand() {
        check(&[
            ("CLR.W D0", &[0x4240], 7),
            ("CLR.W (A0)", &[0x4250], 11),
            ("CLR.L (A0)", &[0x4290], 15),
            ("NEG.W (A0)", &[0x4450], 15),
            ("NOT.L D0", &[0x4680], 7),
            ("NBCD D0", &[0x4800], 10),
            ("NBCD (A0)", &[0x4810], 14),
            ("ST D0", &[0x50C0], 13),
            ("ST (A0)", &[0x50D0], 21),
            ("TAS D0", &[0x4AC0], 10),
            ("TAS (A0)", &[0x4AD0], 15),
            ("TST.W (A0)", &[0x4A50], 11),
            ("TST.L D0", &[0x4A80], 7),
        ]);
    }

    #[test]
    fn table_17_shift_rotate() {
        check(&[
            ("LSL.W #1,D0", &[0xE348], 16),
            ("LSL.L #8,D0", &[0xE188], 37),
            ("ASL.W (A0)", &[0xE1D0], 18),
        ]);
        assert_eq!(run_with(&[0xE260], |c| c.set_d(1, 5)), 28, "ASR.W D1,D0");
        assert_eq!(run_with(&[0xE060], |c| c.set_d(0, 3)), 22, "ASR.W D0,D0");
    }

    #[test]
    fn table_18_bit_manipulation() {
        check(&[
            ("BTST D0,D1", &[0x0101], 7),
            ("BCHG D0,D1", &[0x0141], 10),
            ("BSET D0,(A0)", &[0x01D0], 18),
            ("BTST #3,D0", &[0x0800, 3], 14),
            ("BSET #3,D0", &[0x08C0, 3], 17),
            ("BTST #3,(A0)", &[0x0810, 3], 18),
            ("BCLR #3,(A0)", &[0x0890, 3], 25),
        ]);
    }

    #[test]
    fn table_19_conditional() {
        check(&[
            ("BRA.B", &[0x6002], 13),
            ("BRA.W", &[0x6000, 4], 14),
            ("BSR.B", &[0x6102], 21),
            ("BSR.W", &[0x6100, 4], 25),
            ("BNE.B taken", &[0x6602], 13),
            ("BEQ.B not taken", &[0x6702], 13),
            ("BEQ.W not taken", &[0x6700, 4], 14),
            ("DBF expires", &[0x51C8, 0xFFFE], 17),
            ("DBT cc true", &[0x50C8, 0xFFFE], 14),
            ("TRAPV not taken", &[0x4E76], 10),
            ("CHK.W D0,D1 not taken", &[0x4380], 19),
        ]);
        assert_eq!(run_with(&[0x51C8, 0xFFFE], |c| c.set_d(0, 5)), 17, "DBF loops");
        assert_eq!(run_with(&[0x6702, 0], |c| c.set_ccr(4)), 13, "BEQ.B taken");
        assert_eq!(run_with(&[0x4E76], |c| c.set_ccr(2)), 55, "TRAPV taken");
        let chk = |c: &mut CpuCore| {
            c.set_d(0, 10);
            c.set_d(1, 20);
        };
        assert_eq!(run_with(&[0x4380], chk), 70, "CHK.W taken");
    }

    #[test]
    fn table_20_jump_load_movem() {
        check(&[
            ("JMP (A0)", &[0x4ED0], 7),
            ("JSR (A0)", &[0x4E90], 18),
            ("LEA d(A0),A1", &[0x43E8, 4], 14),
            ("PEA (A0)", &[0x4850], 18),
            ("MOVEM.W D0-D2,(A0)", &[0x4890, 0x0007], 44),
            ("MOVEM.L (A0)+,D0-D3", &[0x4CD8, 0x000F], 70),
            ("MOVEM.L D0-D1,-(A7)", &[0x48E7, 0xC000], 45),
            ("MOVEM.W d(A0),D0", &[0x4CA8, 0x0001, 4], 37),
        ]);
    }

    #[test]
    fn table_21_multi_precision() {
        check(&[
            ("ADDX.W D0,D1", &[0xD340], 7),
            ("ADDX.L -(A0),-(A1)", &[0xD388 | 0x0200], 40),
            ("SUBX.W -(A0),-(A1)", &[0x9348], 28),
            ("CMPM.W (A0)+,(A1)+", &[0xB348], 18),
            ("CMPM.L (A0)+,(A1)+", &[0xB388], 26),
            ("ABCD D0,D1", &[0xC300], 10),
            ("SBCD -(A0),-(A1)", &[0x8308], 31),
        ]);
    }

    #[test]
    fn table_22_miscellaneous() {
        check(&[
            ("NOP", &[0x4E71], 7),
            ("EXG D0,D1", &[0xC141], 13),
            ("EXT.W D0", &[0x4880], 7),
            ("SWAP D0", &[0x4840], 7),
            ("LINK A0,#-4", &[0x4E50, 0xFFFC], 25),
            ("UNLK A0", &[0x4E58], 15),
            ("MOVE SR,D0", &[0x40C0], 7),
            ("MOVE SR,(A0)", &[0x40D0], 15),
            ("MOVE D0,CCR", &[0x44C0], 10),
            ("MOVE D0,SR", &[0x46C0], 10),
            ("MOVE A0,USP", &[0x4E60], 7),
            ("ORI #0,CCR", &[0x003C, 0], 14),
            ("ANDI #$FFFF,SR", &[0x027C, 0xFFFF], 14),
            ("MOVEP.W D0,0(A0)", &[0x0188, 0], 25),
            ("MOVEP.L D0,0(A0)", &[0x01C8, 0], 39),
            ("MOVEP.W 0(A0),D0", &[0x0108, 0], 22),
            ("MOVEP.L 0(A0),D0", &[0x0148, 0], 36),
            ("RESET", &[0x4E70], 154),
            ("RTE short format", &[0x4E73], 39),
            ("RTR", &[0x4E77], 22),
            ("RTS", &[0x4E75], 15),
            ("STOP", &[0x4E72, 0x2700], 17),
        ]);
    }

    #[test]
    fn table_23_exceptions() {
        let (mut cpu, mut bus) = machine(CpuType::SCC68070, &[0x4E40]);
        assert!(matches!(cpu.step(&mut bus), StepResult::TrapInstruction { .. }));
        assert_eq!(cpu.take_trap_exception(&mut bus, 0), 52, "TRAP");

        let (mut cpu, mut bus) = machine(CpuType::SCC68070, &[0x4AFC]);
        assert!(matches!(cpu.step(&mut bus), StepResult::IllegalInstruction { .. }));
        assert_eq!(cpu.take_illegal_exception(&mut bus), 55, "illegal");

        assert_eq!(run(&[0x82C0]), 64, "DIVU.W D0,D1 by zero");
        assert_eq!(run(&[0x82D0]), 68, "DIVU.W (A0),D1 by zero");
        assert_eq!(run_with(&[0x46C0], |c| c.set_sr(0)), 55, "privilege violation");
        assert_eq!(exception_clocks(vector::ADDRESS_ERROR), 158);

        let (mut cpu, mut bus) = machine(CpuType::SCC68070, &[0x4E71]);
        cpu.set_sr(0x2000);
        cpu.int_level = 4;
        let result = cpu.run_for_cycles(&mut bus, 1);
        assert_eq!((result.cycles, result.instructions), (65, 0), "interrupt");
    }

    #[test]
    fn other_cpu_types_keep_their_timing() {
        for (cpu_type, op, expected) in [
            (CpuType::M68000, 0x7000u16, 4),
            (CpuType::M68010, 0x4E71, 4),
            (CpuType::M68000, 0x2200, 4),
        ] {
            let (mut cpu, mut bus) = machine(cpu_type, &[op]);
            assert_eq!(step_cycles(&mut cpu, &mut bus), expected, "{cpu_type:?} {op:04X}");
        }
    }

    #[test]
    fn short_loop_charges_the_table_sum_per_iteration() {
        // MOVEQ 7 once, then SUBQ.W 7 + BNE.B 13 per pass
        let (mut cpu, mut bus) = machine(CpuType::SCC68070, &[0x7005, 0x5340, 0x66FC, 0x4E71]);
        let mut total = step_cycles(&mut cpu, &mut bus);
        for _ in 0..5 {
            total += step_cycles(&mut cpu, &mut bus);
            total += step_cycles(&mut cpu, &mut bus);
        }
        assert_eq!(total, 7 + 5 * (7 + 13));
    }

    #[test]
    fn trace_jit_never_records_for_the_scc68070() {
        use crate::core::trace_jit::note_backward_branch;
        let mut cpu = CpuCore::new();
        cpu.set_cpu_type(CpuType::SCC68070);
        cpu.pc = 0x2000;
        assert!(!note_backward_branch(&mut cpu, CpuType::SCC68070));
    }
}

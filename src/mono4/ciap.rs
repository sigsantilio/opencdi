// CIAP disc and audio interface at 0x300000: RAM and program memory around a block of word registers at 0x2580.
use super::Loud;
use crate::audio::CdiAudio;
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
pub const REG_TACS: u32 = 0x08;
pub const REG_ATT_LEFT: u32 = 0x20;
pub const REG_ATT_RIGHT: u32 = 0x22;
/// Left-to-left and right-to-right open, the cross paths muted.
const ATT_STRAIGHT: u16 = 0x003F;
pub const REG_ASHDW: u32 = 0x1A;
pub const REG_APTR: u32 = 0x34;
pub const ISR_AUDIO_DONE: u16 = 0x0002;
const AUDIO_BUFS: [usize; 2] = [0x0000, 0x0900];
const AUDIO_LEN: usize = 0x900;
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

// Coding byte from the audio shadow register: bit 13 8-bit, bit 12 18.9 kHz, bit 11 stereo, bit 3 emphasis.
fn ashdw_coding(v: u16) -> u8 {
    let mut c = 0;
    for (bit, out) in [(13, 0x10), (12, 0x04), (11, 0x01), (3, 0x40)] {
        if v & (1 << bit) != 0 {
            c |= out;
        }
    }
    c
}

// Sync field of a data sector as a byte-swapped image stores it.
const SWAPPED_SYNC: [u8; 12] = [0xFF, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0xFF];

// Header check: BCD time of the sector plus 150, mode 1 or 2, and for mode 2 two equal subheader copies.
fn header_valid(lba: u32, raw: &[u8; CD_SECTOR_RAW_SIZE]) -> bool {
    let bcd = |v: u32| (((v / 10) << 4) | (v % 10)) as u8;
    let f = lba + 150;
    raw[12..15] == [bcd(f / 4500), bcd(f / 75 % 60), bcd(f % 75)]
        && (raw[15] == 1 || (raw[15] == 2 && raw[16..20] == raw[20..24]))
}

// Yellow Book scrambler pattern, generated once.
fn scramble_table() -> &'static [u8; 2448] {
    static TABLE: std::sync::OnceLock<[u8; 2448]> = std::sync::OnceLock::new();
    TABLE.get_or_init(crate::cdic::generate_sector_scramble)
}

pub struct Ciap {
    pub ram: Vec<u8>,
    pub regs: [u16; 64],
    pub running: bool,
    pub q_mode: bool,
    pub lba: u32,
    pub drive_active: bool,
    pub audio_playing: bool, // CPU-fed playback started by APCR 0x140
    pub audio_wait: u8, // sector periods until the next audio-done step
    pub audio_next: usize, // audio buffer the next step looks at first
    pub audio_coding: [u8; 2], // coding captured from ASHDW when the CPU queued each buffer
    pub event_armed: bool, // APCR 0x20 written while a buffer was queued
    pub asel: bool, // CCR 0x0008: chip plays audio sectors of the TACS channels until CCR 0x0100
    pub audio_buffers: u64,
    pub audio_sectors: u64,
    pub audio_overlap: u64, // TACS sectors played while CPU-fed playback ran
    pub toc: bool, // table of contents read: Q reports carry lead-in entries
    pub toc_index: usize,
    pub start_delay: u8, // sector periods after a seek before the first sector
    pub drive_stopped: bool, // the drive stopped because delivery was off; the board reports it (cleared within the same tick)
    pub cycles: u32,
    pub dma_request: Option<u16>,
    pub seeks: u64,
    pub sectors: u64,
    pub skipped: u64,
    pub dmas: u64,
    pub byteswap: bool,
    pub descrambled: u64,
    pub q_reports: u64,
    pub loud: Loud,
    pub trace: bool,
}

impl Default for Ciap {
    fn default() -> Self {
        Self { ram: vec![0; WINDOW_SIZE], regs: power_on_regs(), running: false, q_mode: false, lba: 0, drive_active: false, audio_playing: false, audio_wait: 0, audio_next: 0, audio_coding: [0; 2], event_armed: false, asel: false, audio_buffers: 0, audio_sectors: 0, audio_overlap: 0, toc: false, toc_index: 0, start_delay: 0, drive_stopped: false, cycles: 0, dma_request: None, seeks: 0, sectors: 0, skipped: 0, dmas: 0, byteswap: false, descrambled: 0, q_reports: 0, loud: Loud::default(), trace: false }
    }
}

fn power_on_regs() -> [u16; 64] {
    let mut regs = [0; 64];
    regs[(REG_BMAN >> 1) as usize] = BMAN_RESET;
    regs[(REG_ATT_LEFT >> 1) as usize] = ATT_STRAIGHT;
    regs[(REG_ATT_RIGHT >> 1) as usize] = ATT_STRAIGHT;
    regs
}

/// Gain of a 6-bit attenuation field (2 dB steps, 0x3F is mute).
fn att_gain(v: u16) -> f32 {
    let v = v & 0x3F;
    if v == 0x3F { 0.0 } else { 10f32.powf(-(v as f32) * 2.0 / 20.0) }
}

impl Ciap {
    pub fn reset(&mut self) {
        self.regs = power_on_regs();
        (self.running, self.q_mode, self.lba, self.drive_active) = (false, false, 0, false);
        (self.start_delay, self.drive_stopped, self.toc, self.toc_index) = (0, false, false, 0);
        (self.dma_request, self.cycles) = (None, 0);
        (self.audio_playing, self.audio_wait, self.audio_next, self.audio_coding, self.asel) = (false, 0, 0, [0; 2], false);
        self.event_armed = false;
    }

    /// Drive request from IKAT port D: a seek positions the next sector and starts the drive, a stop halts delivery.
    pub fn drive(&mut self, ev: super::ikat::DriveEvent) {
        match ev {
            super::ikat::DriveEvent::Seek(lba) => {
                (self.lba, self.drive_active, self.start_delay, self.toc) = (lba, true, 1, false);
                self.seeks += 1;
            }
            super::ikat::DriveEvent::Stop => (self.drive_active, self.toc) = (false, false),
            // A table of contents read ends any running data read.
            super::ikat::DriveEvent::Toc => {
                (self.toc, self.toc_index, self.drive_active) = (true, 0, false);
                (self.start_delay, self.drive_stopped) = (0, false);
            }
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
            REG_BMAN => {
                let before = self.reg(REG_BMAN);
                self.set(REG_BMAN, before ^ value);
                // Queuing an audio buffer captures the coding of the last ASHDW write; unqueuing one is reported once.
                for (i, bit) in [(0usize, 0x01u16), (1, 0x02)] {
                    if value & bit != 0 && before & bit == 0 {
                        self.audio_coding[i] = ashdw_coding(self.reg(REG_ASHDW));
                    } else if value & bit != 0 && self.loud.first(0x9_0000 | bit as u32) {
                        eprintln!("CIAP audio buffer {} unqueued by the CPU pc~{:06X}", i, pc);
                    }
                }
            }
            REG_CCR => {
                self.set(REG_CCR, value);
                self.command(value, pc);
            }
            // Play control: 0x140 starts CPU-fed playback, which ends by itself once nothing is queued.
            REG_APCR => {
                self.set(REG_APCR, value);
                let queued = self.reg(REG_BMAN) & 0x03 != 0;
                match value {
                    0x0140 if !self.audio_playing => (self.audio_playing, self.audio_wait) = (true, 1),
                    // Audio event: 0xA0 at once (queued buffers still play out), 0x20 at once only when nothing is queued.
                    0x0020 | 0x00A0 if value == 0x00A0 || !queued => {
                        self.set(REG_ISR, self.reg(REG_ISR) | ISR_AUDIO_EVENT);
                        self.set(REG_ASTAT, self.reg(REG_ASTAT) | 0x80);
                        self.event_armed = false;
                    }
                    // 0x20 with a buffer queued: the event follows once the queue has played out.
                    0x0020 => self.event_armed = true,
                    0x0140 => {}
                    0x0006 | 0x0040 | 0x0101 | 0x0142 | 0x0200 => {
                        if self.loud.first(0x8_0000 | value as u32) {
                            eprintln!("CIAP APCR {:04X} accepted, no effect modelled pc~{:06X}", value, pc);
                        }
                    }
                    _ => {
                        if self.loud.first(0x8_0000 | value as u32) {
                            eprintln!("CIAP unmodelled APCR value {:04X} pc~{:06X}", value, pc);
                        }
                    }
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

    /// Advances the sector clock: one drive step every `sector_cycles`; the first sector comes on the second step after a seek, and a step with delivery off stops the drive.
    pub fn tick(&mut self, cycles: u32, sector_cycles: u32, cdrom: &mut Option<CdRom>, audio: &mut CdiAudio) {
        self.cycles += cycles;
        while self.cycles >= sector_cycles {
            self.cycles -= sector_cycles;
            self.audio_step(audio);
            if self.toc && self.running && self.q_mode {
                if let Some(cd) = cdrom.as_ref() {
                    self.toc_entry(cd);
                }
                continue;
            }
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
                self.sector(cd, audio);
            }
        }
    }

    // Current data buffer: buffer 0 when its bit is set, else buffer 1 when its bit is set.
    fn data_buffer(&self) -> Option<usize> {
        let bman = self.reg(REG_BMAN);
        DATA_BUFS.iter().find(|&&(bit, _)| bman & bit != 0).map(|&(_, at)| at)
    }

    // A data sector whose sync field reads byte-swapped marks the image swapped; failing headers are descrambled if that fixes them.
    fn repair(&mut self, lba: u32, raw: &mut [u8; CD_SECTOR_RAW_SIZE]) {
        if !self.byteswap && raw[..12] == SWAPPED_SYNC {
            self.byteswap = true;
            if self.loud.first(0x6_0000) {
                eprintln!("CIAP disc image is byte-swapped (sector {}); swapping every sector", lba);
            }
        }
        if self.byteswap {
            raw.chunks_exact_mut(2).for_each(|p| p.swap(0, 1));
        }
        if !header_valid(lba, raw) {
            let table = scramble_table();
            let mut copy = *raw;
            copy[12..].iter_mut().zip(&table[12..]).for_each(|(b, t)| *b ^= t);
            if header_valid(lba, &copy) {
                *raw = copy;
                self.descrambled += 1;
            }
        }
    }

    /// Output gains from the attenuation registers (left-to-left, left-to-right, right-to-right, right-to-left); None while they pass stereo straight through.
    pub fn attenuation(&self) -> Option<[f32; 4]> {
        let (a, b) = (self.reg(REG_ATT_LEFT), self.reg(REG_ATT_RIGHT));
        if (a & 0x3F3F, b & 0x3F3F) == (ATT_STRAIGHT, ATT_STRAIGHT) {
            return None;
        }
        Some([att_gain(a >> 8), att_gain(b), att_gain(b >> 8), att_gain(a)])
    }

    // Delivers the sector at the drive position into the current data buffer, then advances it; the buffer manager is left to the CPU.
    fn sector(&mut self, cd: &mut CdRom, audio: &mut CdiAudio) {
        let lba = self.lba;
        self.lba += 1;
        let mut raw = [0u8; CD_SECTOR_RAW_SIZE];
        if !cd.read_sector(lba, &mut raw) {
            if self.loud.first(0x2_0000) {
                eprintln!("CIAP sector {} unreadable", lba);
            }
            return;
        }
        self.repair(lba, &mut raw);
        if self.q_mode {
            self.q_locator(cd, lba);
        }
        let (file, channel, submode) = (raw[16], raw[17], raw[18]);
        // Form 2 audio on a TACS channel, with selection on, is played by the chip as it passes.
        if self.tacs_selected(submode, channel) {
            let coding = raw[23];
            // Own decoder state and queue, so a CPU-fed stream at the same time is not disturbed.
            audio.decode_audio_map_sector(&raw[24..24 + AUDIO_LEN], coding & 0x03 == 0x01, coding & 0x30 == 0x10, if coding & 0x0C == 0x04 { 18900 } else { 37800 });
            self.audio_sectors += 1;
            if self.audio_playing {
                self.audio_overlap += 1;
            }
        }
        // Every sector, audio included, also goes to the data buffer.
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

    // Q locator of a delivered sector in the Q-reporting modes: control/ADR, track, index 1, track-relative time, 0, absolute time (LBA plus 150), all BCD; current Q buffer, ISR bit 2.
    fn q_locator(&mut self, cd: &CdRom, lba: u32) {
        let bcd = |v: u32| (((v / 10) << 4) | (v % 10)) as u8;
        let msf = |f: u32| [bcd(f / 4500), bcd(f / 75 % 60), bcd(f % 75)];
        let t = cd.tracks.iter().rev().find(|t| t.start_lba <= lba);
        let (ctrl, num, start) = t.map_or((0x41, 1, 0), |t| ((t.control & 0xF0) | 0x01, t.number as u32, t.start_lba));
        let (r, a) = (msf(lba - start), msf(lba + 150));
        let at = if self.reg(REG_BMAN) & 0x10 != 0 { 0x1B24 } else { 0x24E6 };
        self.ram[at..at + 10].copy_from_slice(&[ctrl, bcd(num), 0x01, r[0], r[1], r[2], 0x00, a[0], a[1], a[2]]);
        self.set(REG_ISR, self.reg(REG_ISR) | 0x0004);
        self.q_reports += 1;
    }

    // One lead-in Q entry per sector period into the current Q buffer: A0, A1, A2, then each track start, repeating.
    fn toc_entry(&mut self, cd: &CdRom) {
        let bcd = |v: u32| (((v / 10) << 4) | (v % 10)) as u8;
        let msf = |lba: u32| { let f = lba + 150; [bcd(f / 4500), bcd(f / 75 % 60), bcd(f % 75)] };
        let tracks = &cd.tracks;
        let (first, last) = (tracks.first().map_or(1, |t| t.number), tracks.last().map_or(1, |t| t.number));
        let ctrl = |c: u8| (c & 0xF0) | 0x01;
        let i = self.toc_index % (3 + tracks.len());
        self.toc_index += 1;
        let (c, point, p) = match i {
            0 => (ctrl(tracks.first().map_or(0x41, |t| t.control)), 0xA0, [bcd(first as u32), cd.disc_type, 0]),
            1 => (ctrl(tracks.last().map_or(0x41, |t| t.control)), 0xA1, [bcd(last as u32), 0, 0]),
            2 => (0x41, 0xA2, msf(cd.total_sectors)),
            n => {
                let t = &tracks[n - 3];
                (ctrl(t.control), bcd(t.number as u32), msf(t.start_lba))
            }
        };
        let at = if self.reg(REG_BMAN) & 0x10 != 0 { 0x1B24 } else { 0x24E6 };
        self.ram[at..at + 10].copy_from_slice(&[c, 0x00, point, 0, 0, 0, 0, p[0], p[1], p[2]]);
        self.set(REG_ISR, self.reg(REG_ISR) | 0x0004);
        if self.trace {
            eprintln!("CIAP TOC point {:02X} {:02X} {:02X} {:02X} at {:04X}", point, p[0], p[1], p[2], at);
        }
    }

    // Selection on, Form 2 audio submode, channel in TACS.
    fn tacs_selected(&self, submode: u8, channel: u8) -> bool {
        self.asel && submode & 0x24 == 0x24 && self.reg(REG_TACS) & (1 << (channel & 15)) != 0
    }

    // CPU-fed playback, one step per sector period: when the current buffer has played, take the next queued one, play it, hand it back (BMAN bit cleared, ISR bit 1); stop when none is queued.
    fn audio_step(&mut self, audio: &mut CdiAudio) {
        if !self.audio_playing {
            return;
        }
        self.audio_wait = self.audio_wait.saturating_sub(1);
        if self.audio_wait > 0 {
            return;
        }
        let bman = self.reg(REG_BMAN);
        let first = self.audio_next & 1;
        let pick = [first, first ^ 1].into_iter().find(|&i| bman & (1 << i) != 0);
        let Some(i) = pick else {
            self.audio_playing = false;
            if std::mem::take(&mut self.event_armed) {
                self.set(REG_ISR, self.reg(REG_ISR) | ISR_AUDIO_EVENT);
                self.set(REG_ASTAT, self.reg(REG_ASTAT) | 0x80);
                if self.trace {
                    eprintln!("CIAP AUDIO event after the queue played out");
                }
            }
            return;
        };
        let (at, coding) = (AUDIO_BUFS[i], self.audio_coding[i]);
        audio.decode_sector(&self.ram[at..at + AUDIO_LEN], coding & 0x03 == 0x01, coding & 0x30 == 0x10, if coding & 0x0C == 0x04 { 18900 } else { 37800 });
        self.set(REG_BMAN, bman & !(1 << i));
        self.set(REG_ISR, self.reg(REG_ISR) | ISR_AUDIO_DONE);
        self.set(REG_APTR, (at as u16 + 0x800) / 2);
        (self.audio_wait, self.audio_next) = (crate::cdic::get_sector_count_for_coding(coding), i ^ 1);
        self.audio_buffers += 1;
        if self.trace {
            eprintln!("CIAP AUDIO buffer {} coding {:02X} played, next in {} periods", i, coding, self.audio_wait);
        }
    }

    // Commands: 0x94, 0xC4, 0x44 and 0x80 start delivery, 0x100 stops it; Q reporting with 0x10, 0x44 and 0x94, not with 0xC4.
    fn command(&mut self, v: u16, pc: u32) {
        match v {
            0x0094 => (self.running, self.q_mode) = (true, true),
            0x00C4 => (self.running, self.q_mode) = (true, false),
            0x0100 => (self.running, self.q_mode, self.asel) = (false, false, false),
            0x0044 => (self.running, self.q_mode) = (true, true),
            0x0080 => self.running = true,
            0x0010 => self.q_mode = true,
            // Accepted without a modelled effect; logged once so a run shows they arrived.
            0x0008 => {
                self.asel = true;
                if self.loud.first(0x1_0008) {
                    eprintln!("CIAP command 0008: selection on, TACS audio played by the chip (inferred) pc~{:06X}", pc);
                }
            }
            0x7000 | 0x3000 | 0x0200 | 0x0040 => {
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
        c.tick(period, period, &mut None, &mut CdiAudio::new());
        assert!(c.drive_active && !c.drive_stopped);
        c.tick(period, period, &mut None, &mut CdiAudio::new());
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
    fn repair_unswaps_and_descrambles() {
        let mut good = [0u8; CD_SECTOR_RAW_SIZE];
        good[1..11].fill(0xFF);
        good[12..20].copy_from_slice(&[0x00, 0x02, 0x16, 0x02, 0x00, 0x00, 0x09, 0x00]);
        good[20..24].copy_from_slice(&[0x00, 0x00, 0x09, 0x00]);
        assert!(header_valid(16, &good));
        let mut scrambled = good;
        scrambled[12..].iter_mut().zip(&scramble_table()[12..]).for_each(|(b, t)| *b ^= t);
        let mut c = Ciap::default();
        c.repair(16, &mut scrambled);
        assert_eq!((scrambled, c.descrambled), (good, 1));
        let mut swapped = good;
        swapped.chunks_exact_mut(2).for_each(|p| p.swap(0, 1));
        c.repair(16, &mut swapped);
        assert!(c.byteswap && swapped == good);
    }

    #[test]
    fn toc_request_ends_a_data_read_and_reset_clears_it() {
        let mut c = Ciap::default();
        c.drive(super::super::ikat::DriveEvent::Seek(13));
        c.drive(super::super::ikat::DriveEvent::Toc);
        assert!(c.toc && !c.drive_active);
        c.reset();
        assert!(!c.toc && c.toc_index == 0);
    }

    #[test]
    fn cpu_fed_audio_plays_queued_buffers_and_hands_them_back() {
        let mut c = Ciap::default();
        let mut audio = CdiAudio::new();
        let period = crate::mcd212::VideoMode::Pal.cycles_per_sector();
        c.write_reg(REG_ASHDW, 0x0800, 0);
        c.write_reg(REG_BMAN, 0x01, 0);
        c.write_reg(REG_BMAN, 0x02, 0);
        assert_eq!((c.peek_reg(REG_BMAN) & 3, c.audio_coding), (3, [0x01, 0x01]));
        c.write_reg(REG_APCR, 0x20, 0);
        assert_eq!(c.peek_reg(REG_ISR) & ISR_AUDIO_EVENT, 0);
        c.write_reg(REG_APCR, 0x140, 0);
        c.write_reg(REG_IER, ISR_AUDIO_DONE, 0);
        c.tick(period, period, &mut None, &mut audio);
        assert_eq!((c.peek_reg(REG_BMAN) & 3, c.read_reg(REG_ISR, 0) & ISR_AUDIO_DONE), (2, ISR_AUDIO_DONE));
        c.tick(period * 3, period, &mut None, &mut audio);
        assert_eq!(c.peek_reg(REG_BMAN) & 3, 2);
        c.tick(period, period, &mut None, &mut audio);
        assert_eq!((c.peek_reg(REG_BMAN) & 3, c.audio_buffers), (0, 2));
        c.tick(period * 4, period, &mut None, &mut audio);
        // The 0x20 written while buffers were queued raises the event once the queue has played out.
        assert!(!c.audio_playing && !c.event_armed);
        assert_eq!(c.read_reg(REG_ISR, 0) & ISR_AUDIO_EVENT, ISR_AUDIO_EVENT);
        c.tick(period * 4, period, &mut None, &mut audio);
        assert_eq!(c.peek_reg(REG_ISR) & ISR_AUDIO_EVENT, 0);
        c.write_reg(REG_APCR, 0x20, 0);
        assert_eq!(c.peek_reg(REG_ISR) & ISR_AUDIO_EVENT, ISR_AUDIO_EVENT);
        assert!(audio.total_sectors_decoded == 2);
    }

    #[test]
    fn attenuation_registers_set_the_output_gains() {
        let mut c = Ciap::default();
        assert_eq!(c.attenuation(), None);
        // Left and right each pass to both outputs at full level (the register pairs are LLA/RLA and RRA/LRA).
        c.write_reg(REG_ATT_LEFT, 0x0000, 0);
        c.write_reg(REG_ATT_RIGHT, 0x0000, 0);
        assert_eq!(c.attenuation(), Some([1.0, 1.0, 1.0, 1.0]));
        // Left output 2 dB down (field 1), right output muted.
        c.write_reg(REG_ATT_LEFT, 0x0101, 0);
        c.write_reg(REG_ATT_RIGHT, 0x3F3F, 0);
        let g = c.attenuation().unwrap();
        assert!((g[0] - 0.7943).abs() < 1e-3 && g[3] == g[0] && g[1] == 0.0 && g[2] == 0.0);
        c.write_reg(REG_ATT_LEFT, ATT_STRAIGHT, 0);
        c.write_reg(REG_ATT_RIGHT, ATT_STRAIGHT, 0);
        assert_eq!(c.attenuation(), None);
    }

    #[test]
    fn audio_stop_tacs_reset_and_state() {
        let period = crate::mcd212::VideoMode::Pal.cycles_per_sector();
        let mut audio = CdiAudio::new();
        let mut c = Ciap::default();
        c.write_reg(REG_BMAN, 0x03, 0);
        c.write_reg(REG_APCR, 0x140, 0);
        c.tick(period, period, &mut None, &mut audio);
        assert_eq!(c.peek_reg(REG_BMAN) & 3, 2);
        // 0xA0 raises the event at once; the queued buffer still plays and playback then ends.
        c.write_reg(REG_APCR, 0xA0, 0);
        assert!(c.audio_playing && c.peek_reg(REG_ISR) & ISR_AUDIO_EVENT != 0);
        c.tick(period * 8, period, &mut None, &mut audio);
        assert_eq!((c.peek_reg(REG_BMAN) & 3, c.audio_buffers), (0, 2));
        c.tick(period * 8, period, &mut None, &mut audio);
        assert!(!c.audio_playing);
        // With the queue drained, a later 0x20 raises the event.
        c.read_reg(REG_ISR, 0);
        c.write_reg(REG_APCR, 0x20, 0);
        assert_ne!(c.peek_reg(REG_ISR) & ISR_AUDIO_EVENT, 0);
        // A buffer queued while playing is played in turn.
        c.write_reg(REG_APCR, 0x140, 0);
        c.write_reg(REG_BMAN, 0x01, 0);
        c.tick(period * 9, period, &mut None, &mut audio);
        assert_eq!(c.peek_reg(REG_BMAN) & 3, 0);
        // State round trip during playback.
        c.write_reg(REG_ASHDW, 0x3000, 0);
        c.write_reg(REG_BMAN, 0x03, 0);
        c.write_reg(REG_APCR, 0x140, 0);
        c.tick(period, period, &mut None, &mut audio);
        assert_eq!(c.audio_coding[1], 0x14);
        // TACS selection needs ASEL, Form 2 audio and the channel bit.
        c.write_reg(REG_TACS, 0x8000, 0);
        assert!(!c.tacs_selected(0x64, 15));
        c.write_reg(REG_CCR, 0x0008, 0);
        assert!(c.tacs_selected(0x64, 15) && !c.tacs_selected(0x64, 14) && !c.tacs_selected(0x48, 15));
        c.write_reg(REG_CCR, 0x0100, 0);
        assert!(!c.tacs_selected(0x64, 15));
        // Unmodelled APCR values are reported; reset clears the audio fields.
        let before = c.loud.count;
        c.write_reg(REG_APCR, 0x1234, 0);
        assert_eq!(c.loud.count, before + 1);
        c.write_reg(REG_CCR, 0x0008, 0);
        c.reset();
        assert_eq!((c.audio_playing, c.audio_wait, c.audio_next, c.audio_coding, c.asel), (false, 0, 0, [0; 2], false));
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

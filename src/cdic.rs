use log::{info, debug, trace, error};
use crate::cdrom::{CdRom, CD_SECTOR_RAW_SIZE};
use crate::audio::CdiAudio;

pub const CDIC_RAM_SIZE: usize = 0x3C00; // 15360 bytes

// Generate Yellow Book scrambler table (2448 bytes)
fn generate_sector_scramble() -> [u8; 2448] {
    let mut table = [0u8; 2448];
    let mut reg: u16 = 0x0001;

    for i in 12..2448 {
        let mut byte_val = 0u8;
        for bit in 0..8 {
            let out_bit = (reg & 1) as u8;
            byte_val |= out_bit << bit;
            let feedback = (reg ^ (reg >> 1)) & 1;
            reg = (reg >> 1) | (feedback << 14);
        }
        table[i] = byte_val;
    }
    table
}

pub const CRC_CCITT_TABLE: [u16; 256] = [
    0x0000, 0x1021, 0x2042, 0x3063, 0x4084, 0x50a5, 0x60c6, 0x70e7,
    0x8108, 0x9129, 0xa14a, 0xb16b, 0xc18c, 0xd1ad, 0xe1ce, 0xf1ef,
    0x1231, 0x0210, 0x3273, 0x2252, 0x52b5, 0x4294, 0x72f7, 0x62d6,
    0x9339, 0x8318, 0xb37b, 0xa35a, 0xd3bd, 0xc39c, 0xf3ff, 0xe3de,
    0x2462, 0x3443, 0x0420, 0x1401, 0x64e6, 0x74c7, 0x44a4, 0x5485,
    0xa56a, 0xb54b, 0x8528, 0x9509, 0xe5ee, 0xf5cf, 0xc5ac, 0xd58d,
    0x3653, 0x2672, 0x1611, 0x0630, 0x76d7, 0x66f6, 0x5695, 0x46b4,
    0xb75b, 0xa77a, 0x9719, 0x8738, 0xf7df, 0xe7fe, 0xd79d, 0xc7bc,
    0x48c4, 0x58e5, 0x6886, 0x78a7, 0x0840, 0x1861, 0x2802, 0x3823,
    0xc9cc, 0xd9ed, 0xe98e, 0xf9af, 0x8948, 0x9969, 0xa90a, 0xb92b,
    0x5af5, 0x4ad4, 0x7ab7, 0x6a96, 0x1a71, 0x0a50, 0x3a33, 0x2a12,
    0xdbfd, 0xcbdc, 0xfbbf, 0xeb9e, 0x9b79, 0x8b58, 0xbb3b, 0xab1a,
    0x6ca6, 0x7c87, 0x4ce4, 0x5cc5, 0x2c22, 0x3c03, 0x0c60, 0x1c41,
    0xedae, 0xfd8f, 0xcdec, 0xddcd, 0xad2a, 0xbd0b, 0x8d68, 0x9d49,
    0x7e97, 0x6eb6, 0x5ed5, 0x4ef4, 0x3e13, 0x2e32, 0x1e51, 0x0e70,
    0xff9f, 0xefbe, 0xdfdd, 0xcffc, 0xbf1b, 0xaf3a, 0x9f59, 0x8f78,
    0x9188, 0x81a9, 0xb1ca, 0xa1eb, 0xd10c, 0xc12d, 0xf14e, 0xe16f,
    0x1080, 0x00a1, 0x30c2, 0x20e3, 0x5004, 0x4025, 0x7046, 0x6067,
    0x83b9, 0x9398, 0xa3fb, 0xb3da, 0xc33d, 0xd31c, 0xe37f, 0xf35e,
    0x02b1, 0x1290, 0x22f3, 0x32d2, 0x4235, 0x5214, 0x6277, 0x7256,
    0xb5ea, 0xa5cb, 0x95a8, 0x8589, 0xf56e, 0xe54f, 0xd52c, 0xc50d,
    0x34e2, 0x24c3, 0x14a0, 0x0481, 0x7466, 0x6447, 0x5424, 0x4405,
    0xa7db, 0xb7fa, 0x8799, 0x97b8, 0xe75f, 0xf77e, 0xc71d, 0xd73c,
    0x26d3, 0x36f2, 0x0691, 0x16b0, 0x6657, 0x7676, 0x4615, 0x5634,
    0xd94c, 0xc96d, 0xf90e, 0xe92f, 0x99c8, 0x89e9, 0xb98a, 0xa9ab,
    0x5844, 0x4865, 0x7806, 0x6827, 0x18c0, 0x08e1, 0x3882, 0x28a3,
    0xcb7d, 0xdb5c, 0xeb3f, 0xfb1e, 0x8bf9, 0x9bd8, 0xabbb, 0xbb9a,
    0x4a75, 0x5a54, 0x6a37, 0x7a16, 0x0af1, 0x1ad0, 0x2ab3, 0x3a92,
    0xfd2e, 0xed0f, 0xdd6c, 0xcd4d, 0xbdaa, 0xad8b, 0x9de8, 0x8dc9,
    0x7c26, 0x6c07, 0x5c64, 0x4c45, 0x3ca2, 0x2c83, 0x1ce0, 0x0cc1,
    0xef1f, 0xff3e, 0xcf5d, 0xdf7c, 0xaf9b, 0xbfba, 0x8fd9, 0x9ff8,
    0x6e17, 0x7e36, 0x4e55, 0x5e74, 0x2e93, 0x3eb2, 0x0ed1, 0x1ef0
];

pub struct Cdic {
    pub ram: Box<[u8; CDIC_RAM_SIZE]>,
    pub audio: CdiAudio,

    // Registers (0x303C00..0x303FFF)
    pub command: u16,          // 0x3C00
    pub time: u32,             // 0x3C02 / 0x3C04
    pub file: u16,             // 0x3C06
    pub channel: u32,          // 0x3C08 / 0x3C0A
    pub audio_channel: u16,    // 0x3C0C
    pub audio_buffer: u16,     // 0x3FF4 (ABUF)
    pub x_buffer: u16,         // 0x3FF6 (XBUF)
    pub dma_control: u16,      // 0x3FF8
    pub z_buffer: u16,         // 0x3FFA
    pub interrupt_vector: u16, // 0x3FFC
    pub data_buffer: u16,      // 0x3FFE

    // Disc playback state
    pub disc_command: u8,
    pub disc_mode: u8,
    pub disc_spinup_counter: u8,
    pub curr_lba: u32,

    // Audio Map State
    pub audio_sector_counter: u8,
    // Audio map player: playback on, next buffer, buffer playing (0xFFFF none) with its coding, groups decoded and cycles played.
    pub map_on: bool,
    pub map_next: u16,
    pub buf_addr: u16,
    pub buf_coding: u8,
    pub buf_group: u8,
    pub buf_elapsed: u32,
    pub map_finished: bool,
    pub map_sector_cycles: u32,

    // Timing & Interrupt
    pub cycle_accumulator: u32,
    pub irq_pending: bool,
    pub dma_triggered: bool,

    pub scramble_table: [u8; 2448],
    pub cd_byteswap: bool,
}

impl Cdic {
    pub fn new() -> Self {
        Self {
            ram: Box::new([0; CDIC_RAM_SIZE]),
            audio: CdiAudio::new(),

            command: 0,
            time: 0,
            file: 0,
            channel: 0xFFFF_FFFF,
            audio_channel: 0xFFFF,
            audio_buffer: 0,
            x_buffer: 0,
            dma_control: 0,
            z_buffer: 0,
            interrupt_vector: 0x0F,
            data_buffer: 0,

            disc_command: 0,
            disc_mode: 0,
            disc_spinup_counter: 0,
            curr_lba: 0,

            audio_sector_counter: 0,
            map_on: false,
            map_next: 0x2800,
            buf_addr: 0xFFFF,
            buf_coding: 0,
            buf_group: 0,
            buf_elapsed: 0,
            map_finished: false,
            map_sector_cycles: crate::mcd212::VideoMode::Pal.cycles_per_sector(),

            cycle_accumulator: 0,
            irq_pending: false,
            dma_triggered: false,

            scramble_table: generate_sector_scramble(),
            cd_byteswap: false,
        }
    }

    pub fn reset(&mut self) {
        self.ram.fill(0);
        self.audio.reset();

        self.command = 0;
        self.time = 0;
        self.file = 0;
        self.channel = 0xFFFF_FFFF;
        self.audio_channel = 0xFFFF;
        self.audio_buffer = 0;
        self.x_buffer = 0;
        self.dma_control = 0;
        self.z_buffer = 0;
        self.interrupt_vector = 0x0F;
        self.data_buffer = 0;

        self.disc_command = 0;
        self.disc_mode = 0;
        self.disc_spinup_counter = 0;
        self.curr_lba = 0;
        self.cd_byteswap = false;

        self.audio_sector_counter = 0;
        self.map_on = false;
        self.map_next = 0x2800;
        self.buf_addr = 0xFFFF;
        self.buf_coding = 0;
        self.buf_group = 0;
        self.buf_elapsed = 0;
        self.map_finished = false;

        self.cycle_accumulator = 0;
        self.irq_pending = false;
        self.dma_triggered = false;
    }

    pub fn read_reg(&mut self, offset: u32) -> u16 {
        match offset {
            0x00 => {
                trace!("CDIC read Command: 0x{:04X}", self.command);
                self.command
            }
            0x02 => (self.time >> 16) as u16,
            0x04 => (self.time & 0xFFFF) as u16,
            0x06 => self.file,
            0x08 => (self.channel >> 16) as u16,
            0x0A => (self.channel & 0xFFFF) as u16,
            0x0C => self.audio_channel,

            0x3F4 => {
                let temp = self.audio_buffer;
                self.audio_buffer &= 0x7FFF;
                self.update_interrupt();
                trace!("CDIC read ABUF: 0x{:04X}", temp);
                temp
            }
            0x3F6 => {
                let temp = self.x_buffer;
                self.x_buffer &= 0x7FFF;
                self.update_interrupt();
                trace!("CDIC read XBUF: 0x{:04X}", temp);
                temp
            }
            0x3FA => {
                // Bit 11 shows playback and bit 0 the finished flag, cleared by the read; with an audio channel set an idle read keeps the old alternating bit 0.
                if self.audio_channel == 0 || self.map_finished || self.map_playing() {
                    let r = (self.z_buffer & !0x0801) | if self.map_playing() { 0x0800 } else { 0 } | self.map_finished as u16;
                    self.map_finished = false;
                    return r;
                }
                if self.buf_addr == 0xFFFF {
                    self.z_buffer ^= 0x0001;
                }
                self.z_buffer
            }
            0x3FE => {
                let temp = self.data_buffer;
                trace!("CDIC read DBUF: 0x{:04X}", temp);
                temp
            }

            _ => 0,
        }
    }

    pub fn write_reg(&mut self, offset: u32, val: u16) {
        match offset {
            0x00 => {
                debug!("CDIC WRITE command = 0x{:04X}", val);
                self.command = val;
            }
            0x02 => self.time = (self.time & 0x0000_FFFF) | ((val as u32) << 16),
            0x04 => self.time = (self.time & 0xFFFF_0000) | (val as u32),
            0x06 => self.file = val,
            0x08 => self.channel = (self.channel & 0x0000_FFFF) | ((val as u32) << 16),
            0x0A => self.channel = (self.channel & 0xFFFF_0000) | (val as u32),
            0x0C => {
                info!("CDIC WRITE audio_channel = 0x{:04X}", val);
                self.audio_channel = val;
            }

            0x3F4 => {
                self.audio_buffer = val;
                self.update_interrupt();
            }
            0x3F6 => {
                self.x_buffer = val;
                self.update_interrupt();
            }
            0x3F8 => {
                info!("CDIC WRITE dma_control = 0x{:04X}", val);
                self.dma_control = val;
                self.dma_triggered = true;
            }
            0x3FA => {
                self.z_buffer = val;
                trace!("CDIC WRITE audio control = 0x{:04X} (on={}, buffer=0x{:04X})", val, self.map_on, self.buf_addr);
                // Bit 11 alone starts the map only with no audio channel; an empty map on an idle decoder ends at once with the finished flag.
                let bit11_start = (val & 0x0800) != 0 && self.audio_channel == 0 && (val & 0x2000) == 0;
                if bit11_start || (val & 0x2000) != 0 {
                    // A start while a map plays is ignored; over a stopped buffer still playing it waits for that buffer.
                    if !self.map_on {
                        self.map_on = true;
                        self.map_next = if bit11_start { 0x2800 } else { val & 0x3A00 };
                        if self.buf_addr == 0xFFFF && !self.start_map_buffer(0) && bit11_start {
                            self.map_finished = true;
                        }
                    }
                } else {
                    self.map_on = false;
                }
                self.update_interrupt();
            }
            0x3FC => {
                trace!("CDIC WRITE interrupt_vector = 0x{:04X}", val);
                self.interrupt_vector = val;
            }
            0x3FE => {
                trace!("CDIC WRITE data_buffer = 0x{:04X}", val);
                self.data_buffer = val;
                if (self.data_buffer & 0x8000) != 0 {
                    self.handle_cdic_command();
                }
                if (self.data_buffer & 0x4000) == 0 {
                    self.cancel_disc_read();
                }
            }
            _ => {}
        }
    }

    fn map_playing(&self) -> bool {
        self.map_on
    }

    // ABUF interrupts only while AUDCTL bit 13 is set.
    fn update_interrupt(&mut self) {
        let abuf = (self.audio_buffer & 0x8000) != 0 && (self.z_buffer & 0x2000) != 0;
        self.irq_pending = (self.x_buffer & 0x8000) != 0 || abuf;
    }

    fn lba_from_time(&self) -> u32 {
        let bcd_mins = ((self.time >> 24) & 0xFF) as u8;
        let raw_mins = ((bcd_mins >> 4) * 10) + (bcd_mins & 0xF);

        let bcd_secs = ((self.time >> 16) & 0xFF) as u8;
        let raw_secs = ((bcd_secs >> 4) * 10) + (bcd_secs & 0xF);

        let mut lba = ((raw_mins as u32 * 60) + raw_secs as u32) * 75;

        let bcd_frac = ((self.time >> 8) & 0xFF) as u8;
        if (bcd_frac & 0x80) == 0 {
            let raw_frac = ((bcd_frac >> 4) * 10) + (bcd_frac & 0xF);
            lba += raw_frac as u32;
        }

        if lba >= 150 {
            lba -= 150;
        }
        lba
    }

    fn init_disc_read(&mut self, mode: u8) {
        let new_lba = if mode == 4 { 0 } else { self.lba_from_time() };
        let is_seek = (new_lba as i64 - self.curr_lba as i64).abs() > 4;

        self.disc_command = self.command as u8;
        self.disc_mode = mode;
        self.curr_lba = new_lba;
        self.disc_spinup_counter = 0;
        self.cycle_accumulator = 170_000;

        if is_seek {
            self.audio.left_state.reset();
            self.audio.right_state.reset();
            self.audio.main.clear();
        }

        info!(
            "CDIC init disc read: cmd=0x{:02X}, mode={}, LBA={}, is_seek={}",
            self.disc_command, self.disc_mode, self.curr_lba, is_seek
        );
    }

    pub fn cancel_disc_read(&mut self) {
        debug!("CDIC cancel_disc_read called (curr_lba={})", self.curr_lba);
        self.disc_command = 0;
        self.disc_mode = 0;
        self.curr_lba = 0;
        self.disc_spinup_counter = 0;
        self.cycle_accumulator = 0;
    }

    fn handle_cdic_command(&mut self) {
        info!("CDIC Command 0x{:02X} received (LBA={}, time=0x{:08X})", self.command, self.curr_lba, self.time);
        match self.command {
            0x23 | 0x24 => {
                // Reset Mode 1 / Reset Mode 2: read single sector then cancel in process_sector
                if self.disc_command == 0 {
                    self.init_disc_read(1);
                }
            }
            0x2B => {
                // Stop CDDA
                self.cancel_disc_read();
            }
            0x2E => {
                // Update
            }
            0x27 => {
                // Fetch TOC
                self.init_disc_read(4);
            }
            0x28 => {
                // Play CDDA
                self.init_disc_read(3);
            }
            0x29 | 0x2C => {
                // Read Mode 1 / Seek
                self.init_disc_read(1);
            }
            0x2A => {
                // Read Mode 2
                self.init_disc_read(2);
            }
            _ => {
                debug!("CDIC: Unhandled command 0x{:02X}", self.command);
            }
        }
        self.data_buffer &= !0x8000;
    }
}

pub fn get_sector_count_for_coding(coding: u8) -> u8 {
    let mut base_count = 2u8;
    if (coding & 0x30) == 0x00 {
        base_count *= 2;
    }
    if (coding & 0x0C) == 0x04 {
        base_count *= 2;
    }
    if (coding & 0x03) == 0x00 {
        base_count *= 2;
    }
    base_count
}

impl Cdic {
    // Starts the buffer at `map_next`, `elapsed` cycles in; coding 0xFF ends the map instead (returns false).
    fn start_map_buffer(&mut self, elapsed: u32) -> bool {
        let addr = (self.map_next & 0x3FFE) as usize;
        let coding = if addr + 12 + 2304 <= CDIC_RAM_SIZE { self.ram[addr + 11] } else { 0xFF };
        if coding == 0xFF {
            trace!("CDIC AUDIO MAP END: addr=0x{:04X}, coding=0xFF", addr);
            self.map_on = false;
            return false;
        }
        self.buf_addr = addr as u16;
        self.buf_coding = coding;
        self.buf_group = 0;
        self.buf_elapsed = elapsed;
        self.map_next ^= 0x1A00;
        self.decode_map_groups();
        true
    }

    fn map_buffer_cycles(&self) -> u32 {
        get_sector_count_for_coding(self.buf_coding) as u32 * self.map_sector_cycles
    }

    // The decoder reads buffer RAM as it plays: each of the 18 sound groups is decoded when its time comes.
    fn decode_map_groups(&mut self) {
        let len = self.map_buffer_cycles() as u64;
        let due = ((self.buf_elapsed as u64 * 18 / len) as u8 + 1).min(18);
        if self.buf_group >= due {
            return;
        }
        let coding = self.buf_coding;
        let is_stereo = (coding & 0x03) == 0x01;
        let is_8bit = (coding & 0x30) == 0x10;
        let sample_rate = if (coding & 0x0C) == 0x04 { 18900 } else { 37800 };
        let start = self.buf_addr as usize + 12 + self.buf_group as usize * 128;
        let end = self.buf_addr as usize + 12 + due as usize * 128;
        let first = self.buf_group == 0;
        self.audio.decode_audio_map_groups(&self.ram[start..end], first, is_stereo, is_8bit, sample_rate);
        self.buf_group = due;
    }

    // Plays the audio map buffer in real time; when it has played ABUF bit 15 is set (also after a stop) and the next buffer starts at once.
    fn step_audio_map(&mut self, cycles: u32) {
        if self.buf_addr == 0xFFFF {
            return;
        }
        self.buf_elapsed += cycles;
        loop {
            let len = self.map_buffer_cycles();
            if self.buf_elapsed < len {
                self.decode_map_groups();
                return;
            }
            self.decode_map_groups();
            let rest = self.buf_elapsed - len;
            self.buf_addr = 0xFFFF;
            self.audio_buffer |= 0x8000;
            self.update_interrupt();
            if !self.map_on || !self.start_map_buffer(rest) {
                return;
            }
        }
    }

    // `sector_cycles` is one sector at 1x speed (75 per second) in CPU cycles of the current video standard.
    pub fn step_cycles(&mut self, cycles: u32, sector_cycles: u32, cdrom: &mut Option<CdRom>) {
        // 1. Audio map playback
        self.map_sector_cycles = sector_cycles;
        self.step_audio_map(cycles);

        // 2. Disc sector read (75 Hz)
        if self.disc_command == 0 {
            return;
        }

        self.cycle_accumulator += cycles;
        while self.cycle_accumulator >= sector_cycles {
            self.cycle_accumulator -= sector_cycles;

            if self.disc_spinup_counter > 0 {
                self.disc_spinup_counter -= 1;
                continue;
            }

            self.process_sector(cdrom);

            if self.disc_command == 0 {
                break;
            }

            self.curr_lba += 1;
        }
    }

    pub fn is_mode2_sector_selected(&mut self, buf: &[u8]) -> bool {
        let file_num = (buf[20] as u16) << 8;
        if file_num != self.file {
            info!("CDIC filter REJECT FILE: LBA={}, buf[20]=0x{:02X} (val=0x{:04X}) vs self.file=0x{:04X}",
                self.curr_lba, buf[20], file_num, self.file);
            return false;
        }

        let submode = buf[22];
        if (submode & 0x80) != 0 {
            debug!("CDIC SUBMODE_EOF at LBA={}", self.curr_lba);
            // SUBMODE_EOF: queue end of read after this sector
            self.disc_command = 0;
        }

        // End-of-File, Trigger, or End-of-Record sectors skip selection beyond initial file selection
        if (submode & (0x80 | 0x10 | 0x01)) != 0 {
            return true;
        }

        // Sectors with no applicable data (RT 0x40, FORM2 0x20, DATA 0x08, AUDIO 0x04, VIDEO 0x02) are skipped.
        // Real-Time and Form 2 sectors contain valid streaming media (video/data) in CD-i.
        if (submode & (0x40 | 0x20 | 0x08 | 0x04 | 0x02)) == 0 {
            info!("CDIC filter REJECT NO APPLICABLE DATA: LBA={}, submode=0x{:02X}", self.curr_lba, submode);
            return false;
        }

        // Channel mask selection
        let chan = (buf[21] & 0x1F) as usize;
        let chan_selected = (self.channel & (1 << chan)) != 0;
        if !chan_selected {
            info!("CDIC filter REJECT CHAN: LBA={}, chan={}, self.channel=0x{:08X}", self.curr_lba, chan, self.channel);
        }
        chan_selected
    }

    pub fn is_mode2_audio_selected(&self, buf: &[u8]) -> bool {
        let submode = buf[22];
        // Must be Form 2 (0x20) and Audio (0x04)
        if (submode & 0x24) != 0x24 {
            return false;
        }

        // Green Book CD-i ADPCM Coding Information check:
        // Byte 23 (sub-header coding information):
        // Bit 6 must be 0 (reserved).
        // Bits 4-5 cannot be 11 (invalid bits/sample).
        // Bits 2-3 cannot be 10 or 11 (invalid sample rate).
        // Bits 0-1 cannot be 10 or 11 (invalid channels).
        // MPEG/DVC audio sectors have coding = 0x7F or start with MPEG sync.
        let coding = buf[23];
        if (coding & 0x40) != 0 || (coding & 0x30) == 0x30 || (coding & 0x0C) >= 0x08 || (coding & 0x03) >= 0x02 {
            return false;
        }

        // Also verify payload does not start with MPEG pack header (00 00 01 BA)
        if buf.len() >= 28 && buf[24] == 0x00 && buf[25] == 0x00 && buf[26] == 0x01 && buf[27] == 0xBA {
            return false;
        }

        // Channel numbers run 0-31 but the mask has 16 bits, so channels 16-31 are never selected
        let chan = (buf[21] & 0x1F) as usize;
        chan < 16 && (self.audio_channel & (1 << chan)) != 0
    }

    pub fn is_valid_sector(&self, buffer: &[u8]) -> bool {
        if buffer.len() < 24 {
            return false;
        }

        let real_lba = self.curr_lba + 150;
        let mins = (real_lba / (60 * 75)) as u8;
        let secs = ((real_lba / 75) % 60) as u8;
        let frac = (real_lba % 75) as u8;
        let mins_bcd = ((mins / 10) << 4) | (mins % 10);
        let secs_bcd = ((secs / 10) << 4) | (secs % 10);
        let frac_bcd = ((frac / 10) << 4) | (frac % 10);

        // Verify MSF
        if buffer[12] != mins_bcd || buffer[13] != secs_bcd || buffer[14] != frac_bcd {
            return false;
        }

        // Verify mode
        if buffer[15] != 1 && buffer[15] != 2 {
            return false;
        }

        // Verify duplicate info if mode 2
        if buffer[15] == 2 {
            if buffer[16] != buffer[20]
                || buffer[17] != buffer[21]
                || buffer[18] != buffer[22]
                || buffer[19] != buffer[23]
            {
                return false;
            }
        }

        true
    }

    pub fn descramble_sector(&self, buffer: &mut [u8]) {
        for i in 12..buffer.len().min(CD_SECTOR_RAW_SIZE) {
            buffer[i] ^= self.scramble_table[i];
        }
    }

    fn process_sector(&mut self, cdrom: &mut Option<CdRom>) {
        info!("CDIC process_sector: LBA={}, mode={}", self.curr_lba, self.disc_mode);
        let Some(cd) = cdrom else {
            error!("CDIC process_sector: cdrom is None!");
            return;
        };

        let is_toc = self.disc_mode == 4 || self.command == 0x27 || self.disc_command == 0x27;

        let mut sector_buf = [0u8; CD_SECTOR_RAW_SIZE];
        if !is_toc {
            if !cd.read_sector(self.curr_lba, &mut sector_buf) {
                error!("CDIC process_sector: read_sector({}) failed!", self.curr_lba);
                return;
            }

            // Detect if dealing with byteswapped loose-bin / CHD image
            if sector_buf[0] == 0xFF && sector_buf[1] == 0x00 {
                info!("CDIC: Byteswapping detected on LBA={}", self.curr_lba);
                self.cd_byteswap = true;
            }

            if self.cd_byteswap {
                for i in (0..CD_SECTOR_RAW_SIZE).step_by(2) {
                    sector_buf.swap(i, i + 1);
                }
            }

            // Validate sector and attempt descrambling if invalid
            if !self.is_valid_sector(&sector_buf) {
                let mut descramble_buf = sector_buf;
                self.descramble_sector(&mut descramble_buf);
                if self.is_valid_sector(&descramble_buf) {
                    sector_buf = descramble_buf;
                }
            }

            // Mode 2 sector filtering
            if sector_buf[15] == 2 && self.disc_mode == 2 {
                let is_audio_sel = self.is_mode2_audio_selected(&sector_buf);
                let is_sector_sel = self.is_mode2_sector_selected(&sector_buf);

                if !is_sector_sel && !is_audio_sel {
                    if self.disc_command == 0 {
                        self.cancel_disc_read();
                    }
                    return;
                }

                if is_audio_sel {
                    let coding = sector_buf[23];
                    let is_stereo = (coding & 0x03) == 0x01;
                    let is_8bit = (coding & 0x30) == 0x10;
                    let sample_rate = if (coding & 0x0C) == 0x04 { 18900 } else { 37800 };

                    self.audio_sector_counter = get_sector_count_for_coding(coding);

                    self.audio.decode_sector(&sector_buf[24..24 + 2304], is_stereo, is_8bit, sample_rate);
                }

                if !is_sector_sel && !(self.command == 0x2A && is_audio_sel) {
                    if self.disc_command == 0 {
                        self.cancel_disc_read();
                    }
                    return;
                }
            }
        } else {
            let _ = cd.read_sector(0, &mut sector_buf);
        }

        // Q subcode calculation
        let real_lba = self.curr_lba + 150;
        let mins = (real_lba / (60 * 75)) as u8;
        let secs = ((real_lba / 75) % 60) as u8;
        let frac = (real_lba % 75) as u8;
        let mins_bcd = ((mins / 10) << 4) | (mins % 10);
        let secs_bcd = ((secs / 10) << 4) | (secs % 10);
        let frac_bcd = ((frac / 10) << 4) | (frac % 10);

        let mut subcode = if is_toc {
            let total_sectors = cd.total_sectors;
            let total_mins = (total_sectors / (60 * 75)) as u8;
            let total_secs = ((total_sectors / 75) % 60) as u8;
            let total_frac = (total_sectors % 75) as u8;
            let tot_mins_bcd = ((total_mins / 10) << 4) | (total_mins % 10);
            let tot_secs_bcd = ((total_secs / 10) << 4) | (total_secs % 10);
            let tot_frac_bcd = ((total_frac / 10) << 4) | (total_frac % 10);

            let first_track = cd.tracks.first().map(|t| t.number).unwrap_or(1);
            let last_track = cd.tracks.last().map(|t| t.number).unwrap_or(1);
            let first_bcd = ((first_track / 10) << 4) | (first_track % 10);
            let last_bcd = ((last_track / 10) << 4) | (last_track % 10);

            let mut toc_packets: Vec<[u8; 5]> = Vec::with_capacity(3 + cd.tracks.len());
            // Packet A0: First track = 1, Disc type = 0x10 (CD-i) or 0x20 (CD-ROM XA / Video CD)
            toc_packets.push([0x41, 0xA0, first_bcd, cd.disc_type, 0x00]);
            // Packet A1: Last track
            toc_packets.push([0x41, 0xA1, last_bcd, 0x00, 0x00]);
            // Packet A2: Lead-out
            toc_packets.push([0x41, 0xA2, tot_mins_bcd, tot_secs_bcd, tot_frac_bcd]);

            // Track start packets: [Control, Point, PMIN, PSEC, PFRAC]
            for t in &cd.tracks {
                let start_abs = t.start_lba + 150;
                let tm = (start_abs / (60 * 75)) as u8;
                let ts = ((start_abs / 75) % 60) as u8;
                let tf = (start_abs % 75) as u8;
                let tm_bcd = ((tm / 10) << 4) | (tm % 10);
                let ts_bcd = ((ts / 10) << 4) | (ts % 10);
                let tf_bcd = ((tf / 10) << 4) | (tf % 10);
                let num_bcd = ((t.number / 10) << 4) | (t.number % 10);
                toc_packets.push([t.control, num_bcd, tm_bcd, ts_bcd, tf_bcd]);
            }

            let pkt_idx = ((self.curr_lba as usize) / 3) % toc_packets.len();
            let toc_data = toc_packets[pkt_idx];

            [
                toc_data[0], // [0] Control / ADR (0x41 or 0x01)
                0x00,        // [1] Track (0x00 in lead-in)
                toc_data[1], // [2] Index / Point (0xA0, 0xA1, 0xA2, 0x01, 0x02...)
                0xA0,        // [3] MINS
                secs_bcd,    // [4] SECS
                frac_bcd,    // [5] FRAC
                0x00,        // [6] Zero
                toc_data[2], // [7] PMIN
                toc_data[3], // [8] PSEC
                toc_data[4], // [9] PFRAC
                0xFF,        // [10] CRC0 (init)
                0xFF,        // [11] CRC1 (init)
            ]
        } else {
            let (cur_track_num, cur_track_ctrl, rel_lba) = if let Some(t) = cd.tracks.iter().find(|t| self.curr_lba >= t.start_lba && self.curr_lba < t.start_lba + t.length_sectors) {
                (t.number, t.control, self.curr_lba - t.start_lba)
            } else {
                (1, 0x41, self.curr_lba)
            };

            let rel_msf = rel_lba;
            let rel_m = (rel_msf / (60 * 75)) as u8;
            let rel_s = ((rel_msf / 75) % 60) as u8;
            let rel_f = (rel_msf % 75) as u8;
            let rel_m_bcd = ((rel_m / 10) << 4) | (rel_m % 10);
            let rel_s_bcd = ((rel_s / 10) << 4) | (rel_s % 10);
            let rel_f_bcd = ((rel_f / 10) << 4) | (rel_f % 10);
            let track_bcd = ((cur_track_num / 10) << 4) | (cur_track_num % 10);

            [
                cur_track_ctrl, // Control
                track_bcd,      // Track
                0x01,           // Index
                rel_m_bcd, rel_s_bcd, rel_f_bcd, // Track relative MSF
                0x00,
                mins_bcd, secs_bcd, frac_bcd,    // Absolute disc MSF
                0xFF, 0xFF // CRC (init)
            ]
        };

        let mut crc_accum: u16 = 0;
        for i in 0..12 {
            crc_accum = ((crc_accum << 8) | (subcode[i] as u16)) ^ CRC_CCITT_TABLE[(crc_accum >> 8) as usize];
        }
        subcode[10] = (crc_accum >> 8) as u8;
        subcode[11] = (crc_accum & 0xFF) as u8;

        // Store into CDIC RAM
        self.data_buffer ^= 0x0001;
        self.data_buffer &= !0x0004;

        let base_offset = ((self.data_buffer & 0x0005) as usize) * 0x0A00;
        let mut dev_idx = base_offset;

        let is_audio_sel = !is_toc && sector_buf[15] == 2 && self.is_mode2_audio_selected(&sector_buf);

        // Copy sector header + subheader 1 (bytes 12..20)
        for i in 12..20 {
            if dev_idx < CDIC_RAM_SIZE {
                self.ram[dev_idx] = sector_buf[i];
                dev_idx += 1;
            }
        }

        // If Mode 2 audio selected with command 0x2A: route to ADPCM Sound Map buffer (+0x1400 words = +0x2800 bytes)
        if self.command == 0x2A && is_audio_sel {
            self.data_buffer |= 0x0004;
            dev_idx += 0x2800;
        }

        // Copy subheader 2 + data payload (bytes 20..2352)
        for i in 20..CD_SECTOR_RAW_SIZE {
            if dev_idx < CDIC_RAM_SIZE {
                self.ram[dev_idx] = sector_buf[i];
                dev_idx += 1;
            }
        }

        // Copy Q subcode (12 words)
        for &b in &subcode {
            if dev_idx + 1 < CDIC_RAM_SIZE {
                self.ram[dev_idx] = 0;
                self.ram[dev_idx + 1] = b;
                dev_idx += 2;
            }
        }

        // Set XBUF ready & Data Buffer ready
        self.x_buffer |= 0x8000;
        self.data_buffer |= 0x4000;
        self.update_interrupt();

        info!(
            "CDIC Sector ready: LBA={}, submode=0x{:02X}, XBUF=0x{:04X}, IRQ4 asserted (audio={})",
            self.curr_lba, sector_buf[22], self.x_buffer, is_audio_sel
        );

        if self.disc_command == 0 || self.command == 0x23 || self.command == 0x24 {
            self.cancel_disc_read();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_channels_above_15_are_never_selected() {
        let mut c = Cdic::new();
        c.audio_channel = 0x0002;
        let mut buf = vec![0u8; 2352];
        buf[22] = 0x24;
        buf[23] = 0x05;
        buf[21] = 1;
        assert!(c.is_mode2_audio_selected(&buf));
        buf[21] = 17;
        assert!(!c.is_mode2_audio_selected(&buf));
    }

    fn tick(c: &mut Cdic, n: u32) {
        let mut cd = None;
        for _ in 0..n {
            c.step_cycles(1000, 1000, &mut cd);
        }
    }

    #[test]
    fn a_stopped_buffer_sets_abuf_when_it_has_played_out() {
        let mut c = Cdic::new();
        c.ram[0x2800 + 11] = 0x05;
        c.write_reg(0x3FA, 0x2800);
        assert_eq!(c.read_reg(0x3F4) & 0x8000, 0);
        c.write_reg(0x3FA, 0x0000);
        tick(&mut c, 7);
        assert_eq!(c.read_reg(0x3F4) & 0x8000, 0);
        tick(&mut c, 1);
        assert_eq!(c.read_reg(0x3F4) & 0x8000, 0x8000);
        assert_eq!(c.buf_addr, 0xFFFF);
        assert!(!c.map_on);
    }

    #[test]
    fn the_map_starts_at_the_write_and_decodes_its_sound_groups_as_they_play() {
        let mut c = Cdic::new();
        c.ram[0x2800 + 11] = 0x05;
        tick(&mut c, 1);
        c.write_reg(0x3FA, 0x2800);
        assert_eq!((c.buf_addr, c.buf_group, c.audio.total_sectors_decoded), (0x2800, 1, 1));
        tick(&mut c, 4);
        assert_eq!(c.buf_group, 10);
        tick(&mut c, 3);
        assert_eq!(c.buf_group, 16);
    }

    #[test]
    fn abuf_comes_when_the_first_buffer_has_played_so_the_second_still_plays_after_an_ff_mark() {
        let mut c = Cdic::new();
        c.audio_channel = 0;
        c.ram[0x2800 + 11] = 0x05;
        c.ram[0x3200 + 11] = 0x05;
        tick(&mut c, 1);
        c.write_reg(0x3FA, 0x2800);
        tick(&mut c, 7);
        assert_eq!(c.audio_buffer & 0x8000, 0);
        tick(&mut c, 1);
        assert_eq!(c.read_reg(0x3F4) & 0x8000, 0x8000);
        assert_eq!(c.audio.total_sectors_decoded, 2);
        c.ram[0x2800 + 11] = 0xFF;
        c.ram[0x3200 + 11] = 0xFF;
        tick(&mut c, 8);
        assert_eq!(c.read_reg(0x3F4) & 0x8000, 0x8000);
        assert_eq!(c.audio.total_sectors_decoded, 2);
        assert_eq!(c.read_reg(0x3FA), 0x2000);
    }

    #[test]
    fn audctl_start_without_channel_ends_an_empty_map_at_once_with_the_finished_flag() {
        let mut c = Cdic::new();
        c.audio_channel = 0;
        c.ram[0x2800 + 11] = 0xFF;
        c.write_reg(0x3FA, 0x0800);
        assert_eq!(c.read_reg(0x3FA), 0x0001);
        assert_eq!(c.read_reg(0x3FA), 0x0000);
        assert!(!c.map_on);
    }

    #[test]
    fn audctl_start_over_a_stopped_buffer_in_flight_reads_as_playing_and_waits() {
        let mut c = Cdic::new();
        c.audio_channel = 0;
        c.ram[0x2800 + 11] = 0x05;
        c.write_reg(0x3FA, 0x2800);
        tick(&mut c, 1);
        c.write_reg(0x3FA, 0x0000);
        c.ram[0x2800 + 11] = 0xFF;
        c.write_reg(0x3FA, 0x0800);
        assert_eq!(c.read_reg(0x3FA), 0x0800);
        c.write_reg(0x3FA, 0x0000);
        assert_eq!(c.read_reg(0x3FA), 0x0000);
    }

    #[test]
    fn audctl_reads_playback_in_bit_11_while_a_map_plays() {
        let mut c = Cdic::new();
        c.audio_channel = 0;
        c.ram[0x2800 + 11] = 0x05;
        c.write_reg(0x3FA, 0x2800);
        assert_eq!(c.read_reg(0x3FA), 0x2800);
        tick(&mut c, 1);
        assert_eq!(c.read_reg(0x3FA), 0x2800);
    }

    #[test]
    fn audctl_bit_13_gates_the_buffer_interrupt() {
        let mut c = Cdic::new();
        c.audio_buffer = 0x8000;
        c.write_reg(0x3FA, 0x0000);
        assert!(!c.irq_pending);
        c.write_reg(0x3FA, 0x2000);
        assert!(c.irq_pending);
    }

    #[test]
    fn audctl_with_an_audio_channel_keeps_the_alternating_idle_bit_0() {
        let mut c = Cdic::new();
        c.audio_channel = 2;
        c.ram[0x2800 + 11] = 0xFF;
        c.write_reg(0x3FA, 0x0800);
        assert!(!c.map_on);
        assert_eq!(c.read_reg(0x3FA), 0x0801);
        assert_eq!(c.read_reg(0x3FA), 0x0800);
    }
}

use std::fs;
use std::path::Path;
use std::io::{self, Write};
use log::trace;
use m68k::AddressBus;

use crate::cdic::{Cdic, CDIC_RAM_SIZE};
use crate::cdrom::CdRom;
use crate::dma::SccDma;
use crate::dvc::Dvc;
use crate::mcd212::Mcd212;
use crate::nvram::Timekeeper;
use crate::scc_timer::SccTimer;
use crate::slave::SlaveHle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedIn4Owner {
    Idle,
    Cdic,
    Vmpeg,
}

pub struct CdiBus {
    pub rom: Box<[u8; 512 * 1024]>,
    pub sys_ram: Box<[u8; 512 * 1024]>,
    pub dvc: Dvc,

    pub cdic: Cdic,
    pub dma: SccDma,
    pub cdrom: Option<CdRom>,

    pub mcd212: Mcd212,
    pub nvram: Timekeeper,
    pub slave: SlaveHle,

    pub shared_in4_owner: SharedIn4Owner,

    pub scc_timer: SccTimer,
    pub scc_uart_cmd: u8,
    pub scc_uart_tx_irq: bool,
    pub current_pc: u32,
    pub trace_setup: bool,

    pub wait_cycles: u32,
    pub current_line_cycle: u32,
    pub emulate_wait_states: bool,
    pub watch_addr1: u32,
    pub watch_addr2: u32,
    pub watch_enabled: bool,
    pub trace_fmv: bool,
    pub current_frame: u64,
}

impl CdiBus {
    pub fn new() -> Self {
        Self {
            rom: vec![0u8; 512 * 1024].into_boxed_slice().try_into().unwrap(),
            sys_ram: vec![0u8; 512 * 1024].into_boxed_slice().try_into().unwrap(),
            dvc: Dvc::new(),

            cdic: Cdic::new(),
            dma: SccDma::new(),
            cdrom: None,

            mcd212: Mcd212::new(),
            nvram: Timekeeper::new(),
            slave: SlaveHle::new(),
            shared_in4_owner: SharedIn4Owner::Idle,

            scc_timer: SccTimer::new(),
            scc_uart_cmd: 0,
            scc_uart_tx_irq: false,
            current_pc: 0,
            trace_setup: false,

            wait_cycles: 0,
            current_line_cycle: 0,
            emulate_wait_states: true,
            watch_addr1: 0,
            watch_addr2: 0,
            watch_enabled: false,
            trace_fmv: false,
            current_frame: 0,
        }
    }

    pub fn reset_system(&mut self) {
        self.mcd212.reset();
        self.mcd212.plane_a[0..8].copy_from_slice(&self.rom[0..8]);
        self.cdic.reset();
        self.sys_ram.fill(0);
        self.dvc.reset();
        let has_disc = self.cdrom.is_some();
        self.slave.reset();
        self.slave.set_disc_inserted(has_disc);
        self.scc_timer = SccTimer::new();
        self.dma = SccDma::new();
        self.scc_uart_cmd = 0;
        self.scc_uart_tx_irq = false;
        self.shared_in4_owner = SharedIn4Owner::Idle;
        self.current_pc = 0;
        self.trace_setup = false;
        self.wait_cycles = 0;
        self.current_line_cycle = 0;
    }

    /// Player reset from the slave: every device as at power-on, but RAM keeps its contents.
    pub fn player_reset(&mut self) {
        let planes = (self.mcd212.plane_a.clone(), self.mcd212.plane_b.clone());
        let (cdic_ram, sys_ram) = (self.cdic.ram.clone(), self.sys_ram.clone());
        let dvc_ram = (self.dvc.ram1.clone(), self.dvc.ram2.clone());
        self.reset_system();
        (self.mcd212.plane_a, self.mcd212.plane_b) = planes;
        (self.cdic.ram, self.sys_ram) = (cdic_ram, sys_ram);
        (self.dvc.ram1, self.dvc.ram2) = dvc_ram;
        self.mcd212.plane_a[0..8].copy_from_slice(&self.rom[0..8]);
    }

    #[inline(always)]
    pub fn add_ram_wait<const PATH: usize>(&mut self) {
        if self.emulate_wait_states {
            let wait = self.mcd212.ram_dtack_cycles::<PATH>(self.current_line_cycle);
            self.wait_cycles += wait;
            self.current_line_cycle = self.current_line_cycle.wrapping_add(wait);
        }
    }

    #[inline(always)]
    pub fn add_rom_wait(&mut self) {
        if self.emulate_wait_states {
            let wait = self.mcd212.rom_dtack_cycles();
            self.wait_cycles += wait;
            self.current_line_cycle = self.current_line_cycle.wrapping_add(wait);
        }
    }

    #[inline(always)]
    pub fn consume_wait_cycles(&mut self) -> i32 {
        let w = self.wait_cycles as i32;
        self.wait_cycles = 0;
        w
    }

    /// Side-effect-free read of RAM/ROM (no wait states, no counters); `None` for device windows and unmapped space.
    pub fn peek_byte(&self, address: u32) -> Option<u8> {
        if (address & 0xFFFF_0000) == 0x8000_0000 || (address & 0x00FF_0000) == 0x0080_0000 {
            return None;
        }
        let addr = address & 0x00FF_FFFF;
        match addr {
            0x000000..=0x07FFFF => Some(self.mcd212.plane_a[addr as usize]),
            0x200000..=0x27FFFF => Some(self.mcd212.plane_b[(addr - 0x200000) as usize]),
            0x300000..=0x303BFF => Some(self.cdic.ram[(addr - 0x300000) as usize]),
            0x400000..=0x47FFFF => Some(self.rom[(addr - 0x400000) as usize]),
            0x500000..=0x57FFFF => Some(self.sys_ram[(addr - 0x500000) as usize]),
            0xD00000..=0xDFFFFF if self.dvc.is_fitted() => Some(self.dvc.read_ram1((addr - 0xD00000) as usize)),
            0xE40000..=0xE7FFFF => self.dvc.rom.as_ref().map(|r| r[(addr - 0xE40000) as usize % r.len()]),
            0xE80000..=0xEFFFFF if self.dvc.is_fitted() && self.dvc.ram2_enabled => {
                Some(self.dvc.read_ram2((addr - 0xE80000) as usize))
            }
            _ => None,
        }
    }

    pub fn peek_word(&self, address: u32) -> Option<u16> {
        let hi = self.peek_byte(address)? as u16;
        let lo = self.peek_byte(address.wrapping_add(1))? as u16;
        Some((hi << 8) | lo)
    }

    pub fn peek_long(&self, address: u32) -> Option<u32> {
        let hi = self.peek_word(address)? as u32;
        let lo = self.peek_word(address.wrapping_add(2))? as u32;
        Some((hi << 16) | lo)
    }

    pub fn tick_timer(&mut self, cycles: u32) {
        self.scc_timer.tick(cycles);
    }

    pub fn load_rom<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<()> {
        let data = fs::read(path)?;
        let len = data.len().min(self.rom.len());
        self.rom[..len].copy_from_slice(&data[..len]);

        // Copy reset vectors (SSP at 0, PC at 4) to Plane A RAM
        self.mcd212.plane_a[0..8].copy_from_slice(&self.rom[0..8]);
        Ok(())
    }

    pub fn load_dvc_rom<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<()> {
        self.dvc.load_rom(path)
    }

    pub fn load_cdrom<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let cd = CdRom::open(path)?;
        self.cdrom = Some(cd);
        self.slave.set_disc_inserted(true);
        Ok(())
    }

    pub fn change_disc<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let cd = CdRom::open(path)?;
        self.cdrom = Some(cd);
        self.cdic.curr_lba = 0;
        self.cdic.command = 0;
        self.cdic.disc_command = 0;
        self.slave.notify_disc_swapped();
        Ok(())
    }

    pub fn tick_cdic(&mut self, cycles: u32) {
        self.cdic.step_cycles(cycles, &mut self.cdrom);
    }

    #[inline]
    pub fn dvc_dma_requested(&self) -> bool {
        self.dvc.dma_requested
            || (self.dvc.get_fmv_word(crate::dvc::FMV_SYSCMD) & 0x8000) != 0
            || (self.dvc.get_fma_word(crate::dvc::FMA_CMD) & 0x8000) != 0
    }

    pub fn arbitrate_shared_in4(&mut self) {
        match self.shared_in4_owner {
            SharedIn4Owner::Cdic => {
                if !self.cdic.irq_pending {
                    self.shared_in4_owner = SharedIn4Owner::Idle;
                }
            }
            SharedIn4Owner::Vmpeg => {
                if !self.dvc.irq_pending() {
                    self.shared_in4_owner = SharedIn4Owner::Idle;
                }
            }
            SharedIn4Owner::Idle => {}
        }

        if self.shared_in4_owner == SharedIn4Owner::Idle {
            if self.dvc.irq_pending() {
                self.shared_in4_owner = SharedIn4Owner::Vmpeg;
            } else if self.cdic.irq_pending {
                self.shared_in4_owner = SharedIn4Owner::Cdic;
            }
        }
    }

    #[inline]
    pub fn in4_pending(&mut self) -> bool {
        self.arbitrate_shared_in4();
        self.shared_in4_owner != SharedIn4Owner::Idle
    }

    pub fn tick_dvc(&mut self, cycles: u32) {
        if self.dvc.is_fitted() {
            self.dvc.tick(cycles);
            if self.dvc_dma_requested() {
                self.perform_dvc_dma();
            }
        }
    }

    pub fn tick_slave(&mut self, cycles: u32) {
        self.slave.tick(cycles);
    }

    pub fn set_video_mode(&mut self, mode: crate::mcd212::VideoMode) {
        self.mcd212.video_mode = mode;
        self.slave.video_mode = mode;
        self.mcd212.framebuffer.fill(0xFF00_0000);
    }

    pub fn tick_scanline(&mut self, line: usize) -> bool {
        let vblank_lines = self.mcd212.video_mode.vblank_lines();
        let total_lines = self.mcd212.video_mode.total_scanlines();
        if line >= vblank_lines && line < total_lines {
            let active_line = line - vblank_lines;
            if self.dvc.is_fitted() {
                self.dvc.get_scanline(active_line, &mut self.mcd212.external_video_line, &mut self.mcd212.external_video_present);
            }
        }
        if line == 0 {
            self.current_frame += 1;
        }
        let frame_done = self.mcd212.tick_scanline(line);
        let mcd_int = (self.mcd212.csrr[1] & (crate::mcd212::CSR2R_IT1 | crate::mcd212::CSR2R_IT2)) != 0;
        self.scc_timer.int1_w(mcd_int);
        if line == 0 && self.dvc.is_fitted() {
            self.dvc.vsync_hz = if self.mcd212.video_mode == crate::mcd212::VideoMode::Pal { 50.0 } else { 60.0 };
            self.dvc.on_vsync();
        }
        if self.dvc.is_fitted() && !self.dvc.audio_out.is_empty() {
            if let Ok(mut q) = self.cdic.audio.sample_queue.lock() {
                self.cdic.audio.source_rate.store(self.dvc.audio_sample_rate, std::sync::atomic::Ordering::Relaxed);
                let max_samples = self.dvc.audio_sample_rate as usize;
                let add_len = self.dvc.audio_out.len();
                let current_len = q.len();
                if current_len + add_len > max_samples {
                    let overflow = (current_len + add_len) - max_samples;
                    let to_drain = overflow.min(current_len);
                    q.drain(0..to_drain);
                }
                q.extend(self.dvc.audio_out.drain(..));
            }
        }
        frame_done
    }

    #[inline(always)]
    pub fn render_scanline(&mut self, line: usize) {
        self.mcd212.render_line(line);
    }

    pub fn perform_dvc_dma(&mut self) {
        let fmv_req = (self.dvc.get_fmv_word(crate::dvc::FMV_SYSCMD) & 0x8000) != 0;
        let fma_req = (self.dvc.get_fma_word(crate::dvc::FMA_CMD) & 0x8000) != 0;
        if !self.dvc.dma_requested && !fmv_req && !fma_req {
            return;
        }

        let count = self.dma.channels[1].transfer_counter as u32;
        if count == 0 {
            return;
        }

        if self.dvc.dma_requested {
            // transfer_for_fma was already correctly set by handle_fmv_write or handle_fma_write
        } else if fmv_req {
            self.dvc.transfer_for_fma = false;
        } else if fma_req {
            self.dvc.transfer_for_fma = true;
        }

        self.dvc.dma_requested = false;
        let start = self.dma.channels[1].memory_address_counter;
        let b16: Vec<u8> = (0..16.min(count * 2)).map(|off| self.read_byte(start.wrapping_add(off))).collect();
        trace!("PERFORM DVC DMA at frame {}: start=0x{:08X}, count={} (for_fma={}) | bytes[0..16]={:02X?}", 
            self.current_frame, start, count, self.dvc.transfer_for_fma, b16);

        let for_fma = self.dvc.transfer_for_fma;
        for i in 0..count {
            let addr = start.wrapping_add(i * 2);
            let hi = self.read_byte(addr);
            let lo = self.read_byte(addr.wrapping_add(1));
            if for_fma {
                self.dvc.push_fma_stream_byte(hi);
                self.dvc.push_fma_stream_byte(lo);
            } else {
                self.dvc.push_stream_byte(hi);
                self.dvc.push_stream_byte(lo);
            }
        }
        self.dvc.process_stream();
        trace!("After DVC DMA: demux_video_len={}, demux_audio_len={}, fma_audio_len={}, has_decoder={}, decoded_count={}",
            self.dvc.demux.video.len(), self.dvc.demux.audio.len(), self.dvc.fma_demux.audio.len(), self.dvc.video_decoder.is_some(), self.dvc.frames_decoded_count);
        self.dma.channels[1].memory_address_counter = start.wrapping_add(count * 2);
        self.dma.channels[1].transfer_counter = 0;
        self.dma.channels[1].channel_status |= 0x80;
        if self.dvc.transfer_for_fma {
            let cmd = self.dvc.get_fma_word(crate::dvc::FMA_CMD) & !0x8000;
            self.dvc.set_fma_word(crate::dvc::FMA_CMD, cmd);
        } else {
            let cmd = self.dvc.get_fmv_word(crate::dvc::FMV_SYSCMD) & !0x8000;
            self.dvc.set_fmv_word(crate::dvc::FMV_SYSCMD, cmd);
        }
    }

    pub fn perform_dma(&mut self) {
        let ch = &mut self.dma.channels[0];
        let start = ch.memory_address_counter as usize;
        let count = ch.transfer_counter as usize;
        let mut device_index = (self.cdic.dma_control & 0x3FFF) as usize;
        let is_to_mem = (ch.operation_control & 0x80) != 0;

        trace!(
            "DMA Ch0 Executing: {} words ({} bytes) {} 0x{:08X} dev CDIC 0x{:04X} at PC=0x{:06X}",
            count, count * 2, if is_to_mem { "to mem" } else { "from mem" }, start, device_index, self.current_pc
        );

        for i in 0..count {
            let mem_addr = start + i * 2;
            let dev_addr = device_index % CDIC_RAM_SIZE;

            if is_to_mem {
                let b0 = self.cdic.ram[dev_addr];
                let b1 = self.cdic.ram[(dev_addr + 1) % CDIC_RAM_SIZE];
                self.write_byte(mem_addr as u32, b0);
                self.write_byte((mem_addr + 1) as u32, b1);
            } else {
                let b0 = self.read_byte(mem_addr as u32);
                let b1 = self.read_byte((mem_addr + 1) as u32);
                self.cdic.ram[dev_addr] = b0;
                self.cdic.ram[(dev_addr + 1) % CDIC_RAM_SIZE] = b1;
            }

            device_index = (device_index + 2) % CDIC_RAM_SIZE;
        }

        self.dma.channels[0].memory_address_counter = (start + count * 2) as u32;
        self.dma.channels[0].transfer_counter = 0;
        self.dma.channels[0].channel_status |= 0x80; // COC (Channel Operation Complete)
        self.cdic.dma_triggered = false;
    }
}

impl AddressBus for CdiBus {
    fn read_byte(&mut self, address: u32) -> u8 {
        // SCC68070 On-Chip Registers (0x80000000..0x8000FFFF or 0x0080XXXX)
        if (address & 0xFFFF_0000) == 0x8000_0000 || (address & 0x00FF_0000) == 0x0080_0000 {
            let reg = address & 0xFFFF;
            return match reg {
                // UART Status Register: Bit 2 = Tx Ready / Empty (0x04)
                0x2011 | 0x2013 => 0x04,
                0x2015 => self.scc_uart_cmd,
                0x2017 | 0x2019 => 0x00,

                // LIR: Line Interrupt Register (INT1/INT2)
                0x1001 => self.scc_timer.read_byte(reg),

                // Timer registers (TSR, TCR, RR, T0..T2, PICR1, PICR2)
                0x2020..=0x2029 | 0x2045 | 0x2047 => self.scc_timer.read_byte(reg),

                // DMA channel registers
                0x4000..=0x406F => {
                    let offset = (reg - 0x4000) & !1;
                    let word = self.dma.read_reg(offset);
                    if (reg & 1) == 0 { (word >> 8) as u8 } else { word as u8 }
                }

                // MMU Status/Control
                0x8000..=0x80FF => 0x00,

                _ => 0x00,
            };
        }

        let addr = address & 0x00FF_FFFF;

        match addr {
            // Plane A RAM (512 KB)
            0x000000..=0x07FFFF => {
                self.add_ram_wait::<0>();
                self.mcd212.plane_a[addr as usize]
            }

            // Plane B RAM (512 KB)
            0x200000..=0x27FFFF => {
                self.add_ram_wait::<1>();
                self.mcd212.plane_b[(addr - 0x200000) as usize]
            }

            // CDIC RAM Buffer (15 KB)
            0x300000..=0x303BFF => self.cdic.ram[(addr - 0x300000) as usize],

            // CDIC Registers
            0x303C00..=0x303FFF => {
                let offset = (addr - 0x303C00) & !1;
                let word = self.cdic.read_reg(offset);
                if (addr & 1) == 0 { (word >> 8) as u8 } else { word as u8 }
            }

            // Slave MCU (HLE)
            0x310000..=0x317FFF => {
                let word = self.slave.read(addr - 0x310000);
                if (addr & 1) == 0 { (word >> 8) as u8 } else { word as u8 }
            }

            // Timekeeper NVRAM (MK48T08)
            0x320000..=0x323FFF => {
                let offset = (addr - 0x320000) / 2;
                self.nvram.read(offset)
            }

            // System ROM (CD-RTOS, 512 KB)
            0x400000..=0x47FFFF => {
                self.add_rom_wait();
                if addr == 0x405E48 {
                    self.trace_setup = true;
                }
                self.rom[(addr - 0x400000) as usize]
            }

            // MCD212 Display Controller Registers
            0x4FFFE0..=0x4FFFFF => {
                let offset = addr - 0x4FFFE0;
                let reg_base = offset & !1;
                let word = self.mcd212.read_reg(reg_base);
                let mcd_int = (self.mcd212.csrr[1] & (crate::mcd212::CSR2R_IT1 | crate::mcd212::CSR2R_IT2)) != 0;
                self.scc_timer.int1_w(mcd_int);
                if (addr & 1) == 0 { (word >> 8) as u8 } else { word as u8 }
            }

            // System RAM (512 KB)
            0x500000..=0x57FFFF => self.sys_ram[(addr - 0x500000) as usize],

            // DVC RAM Block 1 (1 MB)
            0xD00000..=0xDFFFFF => {
                if self.dvc.is_fitted() {
                    self.dvc.read_ram1((addr - 0xD00000) as usize)
                } else {
                    0x00
                }
            }

            // DVC Registers Window (0xE00000..0xE3FFFF)
            0xE00000..=0xE3FFFF => {
                if self.dvc.is_fitted() {
                    self.dvc.read_byte(addr)
                } else {
                    0xFF
                }
            }

            // DVC ROM (vmpega.rom, 256 KB)
            0xE40000..=0xE7FFFF => {
                if self.dvc.is_fitted() {
                    self.dvc.read_rom((addr - 0xE40000) as usize)
                } else {
                    0xFF
                }
            }

            // DVC Picture DRAM (512 KB, gated)
            0xE80000..=0xEFFFFF => {
                if self.dvc.is_fitted() && self.dvc.ram2_enabled {
                    self.dvc.read_ram2((addr - 0xE80000) as usize)
                } else {
                    0xFF
                }
            }

            _ => {
                trace!("Unmapped read_byte at 0x{:06X}", addr);
                0xFF
            }
        }
    }

    fn read_word(&mut self, address: u32) -> u16 {
        // SCC68070 On-Chip Registers (0x80000000..0x8000FFFF or 0x0080XXXX)
        if (address & 0xFFFF_0000) == 0x8000_0000 || (address & 0x00FF_0000) == 0x0080_0000 {
            let reg = address & 0xFFFF;
            if reg >= 0x2020 && reg <= 0x2029 {
                return self.scc_timer.read_word(reg);
            }
            if (0x4000..=0x406F).contains(&reg) {
                let offset = (reg - 0x4000) & !1;
                return self.dma.read_reg(offset);
            }
            let hi = self.read_byte(address) as u16;
            let lo = self.read_byte(address.wrapping_add(1)) as u16;
            return (hi << 8) | lo;
        }

        let addr = address & 0x00FF_FFFF;

        match addr {
            0x000000..=0x07FFFE => {
                self.add_ram_wait::<0>();
                let idx = addr as usize;
                ((self.mcd212.plane_a[idx] as u16) << 8) | (self.mcd212.plane_a[idx + 1] as u16)
            }
            0x200000..=0x27FFFE => {
                self.add_ram_wait::<1>();
                let idx = (addr - 0x200000) as usize;
                ((self.mcd212.plane_b[idx] as u16) << 8) | (self.mcd212.plane_b[idx + 1] as u16)
            }
            0x400000..=0x47FFFE => {
                self.add_rom_wait();
                let idx = (addr - 0x400000) as usize;
                ((self.rom[idx] as u16) << 8) | (self.rom[idx + 1] as u16)
            }
            0x500000..=0x57FFFE => {
                let idx = (addr - 0x500000) as usize;
                ((self.sys_ram[idx] as u16) << 8) | (self.sys_ram[idx + 1] as u16)
            }
            0x300000..=0x303BFE => {
                let idx = (addr - 0x300000) as usize;
                ((self.cdic.ram[idx] as u16) << 8) | (self.cdic.ram[idx + 1] as u16)
            }
            0x303C00..=0x303FFF => {
                let offset = (addr - 0x303C00) & !1;
                let val = self.cdic.read_reg(offset);
                if self.trace_fmv {
                    println!("[CDIC READ WORD] offset=0x{:04X} -> 0x{:04X} at PC=0x{:08X}", offset, val, self.current_pc);
                }
                val
            }
            0x310000..=0x317FFF => {
                self.slave.read(addr - 0x310000)
            }
            0x4FFFE0..=0x4FFFFF => {
                let offset = (addr - 0x4FFFE0) & !1;
                let word = self.mcd212.read_reg(offset);
                let mcd_int = (self.mcd212.csrr[1] & (crate::mcd212::CSR2R_IT1 | crate::mcd212::CSR2R_IT2)) != 0;
                self.scc_timer.int1_w(mcd_int);
                word
            }
            0xD00000..=0xDFFFFE => {
                if self.dvc.is_fitted() {
                    let idx = (addr - 0xD00000) as usize;
                    ((self.dvc.read_ram1(idx) as u16) << 8) | (self.dvc.read_ram1(idx + 1) as u16)
                } else {
                    0xFFFF
                }
            }
            0xE00000..=0xE3FFFE => {
                if self.dvc.is_fitted() {
                    let word_offset = (addr - 0xE00000) >> 1;
                    let val = self.dvc.read_reg(word_offset);
                    if self.trace_fmv {
                        println!("[DVC READ WORD] reg=0x{:04X} -> 0x{:04X} at PC=0x{:08X}", word_offset & 0x7FFF, val, self.current_pc);
                    }
                    val
                } else {
                    0xFFFF
                }
            }
            0xE40000..=0xE7FFFE => {
                if self.dvc.is_fitted() {
                    let idx = (addr - 0xE40000) as usize;
                    ((self.dvc.read_rom(idx) as u16) << 8) | (self.dvc.read_rom(idx + 1) as u16)
                } else {
                    0xFFFF
                }
            }
            0xE80000..=0xEFFFFE => {
                if self.dvc.is_fitted() && self.dvc.ram2_enabled {
                    let idx = (addr - 0xE80000) as usize;
                    ((self.dvc.read_ram2(idx) as u16) << 8) | (self.dvc.read_ram2(idx + 1) as u16)
                } else {
                    0xFFFF
                }
            }
            _ => {
                let hi = self.read_byte(address) as u16;
                let lo = self.read_byte(address.wrapping_add(1)) as u16;
                (hi << 8) | lo
            }
        }
    }

    fn read_long(&mut self, address: u32) -> u32 {
        let hi = self.read_word(address) as u32;
        let lo = self.read_word(address.wrapping_add(2)) as u32;
        (hi << 16) | lo
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        // SCC68070 On-Chip Registers
        if (address & 0xFFFF_0000) == 0x8000_0000 || (address & 0x00FF_0000) == 0x0080_0000 {
            let reg = address & 0xFFFF;
            match reg {
                // UART Transmit Data
                0x2017 | 0x2019 => {
                    let ch = value as char;
                    if ch.is_ascii() && !ch.is_control() || ch == '\n' || ch == '\r' || ch == '\t' {
                        print!("{}", ch);
                    } else {
                        print!("[{:02X}]", value);
                    }
                    let _ = io::stdout().flush();
                    if ((self.scc_uart_cmd >> 2) & 3) == 1 {
                        self.scc_uart_tx_irq = true;
                    }
                }
                0x2015 => {
                    self.scc_uart_cmd = value;
                    let tx_cmd = (value >> 2) & 3;
                    if tx_cmd == 1 {
                        self.scc_uart_tx_irq = true;
                    } else if tx_cmd == 2 || tx_cmd == 3 {
                        self.scc_uart_tx_irq = false;
                    }
                }
                0x1001 => self.scc_timer.write_byte(reg, value),
                0x2020..=0x2029 | 0x2045 | 0x2047 => self.scc_timer.write_byte(reg, value),
                0x4000..=0x406F => {
                    let offset = (reg - 0x4000) & !1;
                    let cur = self.dma.read_reg(offset);
                    let new_val = if (reg & 1) == 0 {
                        (cur & 0x00FF) | ((value as u16) << 8)
                    } else {
                        (cur & 0xFF00) | (value as u16)
                    };
                    trace!("DMA BYTE WRITE: reg=0x{:04X}, offset=0x{:02X}, new_val=0x{:04X} at PC=0x{:06X}", reg, offset, new_val, self.current_pc);
                    self.dma.write_reg(offset, new_val);
                    if offset >= 0x40 && self.dvc_dma_requested() {
                        self.perform_dvc_dma();
                    }
                }
                _ => {}
            }
            return;
        }

        let addr = address & 0x00FF_FFFF;

        if (addr == self.watch_addr1 || addr == self.watch_addr2) && self.watch_enabled {
            let old = self.read_byte(addr);
            println!("[WATCH WRITE BYTE] addr=0x{:08X}, old=0x{:02X}, new=0x{:02X} at PC=0x{:08X} line={} cyc={}", addr, old, value, self.current_pc, self.mcd212.scanline, self.current_line_cycle);
        }

        match addr {
            0x000000..=0x07FFFF => {
                self.add_ram_wait::<0>();
                self.mcd212.plane_a[addr as usize] = value;
            }

            // Plane B RAM
            0x200000..=0x27FFFF => {
                self.add_ram_wait::<1>();
                self.mcd212.plane_b[(addr - 0x200000) as usize] = value;
            }

            // CDIC RAM
            0x300000..=0x303BFF => self.cdic.ram[(addr - 0x300000) as usize] = value,

            // CDIC Registers
            0x303C00..=0x303FFF => {
                let offset = (addr - 0x303C00) & !1;
                let cur = self.cdic.read_reg(offset);
                let new_val = if (addr & 1) == 0 {
                    (cur & 0x00FF) | ((value as u16) << 8)
                } else {
                    (cur & 0xFF00) | (value as u16)
                };
                self.cdic.write_reg(offset, new_val);
                if self.cdic.dma_triggered {
                    self.perform_dma();
                }
            }

            // Slave MCU (HLE)
            0x310000..=0x317FFF => {
                self.slave.write(addr - 0x310000, value as u16);
            }

            // Timekeeper NVRAM
            0x320000..=0x323FFF => {
                let offset = (addr - 0x320000) / 2;
                self.nvram.write(offset, value);
            }

            // MCD212 Display Registers
            0x4FFFE0..=0x4FFFFF => {
                let offset = addr - 0x4FFFE0;
                let reg_base = offset & !1;
                let cur = match reg_base & 0x1E {
                    0x00 => self.mcd212.csrw[1],
                    0x10 => self.mcd212.csrw[0],
                    _ => self.mcd212.read_reg(reg_base),
                };
                let new_val = if (addr & 1) == 0 {
                    (cur & 0x00FF) | ((value as u16) << 8)
                } else {
                    (cur & 0xFF00) | (value as u16)
                };
                self.mcd212.write_reg(reg_base, new_val);
                self.slave.video_mode = self.mcd212.video_mode;
            }

            // System RAM (512 KB)
            0x500000..=0x57FFFF => self.sys_ram[(addr - 0x500000) as usize] = value,

            // DVC RAM Block 1 (1 MB)
            0xD00000..=0xDFFFFF => {
                if self.dvc.is_fitted() {
                    self.dvc.write_ram1((addr - 0xD00000) as usize, value);
                }
            }

            // DVC Registers (0xE00000..0xE3FFFF)
            0xE00000..=0xE3FFFF => {
                if self.dvc.is_fitted() {
                    self.dvc.write_byte(addr, value);
                    if self.dvc_dma_requested() {
                        self.perform_dvc_dma();
                    }
                }
            }

            // DVC Picture DRAM (512 KB, gated)
            0xE80000..=0xEFFFFF => {
                if self.dvc.is_fitted() {
                    self.dvc.write_ram2((addr - 0xE80000) as usize, value);
                }
            }

            _ => {
                trace!("Unmapped write_byte at 0x{:06X}: 0x{:02X}", addr, value);
            }
        }
    }

    fn write_word(&mut self, address: u32, value: u16) {
        // SCC68070 On-Chip Registers (0x80000000..0x8000FFFF or 0x0080XXXX)
        if (address & 0xFFFF_0000) == 0x8000_0000 || (address & 0x00FF_0000) == 0x0080_0000 {
            let reg = address & 0xFFFF;
            if reg >= 0x2020 && reg <= 0x2029 {
                self.scc_timer.write_word(reg, value);
                return;
            }
            if (0x4000..=0x406F).contains(&reg) {
                let offset = (reg - 0x4000) & !1;
                trace!("DMA REG WRITE: offset 0x{:02X} = 0x{:04X} at PC=0x{:06X}", offset, value, self.current_pc);
                self.dma.write_reg(offset, value);
                if offset >= 0x40 && self.dvc_dma_requested() {
                    self.perform_dvc_dma();
                }
                return;
            }
            self.write_byte(address, (value >> 8) as u8);
            self.write_byte(address.wrapping_add(1), value as u8);
            return;
        }

        let addr = address & 0x00FF_FFFF;
        if ((addr <= self.watch_addr1 && self.watch_addr1 < addr + 2) || (addr <= self.watch_addr2 && self.watch_addr2 < addr + 2)) && self.watch_enabled {
            println!("[WATCH WRITE WORD F{}] addr=0x{:08X}, val=0x{:04X} at PC=0x{:08X} line={} cyc={}", self.current_frame, addr, value, self.current_pc, self.mcd212.scanline, self.current_line_cycle);
        }
        match addr {
            0x000000..=0x07FFFE => {
                self.add_ram_wait::<0>();
                let idx = addr as usize;
                self.mcd212.plane_a[idx] = (value >> 8) as u8;
                self.mcd212.plane_a[idx + 1] = value as u8;
                return;
            }
            0x200000..=0x27FFFE => {
                self.add_ram_wait::<1>();
                let idx = (addr - 0x200000) as usize;
                self.mcd212.plane_b[idx] = (value >> 8) as u8;
                self.mcd212.plane_b[idx + 1] = value as u8;
                return;
            }
            0x500000..=0x57FFFE => {
                let idx = (addr - 0x500000) as usize;
                self.sys_ram[idx] = (value >> 8) as u8;
                self.sys_ram[idx + 1] = value as u8;
                return;
            }
            0x300000..=0x303BFE => {
                let idx = (addr - 0x300000) as usize;
                self.cdic.ram[idx] = (value >> 8) as u8;
                self.cdic.ram[idx + 1] = value as u8;
                return;
            }
            0x303C00..=0x303FFF => {
                let offset = (addr - 0x303C00) & !1;
                if self.trace_fmv {
                    println!("[CDIC WRITE WORD] offset=0x{:04X} <- 0x{:04X} at PC=0x{:08X}", offset, value, self.current_pc);
                }
                self.cdic.write_reg(offset, value);
                if self.cdic.dma_triggered {
                    self.perform_dma();
                }
                return;
            }
            0x310000..=0x317FFF => {
                self.slave.write(addr - 0x310000, value);
                return;
            }
            0x4FFFE0..=0x4FFFFF => {
                let offset = (addr - 0x4FFFE0) & !1;
                self.mcd212.write_reg(offset, value);
                self.slave.video_mode = self.mcd212.video_mode;
                return;
            }
            0xD00000..=0xDFFFFE => {
                if self.dvc.is_fitted() {
                    let idx = (addr - 0xD00000) as usize;
                    self.dvc.write_ram1(idx, (value >> 8) as u8);
                    self.dvc.write_ram1(idx + 1, value as u8);
                }
                return;
            }
            0xE00000..=0xE3FFFE => {
                if self.dvc.is_fitted() {
                    let word_offset = (addr - 0xE00000) >> 1;
                    if self.trace_fmv {
                        println!(">>> [BUS WRITE_WORD DVC] addr=0x{:08X}, reg=0x{:04X}, val=0x{:04X} at PC=0x{:08X}",
                            addr, word_offset & 0x7FFF, value, self.current_pc);
                    }
                    self.dvc.write_reg(word_offset, value);
                    if self.dvc_dma_requested() {
                        self.perform_dvc_dma();
                    }
                }
                return;
            }
            0xE80000..=0xEFFFFE => {
                if self.dvc.is_fitted() {
                    let idx = (addr - 0xE80000) as usize;
                    self.dvc.write_ram2(idx, (value >> 8) as u8);
                    self.dvc.write_ram2(idx + 1, value as u8);
                }
                return;
            }
            _ => {}
        }
        self.write_byte(address, (value >> 8) as u8);
        self.write_byte(address.wrapping_add(1), value as u8);
    }

    fn write_long(&mut self, address: u32, value: u32) {
        self.write_word(address, (value >> 16) as u16);
        self.write_word(address.wrapping_add(2), value as u16);
    }

    fn interrupt_acknowledge(&mut self, level: u8) -> u32 {
        let vec = (|| {
            let int1_lvl = (self.scc_timer.lir >> 4) & 7;
            let int1_priority = if int1_lvl != 0 { int1_lvl } else { 5 };
            if (self.scc_timer.lir & 0x80) != 0 && level == int1_priority {
                self.scc_timer.lir &= 0x7F;
                self.mcd212.vblank_irq = false;
                return 0x38 + level as u32; // SCC68070 Vector 0x3D (Vector 61)
            }

            let int2_lvl = self.scc_timer.lir & 7;
            if (self.scc_timer.lir & 0x08) != 0 && int2_lvl > 0 && level == int2_lvl {
                self.scc_timer.lir &= 0xF7;
                return 0x38 + level as u32;
            }

            let timer_lvl = self.scc_timer.picr1 & 7;
            if self.scc_timer.timer_irq && timer_lvl > 0 && level == timer_lvl {
                self.scc_timer.acknowledge_irq(level);
                return 0x38 + level as u32; // SCC68070 Vector 0x3E (Vector 62)
            }

            let uart_lvl = self.scc_timer.picr2 & 7;
            if self.scc_uart_tx_irq && uart_lvl > 0 && level == uart_lvl {
                self.scc_uart_tx_irq = false;
                return 0x38 + level as u32;
            }

            if level == 4 {
                self.arbitrate_shared_in4();
                match self.shared_in4_owner {
                    SharedIn4Owner::Vmpeg => {
                        let vec = self.dvc.intack_vector() as u32;
                        return if vec != 0 { vec } else { 0x1C };
                    }
                    SharedIn4Owner::Cdic => {
                        let vec = (self.cdic.interrupt_vector & 0xFF) as u32;
                        return if vec != 0 { vec } else { 0x1C };
                    }
                    SharedIn4Owner::Idle => {
                        return 0x1C;
                    }
                }
            }

            if level == 2 {
                self.slave.irq_pending = false;
                return 0x1A; // Autovector 2 (Vector 26) - used by OS-9 cdapdriv & pointer drivers!
            }

            0x18 + level as u32
        })();
        vec
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peek_matches_read_without_side_effects() {
        let mut bus = CdiBus::new();
        bus.mcd212.plane_a[0x5172..0x5176].copy_from_slice(&[0x00, 0x00, 0x00, 0x02]);
        bus.rom[0x5E48..0x5E4C].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        bus.sys_ram[0x10..0x12].copy_from_slice(&[0xAB, 0xCD]);
        bus.current_line_cycle = 100;

        assert_eq!(bus.peek_long(0x5172), Some(2));
        assert_eq!(bus.peek_long(0x405E48), Some(0x1234_5678));
        assert_eq!(bus.peek_word(0x500010), Some(0xABCD));
        assert_eq!(bus.peek_byte(0x303C00), None);
        assert_eq!(bus.peek_byte(0xE40000), None);
        assert_eq!(bus.peek_byte(0x8000_2011), None);
        assert_eq!((bus.wait_cycles, bus.current_line_cycle, bus.trace_setup), (0, 100, false));

        assert_eq!(bus.read_long(0x5172), 2);
        assert_eq!(bus.read_byte(0x405E48), 0x12);
        assert!(bus.wait_cycles > 0 && bus.current_line_cycle > 100 && bus.trace_setup);
    }
}

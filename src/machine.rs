use crate::bios::{self, BiosImage};
use crate::bus::CdiBus;
use crate::mcd212::VideoMode;
use m68k::{CpuCore, CpuType};
use std::path::Path;
use std::sync::atomic::Ordering;

pub struct Machine {
    pub bus: CdiBus,
    pub cpu: CpuCore,
    pub instructions: u64,
}

impl Machine {
    pub fn new() -> Self {
        Self { bus: CdiBus::new(), cpu: CpuCore::new(), instructions: 0 }
    }

    /// Reads and checks a system ROM file or zip; the name is the zip entry used, if any.
    pub fn read_bios<P: AsRef<Path>>(path: P) -> std::io::Result<(Option<String>, BiosImage)> {
        let data = std::fs::read(path)?;
        if bios::is_zip(&data) {
            let (entry, img) = bios::best_in_zip(&data).map_err(std::io::Error::other)?;
            Ok((Some(entry), img))
        } else {
            Ok((None, bios::inspect(data)))
        }
    }

    /// Loads a checked system ROM; refuses images whose check failed.
    pub fn load_bios<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<bios::Report> {
        let (entry, img) = Self::read_bios(path)?;
        let mut report = img.report.clone();
        if let Some(e) = entry {
            report.message = format!("{}: {}", e, report.message);
        }
        if report.status == bios::Status::Error {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, report.message));
        }
        self.load_bios_image(&img);
        Ok(report)
    }

    pub fn load_bios_image(&mut self, img: &BiosImage) {
        let rom = &mut self.bus.rom;
        let len = img.data.len().min(rom.len());
        rom[..len].copy_from_slice(&img.data[..len]);
        // Reset vectors (SSP at 0, PC at 4) are read from plane A RAM, as in CdiBus::load_rom.
        self.bus.mcd212.plane_a[0..8].copy_from_slice(&rom[0..8]);
    }

    pub fn load_dvc_rom<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<()> {
        self.bus.load_dvc_rom(path)
    }

    /// Reads a DVC cartridge ROM (`.rom` or a zip holding `vmpega.rom`) and checks it; nothing is fitted.
    pub fn read_dvc<P: AsRef<Path>>(path: P) -> std::io::Result<(Option<String>, BiosImage)> {
        let data = std::fs::read(path)?;
        if bios::is_zip(&data) {
            let (entry, img) = bios::best_dvc_in_zip(&data).map_err(std::io::Error::other)?;
            Ok((Some(entry), img))
        } else {
            Ok((None, bios::inspect_dvc(data)))
        }
    }

    /// Fits a checked cartridge ROM.
    pub fn load_dvc_image(&mut self, img: &BiosImage) {
        self.bus.dvc.load_rom_bytes(&img.data);
    }

    pub fn load_disc<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        self.bus.load_cdrom(path)
    }

    pub fn set_video_mode(&mut self, mode: VideoMode) {
        self.bus.set_video_mode(mode);
    }

    pub fn reset(&mut self) {
        self.cpu.set_cpu_type(CpuType::SCC68070);
        self.cpu.reset(&mut self.bus);
        self.bus.emulate_wait_states = true;
    }

    pub fn framebuffer(&self) -> &[u32] {
        &self.bus.mcd212.framebuffer[..]
    }

    /// Appends the main then sfx queue to `out`; returns the source rate.
    pub fn drain_audio(&mut self, out: &mut Vec<(f32, f32)>) -> u32 {
        let audio = &self.bus.cdic.audio;
        for q in [&audio.sample_queue, &audio.sfx_queue] {
            out.extend(q.lock().unwrap().drain(..));
        }
        audio.source_rate.load(Ordering::Relaxed)
    }

    pub fn run_frame(&mut self) {
        let bus = &mut self.bus;
        let cpu = &mut self.cpu;
        bus.slave.video_mode = bus.mcd212.video_mode;
        let total_lines = bus.mcd212.video_mode.total_scanlines();
        let cyc_per_line = bus.mcd212.video_mode.cycles_per_scanline();
        let mut cycle_debt: i32 = 0;
        for line in 0..total_lines {
            bus.tick_scanline(line);

            if bus.mcd212.vblank_irq {
                bus.scc_timer.lir |= 0x80;
                bus.mcd212.vblank_irq = false;
            }
            cpu.set_irq(highest_ipl(bus));

            let mut line_cycles = cycle_debt;
            while line_cycles < cyc_per_line as i32 {
                let remaining = (cyc_per_line as i32 - line_cycles).max(1);
                let budget = remaining.min(32);

                bus.current_pc = cpu.pc;
                bus.current_line_cycle = line_cycles.max(0) as u32;

                let batch = cpu.run_for_cycles(bus, budget);
                self.instructions += batch.instructions as u64;
                let mut batch_cycles = batch.cycles + bus.consume_wait_cycles();

                match batch.exit {
                    m68k::CycleBatchExit::TrapInstruction { trap_num } => {
                        batch_cycles += cpu.take_trap_exception(bus, trap_num);
                    }
                    m68k::CycleBatchExit::AlineTrap { .. } => {
                        batch_cycles += cpu.take_aline_exception(bus);
                    }
                    m68k::CycleBatchExit::FlineTrap { .. } => {
                        batch_cycles += cpu.take_fline_exception(bus);
                    }
                    m68k::CycleBatchExit::IllegalInstruction { .. } => {
                        batch_cycles += cpu.take_illegal_exception(bus);
                    }
                    m68k::CycleBatchExit::Breakpoint { .. } => {
                        batch_cycles += cpu.take_bkpt_exception(bus);
                    }
                    m68k::CycleBatchExit::Stopped => {
                        let remaining_line = (cyc_per_line as i32 - line_cycles).max(0) as u32;
                        bus.tick_timer(remaining_line);
                        line_cycles = cyc_per_line as i32;
                        break;
                    }
                    _ => {}
                }

                line_cycles += batch_cycles;
                bus.tick_timer(batch_cycles as u32);
                if bus.slave.reset_request {
                    bus.player_reset();
                    cpu.reset(bus);
                }
                cpu.set_irq(highest_ipl(bus));
            }

            cycle_debt = (line_cycles - cyc_per_line as i32).clamp(0, 100);

            bus.render_scanline(line);

            bus.tick_slave(cyc_per_line);
            bus.tick_cdic(cyc_per_line);
            bus.tick_dvc(cyc_per_line);
        }
    }
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

fn highest_ipl(bus: &mut CdiBus) -> u8 {
    let int1_lvl = if (bus.scc_timer.lir & 0x80) != 0 {
        let lvl = (bus.scc_timer.lir >> 4) & 7;
        if lvl != 0 { lvl } else { 5 }
    } else {
        0
    };
    let timer_lvl = if bus.scc_timer.timer_irq { bus.scc_timer.picr1 & 7 } else { 0 };
    let cdic_lvl = if bus.in4_pending() { 4 } else { 0 };
    let slave_lvl = if bus.slave.irq_pending { 2 } else { 0 };
    int1_lvl.max(timer_lvl).max(cdic_lvl).max(slave_lvl)
}

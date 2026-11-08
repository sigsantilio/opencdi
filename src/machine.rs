use crate::bios::{self, BiosImage};
use crate::bus::{BoardKind, CdiBus};
use crate::mcd212::VideoMode;
use crate::os9_trace::Os9Trace;
use crate::savestate::{self, Identity, Reader, State, StateError};
use m68k::{CpuCore, CpuType};
use std::path::Path;

pub struct Machine {
    pub bus: CdiBus,
    pub cpu: CpuCore,
    pub instructions: u64,
    pub os9_trace: Option<Box<Os9Trace>>,
}

impl Machine {
    pub fn new() -> Self {
        Self { bus: CdiBus::new(), cpu: CpuCore::new(), instructions: 0, os9_trace: Os9Trace::from_env() }
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
        self.bus.set_board(if img.report.board == Some(bios::Board::Mono4) { BoardKind::Mono4 } else { BoardKind::Mono1 });
        let rom = &mut self.bus.rom;
        let len = img.data.len().min(rom.len());
        rom[..len].copy_from_slice(&img.data[..len]);
        // Reset vectors (SSP at 0, PC at 4) are read from plane A RAM, as in CdiBus::load_rom.
        self.bus.mcd212.plane_a[0..8].copy_from_slice(&rom[0..8]);
    }

    pub fn load_dvc_rom<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<()> {
        self.bus.load_dvc_rom(path)?;
        self.bus.dvc.impeg = self.bus.board == BoardKind::Mono4;
        Ok(())
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
        self.bus.dvc.impeg = self.bus.board == BoardKind::Mono4;
    }

    /// Reads a cartridge ROM checked for a board: IMPEG on Mono-IV, VMPEG (or an unknown image) on Mono-I.
    pub fn read_dvc_for<P: AsRef<Path>>(path: P, board: BoardKind) -> std::io::Result<(Option<String>, BiosImage)> {
        match board {
            BoardKind::Mono1 => Self::read_dvc(path),
            BoardKind::Mono4 => Ok((None, bios::inspect_impeg(std::fs::read(path)?))),
        }
    }

    /// Checks and fits a cartridge ROM for the current board; refuses images whose check failed (an unknown image loads with a warning on Mono-I).
    pub fn load_dvc<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<bios::Report> {
        let (entry, img) = Self::read_dvc_for(path, self.bus.board)?;
        let mut report = img.report.clone();
        if let Some(e) = entry {
            report.message = format!("{}: {}", e, report.message);
        }
        if report.status == bios::Status::Error {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, report.message));
        }
        self.load_dvc_image(&img);
        Ok(report)
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
        self.bus.counters.halted = false;
    }

    pub fn framebuffer(&self) -> &[u32] {
        &self.bus.mcd212.framebuffer[..]
    }

    /// Appends the frames mixed since the last call at `audio::OUT_RATE`; returns how many.
    pub fn drain_audio(&mut self, out: &mut Vec<(f32, f32)>) -> usize {
        self.bus.cdic.audio.drain(out)
    }

    pub fn run_frame(&mut self) {
        let bus = &mut self.bus;
        let cpu = &mut self.cpu;
        let mut trace = self.os9_trace.as_deref_mut();
        if let Some(t) = trace.as_deref_mut() {
            t.next_frame(bus);
        }
        bus.slave.video_mode = bus.mcd212.video_mode;
        let total_lines = bus.mcd212.video_mode.total_scanlines();
        let cyc_per_line = bus.mcd212.video_mode.cycles_per_scanline();
        let mut cycle_debt: i32 = 0;
        for line in 0..total_lines {
            if let Some(t) = trace.as_deref_mut() {
                t.set_line(line);
            }
            bus.tick_scanline(line);

            if bus.mcd212.vblank_irq {
                bus.scc_timer.raise_int1();
                bus.mcd212.vblank_irq = false;
            }
            cpu.set_irq(highest_ipl(bus));

            let mut line_cycles = cycle_debt;
            while line_cycles < cyc_per_line as i32 {
                let remaining = (cyc_per_line as i32 - line_cycles).max(1);
                let budget = remaining.min(32);

                bus.current_pc = cpu.pc;
                bus.current_line_cycle = line_cycles.max(0) as u32;

                let batch = match trace.as_deref_mut() {
                    Some(t) if t.has_pending() => cpu.run_for_cycles_with_hook(bus, budget, |c, _, _| {
                        t.check_return(c);
                        m68k::CycleBatchControl::Continue
                    }),
                    _ => cpu.run_for_cycles(bus, budget),
                };
                self.instructions += batch.instructions as u64;
                let mut batch_cycles = batch.cycles + bus.consume_wait_cycles();

                match batch.exit {
                    m68k::CycleBatchExit::TrapInstruction { trap_num } => {
                        if let Some(t) = trace.as_deref_mut() {
                            t.on_trap(cpu, bus, trap_num);
                        }
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
                if bus.board_reset_request() {
                    bus.player_reset();
                    cpu.reset(bus);
                    bus.counters.halted = false;
                }
                cpu.set_irq(highest_ipl(bus));
            }

            cycle_debt = (line_cycles - cyc_per_line as i32).clamp(0, 100);

            bus.render_scanline(line);

            bus.tick_board(cyc_per_line);
            bus.tick_audio(cyc_per_line);
        }
        bus.counters.end_frame();
        // A double fault parks the CPU for good; say so once instead of idling silently.
        if cpu.is_halted() && !bus.counters.halted {
            bus.counters.halted = true;
            eprintln!("CPU halted (double fault) at frame {} pc {:06X}", bus.current_frame, cpu.pc);
        }
        if bus.taps.pc && bus.counters.frames % 10 == 0 {
            eprintln!("PC F{} {:06X}", bus.current_frame, cpu.pc);
        }
    }
}

impl Machine {
    // BIOS, disc and DVC ROM the state belongs to; a state is only loaded into a machine with the same three.
    pub fn state_identity(&self) -> Identity {
        let disc = self.bus.cdrom.as_ref().map_or(0, |cd| {
            let mut t = Vec::new();
            ((cd.total_sectors, cd.disc_type), cd.tracks.len() as u32).save(&mut t);
            for tr in &cd.tracks {
                (tr.number, tr.control).save(&mut t);
                (tr.start_lba, tr.length_sectors).save(&mut t);
            }
            savestate::checksum(&t)
        });
        Identity {
            bios: savestate::checksum(&self.bus.rom[..]),
            disc,
            dvc: self.bus.dvc.rom.as_ref().map_or(0, |r| savestate::checksum(&r[..])),
        }
    }

    // Replaces `out` with header, machine payload and the frontend's own bytes (opaque here).
    pub fn save_state(&self, frontend: &[u8], out: &mut Vec<u8>) {
        out.clear();
        out.resize(savestate::HEADER_LEN, 0);
        self.save(out);
        let payload_len = out.len() - savestate::HEADER_LEN;
        out.extend_from_slice(frontend);
        let sum = savestate::checksum(&out[savestate::HEADER_LEN..]);
        savestate::write_header(out, payload_len, frontend.len(), sum, self.state_identity());
    }

    // Loads a state from `save_state` (trailing padding allowed); on any error the machine is left unchanged.
    pub fn load_state<'a>(&mut self, data: &'a [u8]) -> Result<&'a [u8], StateError> {
        let h = savestate::read_header(data)?;
        let body = &data[savestate::HEADER_LEN..savestate::HEADER_LEN + h.payload_len + h.frontend_len];
        if savestate::checksum(body) != h.checksum {
            return Err(StateError::Checksum);
        }
        let id = self.state_identity();
        if h.id.bios != id.bios {
            return Err(StateError::BiosMismatch);
        }
        if h.id.disc != id.disc {
            return Err(StateError::DiscMismatch);
        }
        if h.id.dvc != id.dvc {
            return Err(StateError::DvcMismatch);
        }
        let (payload, frontend) = body.split_at(h.payload_len);
        let mut backup = Vec::new();
        self.save(&mut backup);
        let mut r = Reader::new(payload);
        let res = self.load(&mut r).and_then(|_| match r.remaining() {
            0 => Ok(()),
            n => Err(StateError::Trailing(n)),
        });
        if let Err(e) = res {
            let _ = self.load(&mut Reader::new(&backup));
            return Err(e);
        }
        Ok(frontend)
    }
}

crate::impl_state!(Machine { cpu, bus, instructions } skip { os9_trace });

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

pub fn highest_ipl(bus: &mut CdiBus) -> u8 {
    let int1_lvl = bus.scc_timer.int1_level();
    let timer_lvl = if bus.scc_timer.timer_irq { bus.scc_timer.picr1 & 7 } else { 0 };
    int1_lvl.max(timer_lvl).max(bus.board_ipl())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cartridge_check_depends_on_the_board() {
        let dir = std::env::temp_dir().join(format!("opencdi-dvc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("cart.rom");
        let mut data = vec![0u8; bios::DVC_ROM_SIZE];
        data[..2].copy_from_slice(&[0x4A, 0xFC]);
        data[100] = 1;
        std::fs::write(&p, &data).unwrap();
        let (_, mono1) = Machine::read_dvc_for(&p, BoardKind::Mono1).unwrap();
        let (_, mono4) = Machine::read_dvc_for(&p, BoardKind::Mono4).unwrap();
        assert!(!mono1.report.message.contains("IMPEG"));
        assert_eq!(mono4.report.status, bios::Status::Error);
        assert!(mono4.report.message.contains("differs from the known dump"));
        let short = bios::inspect_impeg(vec![1, 2, 3]);
        assert!(short.report.status == bios::Status::Error && short.report.message.contains("only the known IMPEG dump"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

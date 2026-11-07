use crate::bus::CdiBus;
use crate::mcd212::VideoMode;
use crate::savestate::{self, Identity, Reader, State, StateError};
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

    pub fn load_bios<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<()> {
        self.bus.load_rom(path)
    }

    pub fn load_dvc_rom<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<()> {
        self.bus.load_dvc_rom(path)
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

crate::impl_state!(Machine { cpu, bus, instructions } skip {});

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

use std::path::Path;
use opencdi::bus::CdiBus;
use m68k::AddressBus;
use m68k::{CpuCore, CpuType};

fn find_file(relative_subpath: &str) -> Option<std::path::PathBuf> {
    let candidates = [
        Path::new(relative_subpath).to_path_buf(),
        Path::new("..").join(relative_subpath),
        Path::new("file utili").join(relative_subpath),
        Path::new("..").join("file utili").join(relative_subpath),
    ];
    for c in &candidates {
        if c.exists() {
            return Some(c.clone());
        }
    }
    None
}

use opencdi::mcd212::{SCREEN_WIDTH, SCREEN_HEIGHT};

fn save_bmp(fb: &[u32], path: &str) {
    let width = SCREEN_WIDTH as i32;
    let height = SCREEN_HEIGHT as i32;
    let file_size = 54 + (width * height * 4) as u32;

    let mut bmp = Vec::with_capacity(file_size as usize);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&file_size.to_le_bytes());
    bmp.extend_from_slice(&[0, 0, 0, 0]);
    bmp.extend_from_slice(&54u32.to_le_bytes());

    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&width.to_le_bytes());
    bmp.extend_from_slice(&(-height).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&32u16.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&((width * height * 4) as u32).to_le_bytes());
    bmp.extend_from_slice(&2835u32.to_le_bytes());
    bmp.extend_from_slice(&2835u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());

    for &pixel in fb {
        let b = (pixel & 0xFF) as u8;
        let g = ((pixel >> 8) & 0xFF) as u8;
        let r = ((pixel >> 16) & 0xFF) as u8;
        bmp.push(b);
        bmp.push(g);
        bmp.push(r);
        bmp.push(0xFF);
    }
    let _ = std::fs::write(path, &bmp);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bus = CdiBus::new();
    let bios_path = find_file("bios/cdi220b.rom").expect("BIOS not found");
    bus.load_rom(&bios_path)?;
    if let Some(nv) = find_file("bios/nvram_mk48t08.bin") {
        let _ = bus.nvram.load_from_file(&nv);
    }
    if let Some(dvc_path) = find_file("dvc/vmpega.rom") {
        let _ = bus.load_dvc_rom(&dvc_path);
    }
    let game_path = find_file("giochi/alberoazzurro.chd").expect("Albero Azzurro not found");
    bus.load_cdrom(&game_path)?;

    let mut cpu = CpuCore::new();
    cpu.set_cpu_type(CpuType::SCC68070);
    cpu.reset(&mut bus);

    let mut disc_launched = false;
    let mut shell_ready_count = 0;

    for frame in 0..4500 {
        if bus.cdrom.is_some() && !disc_launched {
            let (_effective_shell_base, shell_state) = if bus.dvc.is_fitted() {
                let s_dvc = bus.read_long(0x00D08090u32.wrapping_add(0xFFFF9172));
                if s_dvc >= 2 {
                    (0x00D08090u32, s_dvc)
                } else {
                    (0x0000C000u32, bus.read_long(0x0000C000u32.wrapping_add(0xFFFF9172)))
                }
            } else {
                (0x0000C000u32, bus.read_long(0x0000C000u32.wrapping_add(0xFFFF9172)))
            };

            if shell_state >= 2 {
                shell_ready_count += 1;
                if shell_ready_count == 1 || shell_ready_count == 150 {
                    bus.slave.update_mouse(575, 335, 0);
                } else if shell_ready_count == 30 || shell_ready_count == 180 {
                    bus.slave.update_mouse(575, 335, 1);
                } else if shell_ready_count == 50 || shell_ready_count == 200 {
                    bus.slave.update_mouse(575, 335, 0);
                }
            }
            if ((cpu.pc >= 0x00010000 && cpu.pc < 0x00300000) || (cpu.pc >= 0x00D00000 && cpu.pc < 0x00E00000)) && frame > 250 {
                disc_launched = true;
                bus.slave.update_mouse(384, 280, 0);
            }
        }

        let total_lines = bus.mcd212.video_mode.total_scanlines();
        let cyc_per_line = bus.mcd212.video_mode.cycles_per_scanline();

        for line in 0..total_lines {
            bus.tick_scanline(line);
            if bus.mcd212.vblank_irq {
                bus.scc_timer.lir |= 0x80;
                bus.mcd212.vblank_irq = false;
            }

            let int1_lvl = if (bus.scc_timer.lir & 0x80) != 0 {
                let lvl = (bus.scc_timer.lir >> 4) & 7;
                if lvl != 0 { lvl } else { 5 }
            } else { 0 };
            let timer_lvl = if bus.scc_timer.timer_irq {
                bus.scc_timer.picr1 & 7
            } else { 0 };
            let cdic_lvl = if bus.in4_pending() { 4 } else { 0 };
            let slave_lvl = if bus.slave.irq_pending { 2 } else { 0 };
            let highest_ipl = int1_lvl.max(timer_lvl).max(cdic_lvl).max(slave_lvl);
            cpu.set_irq(highest_ipl);

            let mut line_cycles = 0;
            while line_cycles < cyc_per_line as i32 {
                let remaining = (cyc_per_line as i32 - line_cycles).max(1);
                let budget = remaining.min(32);
                bus.current_pc = cpu.pc;
                bus.current_line_cycle = line_cycles.max(0) as u32;

                let batch = cpu.run_for_cycles(&mut bus, budget);
                let mut batch_cycles = batch.cycles + bus.consume_wait_cycles();

                match batch.exit {
                    m68k::CycleBatchExit::TrapInstruction { trap_num } => {
                        let trap_cyc = cpu.take_trap_exception(&mut bus, trap_num);
                        batch_cycles += trap_cyc;
                    }
                    m68k::CycleBatchExit::AlineTrap { .. } => {
                        let trap_cyc = cpu.take_aline_exception(&mut bus);
                        batch_cycles += trap_cyc;
                    }
                    m68k::CycleBatchExit::FlineTrap { .. } => {
                        let trap_cyc = cpu.take_fline_exception(&mut bus);
                        batch_cycles += trap_cyc;
                    }
                    m68k::CycleBatchExit::IllegalInstruction { .. } => {
                        let trap_cyc = cpu.take_illegal_exception(&mut bus);
                        batch_cycles += trap_cyc;
                    }
                    m68k::CycleBatchExit::Breakpoint { .. } => {
                        let trap_cyc = cpu.take_bkpt_exception(&mut bus);
                        batch_cycles += trap_cyc;
                    }
                    m68k::CycleBatchExit::Stopped => {
                        let remaining_line = (cyc_per_line as i32 - line_cycles).max(0) as u32;
                        bus.tick_timer(remaining_line);
                        break;
                    }
                    _ => {}
                }

                line_cycles += batch_cycles;
                bus.tick_timer(batch_cycles as u32);
            }

            bus.render_scanline(line);
            bus.tick_slave(cyc_per_line);
            bus.tick_cdic(cyc_per_line);
            bus.tick_dvc(cyc_per_line);
        }

        if frame % 250 == 0 || frame == 6000 {
            let non_black = bus.mcd212.framebuffer.iter().filter(|&&p| (p & 0x00FFFFFF) != 0).count();
            println!(
                "[F{:4}] PC=0x{:06X} DVC play={} dec={} | icm=0x{:08X} tp=0x{:08X} | FB non-black={}",
                frame, cpu.pc, bus.dvc.video_playing, bus.dvc.frames_decoded_count,
                bus.mcd212.image_coding_method, bus.mcd212.transparency_control, non_black
            );
        }

        if frame == 1000 || frame == 1500 || frame == 2000 || frame == 2500 || frame == 3000 || frame == 3500 || frame == 4000 {
            let path = format!("albero_{}.bmp", frame);
            save_bmp(&bus.mcd212.framebuffer[..], &path);
            println!("Saved {}", path);
        }
    }

    Ok(())
}

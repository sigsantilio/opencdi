use std::path::{Path, PathBuf};
use opencdi::bus::CdiBus;
use m68k::AddressBus;
use opencdi::mcd212::{SCREEN_WIDTH, SCREEN_HEIGHT, VideoMode};
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
    if let Some(dvc_path) = find_file("dvc/vmpega.rom") {
        let _ = bus.load_dvc_rom(&dvc_path);
    }
    let game_path = find_file("giochi/Ultra CD-i Soccer (Europe).chd").expect("Game not found");
    bus.load_cdrom(&game_path)?;
    bus.set_video_mode(VideoMode::Pal);

    let mut cpu = CpuCore::new();
    cpu.set_cpu_type(CpuType::SCC68070);
    cpu.reset(&mut bus);

    let mut disc_launched = false;
    let mut shell_ready_count = 0;

    for frame in 0..3100 {
        if bus.cdrom.is_some() && !disc_launched {
            let s_dvc = bus.read_long(0x00D08090u32.wrapping_add(0xFFFF9172));
            let shell_state = if s_dvc >= 2 { s_dvc } else { bus.read_long(0x0000C000u32.wrapping_add(0xFFFF9172)) };
            if shell_state >= 2 {
                shell_ready_count += 1;
                if shell_ready_count == 30 || shell_ready_count == 180 {
                    bus.slave.update_mouse(575, 335, 1);
                } else if shell_ready_count == 50 || shell_ready_count == 200 {
                    bus.slave.update_mouse(575, 335, 0);
                }
            }
            if ((cpu.pc >= 0x00010000 && cpu.pc < 0x00300000) || (cpu.pc >= 0x00D00000 && cpu.pc < 0x00E00000)) && frame > 250 {
                disc_launched = true;
                bus.emulate_wait_states = true;
                bus.slave.update_mouse(384, 280, 0);
            }
        } else if disc_launched {
            // Emulate clicks to advance menu: e.g. at frame 1400, 1800, 2200, 2600, 3000, 3400, 3800
            let btn = if (frame >= 1400 && frame <= 1420)
                || (frame >= 1800 && frame <= 1820)
                || (frame >= 2200 && frame <= 2220)
                || (frame >= 2600 && frame <= 2620)
                || (frame >= 3000 && frame <= 3020)
                || (frame >= 3400 && frame <= 3420)
                || (frame >= 3800 && frame <= 3820)
                || (frame >= 4200 && frame <= 4220) { 1 } else { 0 };
            bus.slave.update_mouse(384, 280, btn);
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
            let timer_lvl = if bus.scc_timer.timer_irq { bus.scc_timer.picr1 & 7 } else { 0 };
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

                let int1_lvl = if (bus.scc_timer.lir & 0x80) != 0 {
                    let lvl = (bus.scc_timer.lir >> 4) & 7;
                    if lvl != 0 { lvl } else { 5 }
                } else { 0 };
                let timer_lvl = if bus.scc_timer.timer_irq { bus.scc_timer.picr1 & 7 } else { 0 };
                let cdic_lvl = if bus.in4_pending() { 4 } else { 0 };
                let slave_lvl = if bus.slave.irq_pending { 2 } else { 0 };
                let highest_ipl = int1_lvl.max(timer_lvl).max(cdic_lvl).max(slave_lvl);
                cpu.set_irq(highest_ipl);
            }

            bus.render_scanline(line);
            bus.tick_slave(cyc_per_line);
            bus.tick_cdic(cyc_per_line);
            bus.tick_dvc(cyc_per_line);
        }

        if frame >= 2220 && frame <= 2225 {
            println!("--- Frame {} ---", frame);
            println!("ICM=0x{:08X} TCR=0x{:08X} Order=0x{:X} Bank=[{}, {}] Backdrop=0x{:X}",
                bus.mcd212.image_coding_method, bus.mcd212.transparency_control,
                bus.mcd212.plane_order, bus.mcd212.clut_bank[0], bus.mcd212.clut_bank[1],
                bus.mcd212.backdrop_color);
            println!("DDR=[0x{:04X}, 0x{:04X}] DCR=[0x{:04X}, 0x{:04X}] WF=[{}, {}]",
                bus.mcd212.ddr[0], bus.mcd212.ddr[1],
                bus.mcd212.dcr[0], bus.mcd212.dcr[1],
                bus.mcd212.weight_factor[0][0], bus.mcd212.weight_factor[1][0]);
            println!("Transparent Color=[0x{:08X}, 0x{:08X}] Mask=[0x{:08X}, 0x{:08X}]",
                bus.mcd212.transparent_color[0], bus.mcd212.transparent_color[1],
                bus.mcd212.mask_color[0], bus.mcd212.mask_color[1]);
            println!("VSR=[0x{:08X}, 0x{:08X}] DCA=[0x{:08X}, 0x{:08X}]",
                bus.mcd212.vsr[0], bus.mcd212.vsr[1],
                bus.mcd212.dca[0], bus.mcd212.dca[1]);
            // Sample clut bank 0 and 1
            println!("CLUT[0..8]: {:08X?}", &bus.mcd212.clut[0..8]);
            println!("CLUT[128..136]: {:08X?}", &bus.mcd212.clut[128..136]);
            if frame == 2225 {
                let mut pa = [0u32; 768];
                let mut pb = [0u32; 768];
                let mut ta = [false; 768];
                let mut tb = [false; 768];
                bus.mcd212.process_vsr::<0>(&mut pa, &mut ta);
                bus.mcd212.process_vsr::<1>(&mut pb, &mut tb);
                let ta_count = ta.iter().filter(|&&t| t).count();
                let tb_count = tb.iter().filter(|&&t| t).count();
                println!("Frame 2225 Scanline test: ta_count={} tb_count={}", ta_count, tb_count);
                let pa_nz = pa.iter().filter(|&&p| (p & 0xFFFFFF) != 0).count();
                let pb_nz = pb.iter().filter(|&&p| (p & 0xFFFFFF) != 0).count();
                println!("Frame 2225 Scanline test: pa_nz={} pb_nz={}", pa_nz, pb_nz);
                println!("Sample pa[300..310]: {:08X?}", &pa[300..310]);
                println!("Sample ta[300..310]: {:?}", &ta[300..310]);
                println!("Sample pb[300..310]: {:08X?}", &pb[300..310]);
                println!("Sample tb[300..310]: {:?}", &tb[300..310]);
            }
        }

        if (frame >= 2850 && frame <= 3050 && frame % 5 == 0) || frame % 100 == 0 {
            let path = format!("soccer_f{:04}.bmp", frame);
            save_bmp(&bus.mcd212.framebuffer[..], &path);
            let nz = bus.mcd212.framebuffer.iter().filter(|&&p| (p & 0xFFFFFF) != 0).count();
            println!("[Frame {:4}] PC=0x{:06X} ICM=0x{:08X} TCR=0x{:08X} Order=0x{:X} VSR=[0x{:X}, 0x{:X}] NZ={}",
                frame, cpu.pc, bus.mcd212.image_coding_method, bus.mcd212.transparency_control,
                bus.mcd212.plane_order, bus.mcd212.vsr[0], bus.mcd212.vsr[1], nz);
        }
    }

    Ok(())
}

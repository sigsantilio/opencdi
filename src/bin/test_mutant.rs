use std::path::Path;
use opencdi::bus::CdiBus;
use m68k::AddressBus;
use opencdi::mcd212::{SCREEN_WIDTH, SCREEN_HEIGHT};
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
    if let Some(nv) = find_file("bios/nvram_mk48t08.bin") {
        let _ = bus.nvram.load_from_file(&nv);
    }
    if !std::env::args().any(|a| a == "--no-dvc") {
        if let Some(dvc_path) = find_file("dvc/vmpega.rom") {
            let _ = bus.load_dvc_rom(&dvc_path);
            println!("Loaded DVC ROM: {:?}", dvc_path);
        }
    } else {
        println!("Running WITHOUT DVC ROM (--no-dvc)");
    }
    let disc_arg = std::env::args().nth(1).unwrap_or_else(|| "giochi/Mutant Rampage - Bodyslam (USA).chd".to_string());
    let game_path = find_file(&disc_arg).expect("Game not found");
    bus.load_cdrom(&game_path)?;
    println!("Loaded game: {:?}", game_path);
    let name_upper = disc_arg.to_uppercase();
    if name_upper.contains("(USA)") || name_upper.contains("(JAPAN)") || name_upper.contains("NTSC") {
        bus.set_video_mode(opencdi::mcd212::VideoMode::Ntsc);
    } else {
        bus.set_video_mode(opencdi::mcd212::VideoMode::Pal);
    }

    let mut cpu = CpuCore::new();
    cpu.set_cpu_type(CpuType::SCC68070);
    cpu.reset(&mut bus);

    let mut disc_launched = false;
    let mut shell_ready_count = 0;
    let mut pad_x = 384i32;
    let mut pad_y = 280i32;

    for frame in 0..7000 {
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
                pad_x = 384;
                pad_y = 280;
                bus.slave.update_mouse(384, 280, 0);
            }
        }

        if bus.slave.device_mouse_updated {
            if frame >= 4500 {
                println!("[F{:4}] Game warped mouse to ({}, {})", frame, bus.slave.device_mouse_x, bus.slave.device_mouse_y);
            }
            pad_x = bus.slave.device_mouse_x;
            pad_y = bus.slave.device_mouse_y;
            bus.slave.device_mouse_updated = false;
        }

        // Hotel Mario gameplay test (exact v0.1.3 sequence)
        if disc_arg.contains("Hotel Mario") {
            let presses: [(u64, u8, u64); 13] = [
                (4120, 1, 20), (4250, 1, 20), (4350, 1, 20), (4450, 1, 20),
                (4700, 1, 20), (4800, 1, 20), (4900, 1, 20), (5000, 1, 20),
                (5100, 1, 20), (5250, 1, 20), (5350, 1, 20), (5450, 1, 20),
                (5550, 1, 20),
            ];
            if disc_launched {
                let mut btn: u8 = 0;
                for &(f, b, len) in &presses {
                    if frame >= f && frame < f + len { btn |= b; }
                }
                if frame >= 5700 {
                    pad_x = (pad_x + 8).clamp(0, 767);
                }
                bus.slave.update_mouse(pad_x, pad_y, btn);
            }
        } else {
            // Mutant Rampage inputs
            if frame == 3050 || frame == 3300 || frame == 3600 || frame == 3900 || frame == 4200 || frame == 4500 {
                bus.slave.update_mouse(pad_x, pad_y, 1);
            } else if frame == 3060 || frame == 3310 || frame == 3610 || frame == 3910 || frame == 4210 || frame == 4510 {
                bus.slave.update_mouse(pad_x, pad_y, 0);
            } else if frame >= 5250 && frame < 5600 {
                pad_x = (pad_x + 8).clamp(0, 767);
                bus.slave.update_mouse(pad_x, pad_y, 0);
            }
        }
        bus.slave.debug_log = false;

        let total_lines = bus.mcd212.video_mode.total_scanlines();
        let cyc_per_line = bus.mcd212.video_mode.cycles_per_scanline();

        for line in 0..total_lines {
            if frame == 3000 && line == 0 {
                for p in 0..2 {
                    let plane = if p == 0 { &bus.mcd212.plane_a } else { &bus.mcd212.plane_b };
                    let mut addr = 0x400usize;
                    println!("=== Frame 3000 ICA{} ===", p);
                    for _ in 0..64 {
                        let cmd = u32::from_be_bytes([plane[addr], plane[addr+1], plane[addr+2], plane[addr+3]]);
                        println!("  ICA{} @0x{:05X}: 0x{:08X}", p, addr, cmd);
                        addr += 4;
                        let op = (cmd >> 24) as u8;
                        if (0x00..=0x0F).contains(&op) || (0x30..=0x3F).contains(&op) || (0x50..=0x5F).contains(&op) {
                            break;
                        }
                        if (0x40..=0x4F).contains(&op) {
                            addr = (cmd & 0x0007_FFFF) as usize;
                        }
                    }
                }
            }
            if frame == 3000 && line >= 32 {
                let dca0 = (bus.mcd212.dca[0] & 0x7FFFC) as usize;
                let dca1 = (bus.mcd212.dca[1] & 0x7FFFC) as usize;
                let mut cmds0 = Vec::new();
                let mut cmds1 = Vec::new();
                for w in 0..16 {
                    let c0 = u32::from_be_bytes([
                        bus.mcd212.plane_a[dca0 + w * 4],
                        bus.mcd212.plane_a[dca0 + w * 4 + 1],
                        bus.mcd212.plane_a[dca0 + w * 4 + 2],
                        bus.mcd212.plane_a[dca0 + w * 4 + 3],
                    ]);
                    if c0 != 0x10000000 {
                        cmds0.push(format!("0x{:08X}", c0));
                    }
                    if (c0 >> 24) == 0 || (c0 >> 24) == 0x30 || (c0 >> 24) == 0x50 {
                        break;
                    }
                }
                for w in 0..16 {
                    let c1 = u32::from_be_bytes([
                        bus.mcd212.plane_b[dca1 + w * 4],
                        bus.mcd212.plane_b[dca1 + w * 4 + 1],
                        bus.mcd212.plane_b[dca1 + w * 4 + 2],
                        bus.mcd212.plane_b[dca1 + w * 4 + 3],
                    ]);
                    if c1 != 0x10000000 {
                        cmds1.push(format!("0x{:08X}", c1));
                    }
                    if (c1 >> 24) == 0 || (c1 >> 24) == 0x30 || (c1 >> 24) == 0x50 {
                        break;
                    }
                }
                if line - 32 >= 235 || line - 32 <= 5 {
                    println!("  [F3000 L{:3} (act {:3})] DCA0@0x{:05X}={:?} | DCA1@0x{:05X}={:?} | icm=0x{:06X} tcr=0x{:06X} vsr0=0x{:06X} vsr1=0x{:06X}",
                        line, line - 32, dca0, cmds0, dca1, cmds1, bus.mcd212.image_coding_method, bus.mcd212.transparency_control, bus.mcd212.get_vsr::<0>(), bus.mcd212.get_vsr::<1>());
                }
            }
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
                        if trap_num == 0 && frame >= 4700 && frame <= 4745 {
                            let syscall = bus.read_word(cpu.pc);
                            println!("[F{:4}] TRAP #0 syscall=0x{:04X} at PC=0x{:06X}: D0=0x{:08X} D1=0x{:08X} D2=0x{:08X}",
                                frame, syscall, cpu.pc, cpu.dar[0], cpu.dar[1], cpu.dar[2]);
                        }
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

        if frame % 200 == 0 {
            let aud_q = bus.cdic.audio.main.len();
            println!(
                "[F{:4}] PC=0x{:06X} | DCR0=0x{:04X} DCR1=0x{:04X} VSR0=0x{:06X} VSR1=0x{:06X} | DVC fit={} play={} dec_cnt={} | aud_q={}",
                frame, cpu.pc,
                bus.mcd212.dcr[0], bus.mcd212.dcr[1],
                bus.mcd212.get_vsr::<0>(), bus.mcd212.get_vsr::<1>(),
                bus.dvc.is_fitted(), bus.dvc.video_playing, bus.dvc.frames_decoded_count,
                aud_q
            );
        }

        if frame == 2500 {
            println!("=== Searching for F$Intercept (TRAP 0 0x002A) in Mutant Rampage RAM (0xD00000..0xE00000) ===");
            for addr in (0x00D00000..0x00E00000).step_by(2) {
                if bus.read_word(addr) == 0x4E40 && bus.read_word(addr + 2) == 0x002A {
                    println!("Found F$Intercept at 0x{:06X}", addr);
                    let mut dpc = addr.saturating_sub(16);
                    while dpc <= addr + 24 {
                        let w = bus.read_word(dpc);
                        let (s, sz) = m68k::dasm::disassemble(dpc, w, CpuType::SCC68070);
                        println!("  0x{:06X}: {:04X}  {}", dpc, w, s);
                        dpc += sz.max(2);
                    }
                }
            }
        }

        if disc_arg.contains("Hotel Mario") {
            if [5700, 5750, 5800, 5850, 5900].contains(&frame) {
                let path = format!("C:/Users/Gian/.gemini/antigravity/brain/3c367666-9892-4238-9468-c77864b7ddc9/test_mario_stage1_{}.bmp", frame);
                save_bmp(&bus.mcd212.framebuffer[..], &path);
                println!("Saved {}", path);
            }
            if frame >= 5910 {
                break;
            }
        } else {
            if [5000, 5200, 5300, 5400, 5500, 5600, 5700].contains(&frame) {
                let path = format!("C:/Users/Gian/.gemini/antigravity/brain/3c367666-9892-4238-9468-c77864b7ddc9/test_move_{}.bmp", frame);
                save_bmp(&bus.mcd212.framebuffer[..], &path);
                println!("Saved {}", path);
            }
            if frame >= 5800 {
                break;
            }
        }
    }

    Ok(())
}

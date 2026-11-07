use std::path::Path;
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
    if let Some(nv) = find_file("bios/nvram_mk48t08.bin") {
        let _ = bus.nvram.load_from_file(&nv);
    }
    if let Some(dvc_path) = find_file("dvc/vmpega.rom") {
        let _ = bus.load_dvc_rom(&dvc_path);
        println!("Loaded DVC ROM: {:?}", dvc_path);
    }
    let game_path = find_file("giochi/Ultra CD-i Soccer (Europe).chd").expect("Game not found");
    bus.load_cdrom(&game_path)?;
    bus.set_video_mode(VideoMode::Pal);

    let mut cpu = CpuCore::new();
    cpu.set_cpu_type(CpuType::SCC68070);
    cpu.reset(&mut bus);

    let mut disc_launched = false;
    let mut shell_ready_count = 0;
    let mut trap6_countdown = 0;
    let mut trap6_trace = 0;

    for frame in 0..950 {
        if frame == 880 {
            println!("ICA Path 0 (Plane A at 0x400):");
            for i in (0x400..0x440).step_by(4) {
                let cmd = bus.read_long(i as u32);
                println!("  0x{:04X}: {:08X}", i, cmd);
            }
            println!("ICA Path 1 (Plane B at 0x200400):");
            for i in (0x400..0x440).step_by(4) {
                let cmd = bus.read_long(0x200000 + i as u32);
                println!("  0x{:04X}: {:08X}", i, cmd);
            }
            println!("Plane A at 0x07FB10..0x07FB60:");
            for i in (0x07FB10..0x07FB60).step_by(4) {
                println!("  0x{:06X}: {:08X}", i, bus.read_long(i as u32));
            }
            println!("Plane A at 0x07FFB0..0x07FFFE:");
            for i in (0x07FFB0..0x07FFFE).step_by(4) {
                println!("  0x{:06X}: {:08X}", i, bus.read_long(i as u32));
            }
            let dcp0 = bus.mcd212.get_dcp::<0>() as usize;
            let dcp1 = bus.mcd212.get_dcp::<1>() as usize;
            println!("DCP Path 0 = 0x{:06X}, Path 1 = 0x{:06X}", dcp0, dcp1);
        }
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
                let click_y = 335;
                if shell_ready_count == 1 || shell_ready_count == 150 {
                    bus.slave.update_mouse(575, click_y, 0);
                } else if shell_ready_count == 30 || shell_ready_count == 180 {
                    bus.slave.update_mouse(575, click_y, 1);
                } else if shell_ready_count == 50 || shell_ready_count == 200 {
                    bus.slave.update_mouse(575, click_y, 0);
                }
            }
            if ((cpu.pc >= 0x00010000 && cpu.pc < 0x00300000) || (cpu.pc >= 0x00D00000 && cpu.pc < 0x00E00000)) && frame > 250 {
                disc_launched = true;
                bus.emulate_wait_states = true;
                bus.slave.update_mouse(384, 280, 0);
            }
        } else if disc_launched {
            let btn = if (frame >= 1500 && frame <= 1520) || (frame >= 2000 && frame <= 2020) { 1 } else { 0 };
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

                if cpu.pc == 0x00409772 {
                    println!("[F$IRQ CALL F{}] D0=0x{:08X} (vector={}) D1=0x{:08X} (pri={}) A0=0x{:08X} A2=0x{:08X}",
                        frame, cpu.d(0), cpu.d(0) & 0xFFFF, cpu.d(1), cpu.d(1) & 0xFF, cpu.a(0), cpu.a(2));
                }
                if frame >= 867 && frame <= 872 && cpu.pc >= 0x42B000 && cpu.pc < 0x42E000 {
                    println!("  [cdapdriv F{}] PC=0x{:06X} (op=0x{:04X}) D0=0x{:08X} D1=0x{:08X} A0=0x{:08X} A2=0x{:08X} A3=0x{:08X}",
                        frame, cpu.pc, bus.read_word(cpu.pc), cpu.d(0), cpu.d(1), cpu.a(0), cpu.a(2), cpu.a(3));
                }
                let batch = cpu.run_for_cycles(&mut bus, budget);
                let mut batch_cycles = batch.cycles + bus.consume_wait_cycles();

                match batch.exit {
                    m68k::CycleBatchExit::TrapInstruction { trap_num } => {
                        if trap_num == 0 {
                            let code = bus.read_word(cpu.pc);
                            if code == 0x0084 {
                                let mut s = String::new();
                                for i in 0..32 {
                                    let b = bus.read_byte(cpu.a(0) + i);
                                    if b == 0 || b == 0x0D { break; }
                                    s.push(b as char);
                                }
                                println!("[I$Open F{}] at PC=0x{:06X}: name={:?} mode=0x{:X}", frame, cpu.pc.wrapping_sub(2), s, cpu.d(0));
                            }
                            if code == 0x0021 {
                                let mut s = String::new();
                                for i in 0..32 {
                                    let b = bus.read_byte(cpu.a(0) + i);
                                    if b == 0 || b == 0x0D { break; }
                                    s.push(b as char);
                                }
                                let bytes: Vec<u8> = (0..16).map(|i| bus.read_byte(cpu.a(0) + i)).collect();
                                println!("[F$Ev_Link F{}] at PC=0x{:06X}: bytes={:02X?} event name={:?} D0=0x{:X}", frame, cpu.pc.wrapping_sub(2), bytes, s, cpu.d(0));
                            }
                            if code == 0x0018 {
                                println!("[F$IRQ F{}] PC=0x{:06X}: D0=0x{:08X} (vector={}) D1=0x{:08X} (pri={}) A0=0x{:08X} A2=0x{:08X}",
                                    frame, cpu.pc.wrapping_sub(2), cpu.d(0), cpu.d(0) & 0xFFFF, cpu.d(1), cpu.d(1) & 0xFF, cpu.a(0), cpu.a(2));
                            }
                            if code >= 0x0080 && code <= 0x0090 {
                                println!("[OS9 I$ 0x{:04X} F{}] PC=0x{:06X}: D0=0x{:08X} D1=0x{:08X} A0=0x{:08X} A1=0x{:08X} A2=0x{:08X}",
                                    code, frame, cpu.pc.wrapping_sub(2), cpu.d(0), cpu.d(1), cpu.a(0), cpu.a(1), cpu.a(2));
                            }
                        } else {
                            if trap_num == 6 {
                                println!("[TRAP #6 F{}] PC=0x{:06X} D0=0x{:08X} D1=0x{:08X}", frame, cpu.pc, cpu.d(0), cpu.d(1));
                            }
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
            }

            bus.render_scanline(line);
            bus.tick_slave(cyc_per_line);
            bus.tick_cdic(cyc_per_line);
            bus.tick_dvc(cyc_per_line);
        }

        let white_cnt = bus.mcd212.framebuffer.iter().filter(|&&p| (p & 0xFFFFFF) == 0xFFFFFF).count();
        let nz = bus.mcd212.framebuffer.iter().filter(|&&p| (p & 0xFFFFFF) != 0).count();

        if frame == 868 {
            println!("Searching 4E40 0018 (F$IRQ) in ROM (0x400000..0x480000):");
            for a in (0x400000..0x480000).step_by(2) {
                if bus.read_word(a) == 0x4E40 && bus.read_word(a + 2) == 0x0018 {
                    println!("  Found F$IRQ at 0x{:06X}", a);
                }
            }
        }
        if frame == 870 {
            println!("Video Driver Init at 0x417770..0x4178C0:");
            let mut addr = 0x417770;
            while addr < 0x4178C0 {
                println!("  {:06X}: {:04X} {:04X} {:04X} {:04X}", addr, bus.read_word(addr), bus.read_word(addr+2), bus.read_word(addr+4), bus.read_word(addr+6));
                addr += 8;
            }

            println!("Memory at 0x00DFC820..0x00DFC860:");
            for a in (0x00DFC820..0x00DFC860).step_by(8) {
                println!("  {:06X}: {:04X} {:04X} {:04X} {:04X}", a, bus.read_word(a), bus.read_word(a+2), bus.read_word(a+4), bus.read_word(a+6));
            }
            println!("Memory at 0x00DFC670..0x00DFC6B0:");
            for a in (0x00DFC670..0x00DFC6B0).step_by(8) {
                println!("  {:06X}: {:04X} {:04X} {:04X} {:04X}", a, bus.read_word(a), bus.read_word(a+2), bus.read_word(a+4), bus.read_word(a+6));
            }
            println!("Registers: D0={:08X} D1={:08X} D7={:08X} A0={:08X} A6={:08X} SR={:04X}", cpu.d(0), cpu.d(1), cpu.d(7), cpu.a(0), cpu.a(6), cpu.get_sr());
            let sys_glob = bus.read_long(0x00000000);
            let sys_call_table = bus.read_long(sys_glob + 0x03A4);
            let virq_handler = bus.read_long(sys_call_table + (0x8E * 4));
            println!("SysGlob=0x{:08X}, SysCallTable=0x{:08X}, F$VIRQ handler=0x{:08X}", sys_glob, sys_call_table, virq_handler);
            println!("SysCallTable[0x20..0x30]:");
            for i in 0x20..=0x30 {
                println!("  Syscall 0x{:02X}: 0x{:08X}", i, bus.read_long(sys_call_table + i * 4));
            }
            println!("CPU Exception Vectors 24..64:");
            for vec in 24..=64 {
                let addr = bus.read_long(vec * 4);
                if addr != 0 {
                    println!("  Vector {} (0x{:02X}): 0x{:08X}", vec, vec, addr);
                }
            }
            println!("Code at 0x27FF2E..0x27FFA0:");
            let mut addr = 0x27FF2E;
            while addr < 0x27FFA0 {
                println!("  {:06X}: {:04X} {:04X} {:04X} {:04X}", addr, bus.read_word(addr), bus.read_word(addr+2), bus.read_word(addr+4), bus.read_word(addr+6));
                addr += 8;
            }
        }

        if frame >= 840 && frame <= 900 && frame % 10 == 0 {
            println!("--- FRAME {} ---", frame);
            println!("  SCC TIMER: TSR=0x{:02X}, TCR=0x{:02X}, PICR1=0x{:02X}, PICR2=0x{:02X}, LIR=0x{:02X}, timer_irq={}, int1_line={}",
                bus.scc_timer.tsr, bus.scc_timer.tcr, bus.scc_timer.picr1, bus.scc_timer.picr2, bus.scc_timer.lir, bus.scc_timer.timer_irq, bus.scc_timer.int1_line);
            println!("  DCR: 0x{:04X} / 0x{:04X}, CSRW: 0x{:04X} / 0x{:04X}", bus.mcd212.dcr[0], bus.mcd212.dcr[1], bus.mcd212.csrw[0], bus.mcd212.csrw[1]);
            println!("  DDR: 0x{:04X} / 0x{:04X}, DCP: 0x{:06X} / 0x{:06X}, DCA: 0x{:06X} / 0x{:06X}", bus.mcd212.ddr[0], bus.mcd212.ddr[1], bus.mcd212.dcp[0], bus.mcd212.dcp[1], bus.mcd212.dca[0], bus.mcd212.dca[1]);
            println!("  ICM: 0x{:08X}, TCR: 0x{:08X}, Order: 0x{:X}", bus.mcd212.image_coding_method, bus.mcd212.transparency_control, bus.mcd212.plane_order);
            println!("  CLUT Bank: {:?}, Backdrop: 0x{:02X}", bus.mcd212.clut_bank, bus.mcd212.backdrop_color);
            println!("  TP Color: 0x{:06X} / 0x{:06X}, Mask: 0x{:06X} / 0x{:06X}", bus.mcd212.transparent_color[0], bus.mcd212.transparent_color[1], bus.mcd212.mask_color[0], bus.mcd212.mask_color[1]);
            println!("  WF[0][0]={} WF[1][0]={}", bus.mcd212.weight_factor[0][0], bus.mcd212.weight_factor[1][0]);
            let nz_a = bus.mcd212.plane_a.iter().filter(|&&b| b != 0).count();
            let nz_b = bus.mcd212.plane_b.iter().filter(|&&b| b != 0).count();
            let clut_nz = bus.mcd212.clut.iter().filter(|&&c| (c & 0xFFFFFF) != 0).count();
            println!("  Plane A nz={}, Plane B nz={}, CLUT nz={}, FB nz={}", nz_a, nz_b, clut_nz, nz);
        }

        if (frame > 850 && nz > 0 && frame % 100 == 0) || (frame > 850 && white_cnt > 100000 && frame % 100 == 0) || (frame % 250 == 0) {
            let path = format!("C:/Users/Gian/.gemini/antigravity/brain/3c367666-9892-4238-9468-c77864b7ddc9/soccer_frame_{}.bmp", frame);
            save_bmp(&bus.mcd212.framebuffer[..], &path);
            println!("Saved screenshot to {} (non-black={}, white={})", path, nz, white_cnt);
        }
    }

    Ok(())
}

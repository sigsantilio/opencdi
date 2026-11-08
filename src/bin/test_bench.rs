#[path = "../audio.rs"]
mod audio;
#[path = "../bus.rs"]
mod bus;
#[path = "../cdic.rs"]
mod cdic;
#[path = "../cdrom.rs"]
mod cdrom;
#[path = "../dma.rs"]
mod dma;
#[path = "../dvc.rs"]
mod dvc;
#[path = "../mcd212.rs"]
mod mcd212;
#[path = "../nvram.rs"]
mod nvram;
#[path = "../scc_timer.rs"]
mod scc_timer;
#[path = "../slave.rs"]
mod slave;

use std::path::{Path, PathBuf};
use std::time::Instant;
use m68k::{CpuCore, CpuType, AddressBus};
use crate::bus::CdiBus;

fn find_file(relative_subpath: &str) -> Option<PathBuf> {
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

fn test_game(game_name: &str, frames: usize) {
    println!("\n==========================================");
    println!("Testing game: {}", game_name);
    let mut bus = CdiBus::new();
    let bios_path = find_file("bios/cdi220b.rom").expect("BIOS cdi220b.rom not found");
    bus.load_rom(&bios_path).unwrap();

    if let Some(nvram_path) = find_file("bios/nvram_mk48t08.bin") {
        let _ = bus.nvram.load_from_file(&nvram_path);
    }

    let game_path = find_file(game_name).expect("Game disc not found");
    bus.load_cdrom(&game_path).unwrap();

    bus.set_video_mode(crate::mcd212::VideoMode::Pal);

    let mut cpu = CpuCore::new();
    cpu.set_cpu_type(CpuType::SCC68070);
    cpu.reset(&mut bus);

    let start_time = Instant::now();
    let mut shell_ready_count = 0;
    let mut frame_durations = Vec::with_capacity(frames);
    let mut last_dcr0 = bus.mcd212.dcr[0];

    for frame in 1..=frames {
        let f_start = Instant::now();
        let play_a6 = 0x0000C000u32;
        let c390 = bus.read_long(play_a6.wrapping_add(0xFFFFC390));
        if (c390 == 1 && shell_ready_count == 0) || frame == 865 {
            shell_ready_count = 1;
            println!(">>> [Frame {}] Triggering Disc Launch!", frame);
            bus.write_long(play_a6.wrapping_add(0xFFFFC364), 0x00000500);
            bus.write_long(play_a6.wrapping_add(0xFFFF9172), 2);
            bus.write_long(play_a6.wrapping_add(0xFFFF91A6), 1);
            bus.write_long(play_a6.wrapping_add(0xFFFFC344), 29);
        }

        bus.slave.video_mode = bus.mcd212.video_mode;
        let total_lines = bus.mcd212.video_mode.total_scanlines();
        let cyc_per_line = bus.mcd212.video_mode.cycles_per_scanline();
        let mut cycle_debt: i32 = 0;

        for _line in 0..total_lines {
            bus.mcd212.tick_scanline(_line);

            if bus.mcd212.vblank_irq {
                bus.scc_timer.lir |= 0x80;
            }
            let int1_lvl = if (bus.scc_timer.lir & 0x80) != 0 {
                let lvl = (bus.scc_timer.lir >> 4) & 7;
                if lvl != 0 { lvl } else { 5 }
            } else { 0 };
            let timer_lvl = if bus.scc_timer.timer_irq {
                let lvl = bus.scc_timer.picr1 & 7;
                if lvl != 0 { lvl } else { 6 }
            } else { 0 };
            let cdic_lvl = if bus.cdic.irq_pending { 4 } else { 0 };
            let slave_lvl = if bus.slave.irq_pending { 2 } else { 0 };
            let highest_ipl = int1_lvl.max(timer_lvl).max(cdic_lvl).max(slave_lvl);
            cpu.set_irq(highest_ipl);

            let mut line_cycles = -cycle_debt;
            while line_cycles < cyc_per_line as i32 {
                let remaining = (cyc_per_line as i32 - line_cycles).max(1);
                let budget = remaining.min(32);
                bus.current_pc = cpu.pc;
                bus.current_line_cycle = line_cycles.max(0) as u32;
                let batch = cpu.run_for_cycles(&mut bus, budget);
                line_cycles += batch.cycles + bus.consume_wait_cycles();

                match batch.exit {
                    m68k::CycleBatchExit::TrapInstruction { trap_num } => {
                        line_cycles += cpu.take_trap_exception(&mut bus, trap_num);
                    }
                    m68k::CycleBatchExit::AlineTrap { .. } => {
                        line_cycles += cpu.take_aline_exception(&mut bus);
                    }
                    m68k::CycleBatchExit::FlineTrap { .. } => {
                        line_cycles += cpu.take_fline_exception(&mut bus);
                    }
                    m68k::CycleBatchExit::IllegalInstruction { .. } => {
                        line_cycles += cpu.take_illegal_exception(&mut bus);
                    }
                    m68k::CycleBatchExit::Breakpoint { .. } => {
                        line_cycles += cpu.take_bkpt_exception(&mut bus);
                    }
                    m68k::CycleBatchExit::Stopped => {
                        line_cycles = cyc_per_line as i32;
                        break;
                    }
                    _ => {}
                }
            }

            cycle_debt = (line_cycles - cyc_per_line as i32).clamp(0, 100);

            // Clock real-time peripherals (CDIC, SCC Timer, Slave MCU) on the CPU clock
            bus.tick_cdic(cyc_per_line);
            bus.tick_timer(cyc_per_line);
        }

        if frame == 2000 {
            println!("Plane: order={}, icm=0x{:06X}, tp=0x{:06X}",
                bus.mcd212.plane_order, bus.mcd212.image_coding_method, bus.mcd212.transparency_control);
            println!("Timer: tsr=0x{:02X}, tcr=0x{:02X}, reload=0x{:04X}, picr1=0x{:02X}, picr2=0x{:02X}, cdic_irq={}, slave_irq={}",
                bus.scc_timer.tsr, bus.scc_timer.tcr, bus.scc_timer.reload, bus.scc_timer.picr1, bus.scc_timer.picr2, bus.cdic.irq_pending, bus.slave.irq_pending);
            println!("--- Disassembly around 0x0007A500..0x0007A550 ---");
            let mut p = 0x0007A500u32;
            while p < 0x0007A550 {
                let op = bus.read_word(p);
                let (dis, len) = m68k::dasm::disassemble(p, op, m68k::CpuType::SCC68070);
                let bytes: Vec<String> = (0..len).map(|k| format!("{:02X}", bus.read_byte(p + k as u32))).collect();
                println!("0x{:06X}: {:<16} {:<24}", p, bytes.join(" "), dis);
                p += len as u32;
            }
        }
        if frame >= 2000 && frame <= 2006 {
            println!("Frame {}: DCR0=0x{:04X} DCR1=0x{:04X} VSR0=0x{:04X} VSR1=0x{:04X} CSR1R=0x{:02X} LIR=0x{:02X} PC=0x{:08X}",
                frame, bus.mcd212.dcr[0], bus.mcd212.dcr[1], bus.mcd212.vsr[0], bus.mcd212.vsr[1], bus.mcd212.csrr[0], bus.scc_timer.lir, cpu.pc);
            let path = format!("debug_striker_frame_{}.bmp", frame);
            let width = mcd212::SCREEN_WIDTH as i32;
            let height = mcd212::SCREEN_HEIGHT as i32;
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
            for &pixel in &bus.mcd212.framebuffer[..] {
                let b = (pixel & 0xFF) as u8;
                let g = ((pixel >> 8) & 0xFF) as u8;
                let r = ((pixel >> 16) & 0xFF) as u8;
                bmp.push(b);
                bmp.push(g);
                bmp.push(r);
                bmp.push(0xFF);
            }
            let _ = std::fs::write(&path, &bmp);
        }
        if bus.mcd212.dcr[0] != last_dcr0 {
            println!("Frame {}: DCR0 changed from 0x{:04X} to 0x{:04X} at PC=0x{:08X}", frame, last_dcr0, bus.mcd212.dcr[0], cpu.pc);
            last_dcr0 = bus.mcd212.dcr[0];
        }

        let f_elapsed = f_start.elapsed();
        frame_durations.push(f_elapsed);

        if frame % 200 == 0 {
            let last_200: std::time::Duration = frame_durations[frame - 200..frame].iter().sum();
            let avg_ms = (last_200.as_secs_f64() / 200.0) * 1000.0;
            let sim_fps = 200.0 / last_200.as_secs_f64();
            println!("Frame {:4}: avg sim time = {:.2} ms/frame ({:.1} sim FPS) | mode={:?} DCR0=0x{:04X} DCR1=0x{:04X} LBA={}",
                frame, avg_ms, sim_fps, bus.mcd212.video_mode, bus.mcd212.dcr[0], bus.mcd212.dcr[1], bus.cdic.curr_lba);
        }
    }

    let total_time = start_time.elapsed();
    println!("Completed {} frames in {:.2}s ({:.1} FPS)", frames, total_time.as_secs_f64(), frames as f64 / total_time.as_secs_f64());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let game = if args.len() > 1 { &args[1] } else { "giochi/Striker Pro (USA).chd" };
    let frames = if args.len() > 2 { args[2].parse().unwrap_or(1000) } else { 1000 };
    test_game(game, frames);
}

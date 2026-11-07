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

fn save_bmp(path: &str, framebuffer: &[u32]) {
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
    for &pixel in framebuffer {
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

fn main() {
    let bios_path = find_file("bios/cdi220b.rom").expect("BIOS cdi220b.rom not found");
    let game_path = find_file("giochi/Rise of the Robots (Europe).chd").expect("Game disc not found");

    let mut bus = CdiBus::new();
    bus.load_rom(&bios_path).unwrap();
    if let Some(nvram_path) = find_file("bios/nvram_mk48t08.bin") {
        let _ = bus.nvram.load_from_file(&nvram_path);
    }
    bus.load_cdrom(&game_path).unwrap();
    bus.set_video_mode(crate::mcd212::VideoMode::Pal);

    bus.watch_addr1 = 0;
    bus.watch_addr2 = 0;

    let mut cpu = CpuCore::new();
    cpu.set_cpu_type(CpuType::SCC68070);
    cpu.reset(&mut bus);

    let max_frames = 4000;
    println!("Starting Rise of the Robots trace up to {} frames...", max_frames);

    let mut shell_ready_count = 0;
    let mut prev_vsr0 = 0u32;
    let mut prev_flip_frame = 0usize;
    let mut trace_trap_count = 0usize;
    let mut combat_vblanks = 0u32;
    let mut combat_frame_count = 0u32;

    for frame in 1..=max_frames {
        // BIOS Shell auto-click
        let play_a6 = 0x0000C000u32;
        let shell_state = bus.read_long(play_a6.wrapping_add(0xFFFF9172));
        if shell_state == 2 {
            shell_ready_count += 1;
            if shell_ready_count == 30 {
                bus.slave.update_mouse(575, 335, 1);
            } else if shell_ready_count == 50 {
                bus.slave.update_mouse(575, 335, 0);
            } else if shell_ready_count >= 120 {
                bus.write_long(play_a6.wrapping_add(0xFFFFC344), 29);
                shell_ready_count = 0;
            }
        }

        // Test skipping intro movie
        if frame == 2000 {
            bus.slave.update_mouse(384, 280, 1);
            println!("Frame 2000: SENDING BUTTON 1 CLICK TO SKIP INTRO MOVIE!");
        } else if frame == 2015 {
            bus.slave.update_mouse(384, 280, 0);
        }

        if frame == 3000 {
            for (title, start, end) in [("timing routine", 0x00273300u32, 0x002733C0u32), ("ticks consumer", 0x0026ECF0u32, 0x0026ED40u32)] {
                println!("=== DISASM {} 0x{:06X}..0x{:06X} ===", title, start, end);
                let mut p = start;
                while p < end {
                    let opcode = bus.read_word(p);
                    let (text, len) = m68k::dasm::disassemble(p, opcode, m68k::CpuType::SCC68070);
                    let l = (len as u32).max(2);
                    print!("  0x{:06X}: ", p);
                    for k in 0..(l / 2) {
                        print!("{:04X} ", bus.read_word(p + k * 2));
                    }
                    println!("  {}", text);
                    p += l;
                }
            }
        }

        if frame >= 3000 && frame <= 3030 {
            let dcr0 = bus.mcd212.dcr[0];
            let dcr1 = bus.mcd212.dcr[1];
            let vsr0 = bus.mcd212.vsr[0];
            let vsr1 = bus.mcd212.vsr[1];
            println!("Frame {:4}: DCR0=0x{:04X} DCR1=0x{:04X} VSR0=0x{:04X} VSR1=0x{:04X}",
                frame, dcr0, dcr1, vsr0, vsr1);
        }

        if frame == 1410 {
            println!("=== FRAME 1410 CDIC INSPECTION ===");
            println!("CDIC: cmd=0x{:04X}, file=0x{:04X}, chan=0x{:08X}, audio_chan=0x{:04X}, disc_cmd={}, disc_mode={}, curr_lba={}",
                bus.cdic.command, bus.cdic.file, bus.cdic.channel, bus.cdic.audio_channel,
                bus.cdic.disc_command, bus.cdic.disc_mode, bus.cdic.curr_lba);
            
            if let Some(ref mut cd) = bus.cdrom {
                let mut sbuf = [0u8; crate::cdrom::CD_SECTOR_RAW_SIZE];
                for l in 0..10000 {
                    if cd.read_sector(l, &mut sbuf) {
                        if let Some(pos) = sbuf.windows(7).position(|w| w == b"cin8800") {
                            println!("Found 'cin8800' in LBA {} at offset {}!", l, pos);
                            let start = pos.saturating_sub(32);
                            let end = (pos + 32).min(sbuf.len());
                            print!("  Hex:");
                            for b in &sbuf[start..end] { print!(" {:02X}", b); }
                            println!();
                        }
                    }
                }
            }

            println!("=== DISASSEMBLY OF 0x0042B850..0x0042B980 ===");
            for addr in (0x0042B850u32..0x0042B980).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }
            for addr in (0x0026D3E0u32..0x0026D500).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }
            for addr in (0x0042A420u32..0x0042A580).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }

            println!("=== DISASSEMBLY OF h_xbuf (0x0042A360..0x0042A430) ===");
            for addr in (0x0042A360u32..0x0042A430).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }
            println!("=== DISASSEMBLY OF caller at 0x0026ECA4 ===");
            for addr in (0x0026EC80u32..0x0026ED20).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }

            println!("=== DISASSEMBLY OF cdapdriv at 0x0042B040..0x0042B140 ===");
            for addr in (0x0042B040u32..0x0042B140).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }

            println!("=== DISASSEMBLY OF 0x0026C740..0x0026C7B0 ===");
            for addr in (0x0026C740u32..0x0026C7B0).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }

            println!("=== DISASSEMBLY OF 0x0026EC00..0x0026EE00 ===");
            for addr in (0x0026EC00u32..0x0026EE00).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }
            println!("=== DISASSEMBLY OF 0x0026CE80..0x0026CF20 ===");
            for addr in (0x0026CE80u32..0x0026CF20).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }

            println!("=== DISASSEMBLY OF 0x0026DDFC..0x0026DF00 ===");
            for addr in (0x0026DDF0u32..0x0026DF00).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }
            println!("=== SEARCH FOR 0xA70C, 0xA709, 0xA6BE IN PLANE B ===");
            for addr in (0x00200002u32..0x00280000).step_by(2) {
                let w = bus.read_word(addr);
                if w == 0xA70C || w == 0xA709 || w == 0xA690 {
                    let prev = bus.read_word(addr - 2);
                    println!("Found 0x{:04X} at 0x{:08X}: op=0x{:04X}", w, addr, prev);
                }
            }
            println!("=== DISASSEMBLY OF 0x00273320..0x002733E0 ===");
            for addr in (0x00273320u32..0x002733E0).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }
            println!("=== DISASSEMBLY OF 0x002731E0..0x00273250 ===");
            for addr in (0x002731E0u32..0x00273250).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }
            println!("=== DISASSEMBLY OF 0x00273440..0x00273540 ===");
            for addr in (0x00273440u32..0x00273540).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }
            println!("=== DISASSEMBLY OF 0x00273800..0x00273850 ===");
            for addr in (0x00273800u32..0x00273850).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }
            println!("=== DISASSEMBLY OF cdapdriv at 0x0042B860..0x0042B8C0 ===");
            for addr in (0x0042B860u32..0x0042B8C0).step_by(16) {
                let w0 = bus.read_word(addr);
                let w1 = bus.read_word(addr + 2);
                let w2 = bus.read_word(addr + 4);
                let w3 = bus.read_word(addr + 6);
                let w4 = bus.read_word(addr + 8);
                let w5 = bus.read_word(addr + 10);
                let w6 = bus.read_word(addr + 12);
                let w7 = bus.read_word(addr + 14);
                println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                    addr, w0, w1, w2, w3, w4, w5, w6, w7);
            }
        }        // Trace interrupts and traps during frames 1400..1402 and 3100..3102
        let trace_detail = (frame >= 1400 && frame <= 1402) || (frame >= 3100 && frame <= 3102);
        let mut int1_count = 0;
        let mut timer_count = 0;
        let mut cdic_count = 0;
        let mut slave_count = 0;

        bus.slave.video_mode = bus.mcd212.video_mode;
        let total_lines = bus.mcd212.video_mode.total_scanlines();
        let cyc_per_line = bus.mcd212.video_mode.cycles_per_scanline();
        let mut cycle_debt: i32 = 0;

        for _line in 0..total_lines {
            if _line == 0 {
                combat_vblanks += 1;
            }
            bus.tick_scanline(_line);

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

            if trace_detail {
                if int1_lvl > 0 { int1_count += 1; }
                if timer_lvl > 0 { timer_count += 1; }
                if cdic_lvl > 0 { cdic_count += 1; }
                if slave_lvl > 0 { slave_count += 1; }
            }

            cpu.set_irq(highest_ipl);

            let mut line_cycles = -cycle_debt;
            while line_cycles < cyc_per_line as i32 {
                let remaining = (cyc_per_line as i32 - line_cycles).max(1);
                let budget = if trace_trap_count > 0 || trace_detail { 1 } else { remaining.min(32) };
                if trace_trap_count > 0 {
                    trace_trap_count -= 1;
                    println!("TRAP_TRACE: PC=0x{:08X}, op=0x{:04X}, D0=0x{:08X}, D1=0x{:08X}, D2=0x{:08X}, D3=0x{:08X}, A0=0x{:08X}, A1=0x{:08X}, A2=0x{:08X}, A4=0x{:08X}",
                        cpu.pc, bus.read_word(cpu.pc), cpu.d(0), cpu.d(1), cpu.d(2), cpu.d(3), cpu.a(0), cpu.a(1), cpu.a(2), cpu.a(4));
                }
                if cpu.pc == 0x00273808 || cpu.pc == 0x0027380A {
                    let a6 = cpu.a(6);
                    let ea = a6.wrapping_add(0xFFFF8A7Bu32);
                    if combat_vblanks < 3 {
                        bus.write_byte(ea, 0xFF);
                    } else {
                        bus.write_byte(ea, 0x00);
                    }
                } else if cpu.pc == 0x0027380E {
                    if combat_vblanks >= 3 {
                        combat_vblanks = 0;
                        combat_frame_count += 1;
                        if combat_frame_count % 10 == 0 {
                            println!("Frame {}: COMBAT FRAME #{} EXECUTED (pace = 3 VBLANKs, ~16.6 fps)!", frame, combat_frame_count);
                        }
                    }
                }
                if (cpu.pc >= 0x0026ECE8 && cpu.pc <= 0x0026ECF0) && frame >= 1399 && frame <= 1405 {
                    println!("Frame {} line {}: AT 0x{:08X}! SP=0x{:08X}, ret=0x{:08X}, ret2=0x{:08X}",
                        frame, _line, cpu.pc, cpu.a(7), bus.read_long(cpu.a(7)), bus.read_long(cpu.a(7) + 4));
                }

                if frame == 1424 && _line == 170 {
                    println!("=== MEMORY DUMP AT 0x00004880..0x00004980 (A6=0x{:08X}) ===", cpu.a(6));
                    for addr in (0x00004880u32..0x00004980).step_by(16) {
                        let w0 = bus.read_word(addr);
                        let w1 = bus.read_word(addr + 2);
                        let w2 = bus.read_word(addr + 4);
                        let w3 = bus.read_word(addr + 6);
                        let w4 = bus.read_word(addr + 8);
                        let w5 = bus.read_word(addr + 10);
                        let w6 = bus.read_word(addr + 12);
                        let w7 = bus.read_word(addr + 14);
                        println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                            addr, w0, w1, w2, w3, w4, w5, w6, w7);
                    }
                }
                if (frame >= 1100 && frame <= 1105) || (frame >= 1400 && frame <= 1426) || (frame >= 1950 && frame <= 2050) {
                    if cpu.pc == 0x0042B048 {
                        let d0 = cpu.d(0);
                        let a2 = cpu.a(2);
                        let a3 = cpu.a(3);
                        let dbuf = bus.cdic.data_buffer;
                        let xbuf = bus.cdic.x_buffer;
                        let zbuf = bus.cdic.z_buffer;
                        let abuf = bus.cdic.audio_buffer;
                        let pid = bus.read_word(a2.wrapping_add(8));
                        let h_xbuf = bus.read_long(a2.wrapping_add(0x8E));
                        let h_abuf = bus.read_long(a2.wrapping_add(0x128));
                        println!("Frame {} line {}: ENTER cdapdriv CDIC ISR (0x0042B048)! A2=0x{:08X}, A3=0x{:08X}, DBUF=0x{:04X}, XBUF=0x{:04X}, ZBUF=0x{:04X}, ABUF=0x{:04X}, PID=0x{:04X}, h_xbuf=0x{:08X}, h_abuf=0x{:08X}",
                            frame, _line, a2, a3, dbuf, xbuf, zbuf, abuf, pid, h_xbuf, h_abuf);
                    } else if cpu.pc == 0x0042B08E {
                        println!("Frame {} line {}: cdapdriv ISR -> XBUF path (0x0042B08E)!", frame, _line);
                        if trace_trap_count == 0 {
                            trace_trap_count = 60;
                        }
                    } else if cpu.pc == 0x0042B068 {
                        println!("Frame {} line {}: cdapdriv ISR -> checking ZBUF/ABUF (0x0042B068)!", frame, _line);
                    } else if cpu.pc == 0x0042B0B0 {
                        println!("Frame {} line {}: cdapdriv ISR -> ABUF path (0x0042B0B0)!", frame, _line);
                    } else if cpu.pc == 0x0042B0BE {
                        println!("Frame {} line {}: cdapdriv ISR -> SIGNAL 1 SENDER (0x0042B0BE)!", frame, _line);
                    }
                    if cpu.pc == 0x0041C120 && trace_trap_count == 0 {
                        println!("Frame {} line {}: AT 0x0041C120 (SetStat sub=0x0A entry)! A0=0x{:08X}, A1=0x{:08X}, A2=0x{:08X}, D0=0x{:08X}, D1=0x{:08X}, D2=0x{:08X}, D3=0x{:08X}",
                            frame, _line, cpu.a(0), cpu.a(1), cpu.a(2), cpu.d(0), cpu.d(1), cpu.d(2), cpu.d(3));
                        trace_trap_count = 80;
                    }
                    if cpu.pc == 0x0042A420 {
                        println!("Frame {} line {}: ENTER cdapdriv ISR at 0x0042A420! A1=0x{:08X}, A2=0x{:08X}, A3=0x{:08X}, A4=0x{:08X}",
                            frame, _line, cpu.a(1), cpu.a(2), cpu.a(3), cpu.a(4));
                    } else if cpu.pc == 0x0042A442 {
                        println!("Frame {} line {}: AT 0x0042A442 (calling 0x0042A4DE)! A2=0x{:08X}, A4=0x{:08X}",
                            frame, _line, cpu.a(2), cpu.a(4));
                    } else if cpu.pc == 0x0042A4DE {
                        let a2 = cpu.a(2);
                        let a4 = cpu.a(4);
                        let val94 = bus.read_word(a2.wrapping_add(0x94));
                        let val4_a4 = bus.read_long(a4.wrapping_add(4));
                        let val0_a4 = bus.read_byte(a4);
                        let val1_a4 = bus.read_byte(a4.wrapping_add(1));
                        let val2_a4 = bus.read_word(a4.wrapping_add(2));
                        let val8_a4 = bus.read_long(a4.wrapping_add(8));
                        let val12_a4 = bus.read_long(a4.wrapping_add(12));
                        println!("Frame {} line {}: ENTER 0x0042A4DE! $94(A2)=0x{:04X}, A4=0x{:08X}: [0]=0x{:02X}, [1]=0x{:02X}, [2..3]=0x{:04X}, [4..7]=0x{:08X}, [8..11]=0x{:08X}, [12..15]=0x{:08X}",
                            frame, _line, val94, a4, val0_a4, val1_a4, val2_a4, val4_a4, val8_a4, val12_a4);
                    } else if cpu.pc == 0x0042A510 {
                        println!("Frame {} line {}: 0x0042A4DE AT 0x0042A510 (BTST #0)", frame, _line);
                    } else if cpu.pc == 0x0042A520 {
                        println!("Frame {} line {}: 0x0042A4DE EXIT EARLY to 0x0042A520!", frame, _line);
                    } else if cpu.pc == 0x0042A52A {
                        println!("Frame {} line {}: 0x0042A4DE AT 0x0042A52A (BTST #4)", frame, _line);
                    } else if cpu.pc == 0x0042A53A {
                        println!("Frame {} line {}: 0x0042A4DE BRANCHED TO 0x0042A53A (ARMING SIGNAL)!", frame, _line);
                    }
                }

                bus.current_pc = cpu.pc;
                bus.current_line_cycle = line_cycles.max(0) as u32;
                let batch_start_cycles = line_cycles;
                let batch = cpu.run_for_cycles(&mut bus, budget);
                line_cycles += batch.cycles + bus.consume_wait_cycles();

                match batch.exit {
                    m68k::CycleBatchExit::TrapInstruction { trap_num } => {
                        let next_word = bus.read_word(cpu.pc);
                        if next_word == 0x0008 {
                            // F$Send
                            let sig = cpu.d(1) as u16;
                            if sig == 0x0102 {
                                let ret1 = bus.read_long(cpu.a(7));
                                let ret2 = bus.read_long(cpu.a(7) + 4);
                                let ret3 = bus.read_long(cpu.a(7) + 8);
                                println!("Frame {} line {}: F$Send 0x0102 to D0=0x{:04X} | Stack: ret1=0x{:08X}, ret2=0x{:08X}, ret3=0x{:08X}",
                                    frame, _line, cpu.d(0) as u16, ret1, ret2, ret3);
                            }
                        } else if next_word == 0x008E {
                            // I$SetStt
                            let path = cpu.d(0) as u16;
                            let code = cpu.d(1) as u16;
                            let sub = cpu.d(2) as u16;
                            let param = cpu.d(3);
                            if code == 0x0056 && sub == 0x0011 && (cpu.d(3) as u16) == 0x00FF {
                                println!("Frame {} line {}: SUB11 ticks(D4)={}", frame, _line, cpu.d(4) as u16);
                            }
                            if ((frame >= 1400 && frame <= 1401) || (frame >= 3000 && frame <= 3005)) && sub == 0x0011 {
                                println!("Frame {} line {}: I$SetStt sub=0x11: D0=0x{:X}, D1=0x{:X}, D2=0x{:X}, D3=0x{:X}, D4=0x{:X} PC=0x{:08X}",
                                    frame, _line, cpu.d(0), cpu.d(1), cpu.d(2), cpu.d(3), cpu.d(4), cpu.pc);
                            }
                            if frame == 1400 || frame == 2100 || (sub != 0x0011 && sub != 0x0004 && sub != 0x0002) {
                                println!("Frame {} line {}: I$SetStt from PC=0x{:08X}, path=0x{:04X}, code=0x{:04X}, sub=0x{:04X}, param=0x{:08X}",
                                    frame, _line, cpu.pc.saturating_sub(2), path, code, sub, param);
                            }
                            if frame == 1424 && code == 0x0056 && sub == 0x000A && trace_trap_count == 0 {
                                println!(">>> START TRACE TRAP at frame {} line {} for I$SetStt sub=0x0A, param=0x{:08X}", frame, _line, param);
                                trace_trap_count = 150;
                            }
                        } else if next_word == 0x0084 {
                            // I$Open
                            let mut path_chars = Vec::new();
                            for i in 0..64 {
                                let b = bus.read_byte(cpu.a(0).wrapping_add(i));
                                if b == 0 || b == 0x0D || b == 0x0A { break; }
                                path_chars.push(b as char);
                            }
                            let path_name: String = path_chars.into_iter().collect();
                            println!("Frame {} line {}: I$Open from PC=0x{:08X}, name='{}', mode=0x{:04X}",
                                frame, _line, cpu.pc.saturating_sub(2), path_name, cpu.d(0) as u16);
                        } else if next_word == 0x0088 {
                            // I$Seek
                            println!("Frame {} line {}: I$Seek from PC=0x{:08X}, path=0x{:04X}, pos={}",
                                frame, _line, cpu.pc.saturating_sub(2), cpu.d(0) as u16, cpu.d(1));
                        } else if next_word == 0x0089 {
                            // I$Read
                            println!("Frame {} line {}: I$Read from PC=0x{:08X}, path=0x{:04X}, count={}, buf=0x{:08X}",
                                frame, _line, cpu.pc.saturating_sub(2), cpu.d(0) as u16, cpu.d(1), cpu.a(0));
                        } else if next_word == 0x0029 {
                            // F$Alarm
                            println!("Frame {} line {}: F$Alarm from PC=0x{:08X}, D0=0x{:08X}, D1=0x{:08X}",
                                frame, _line, cpu.pc.saturating_sub(2), cpu.d(0), cpu.d(1));
                        }



                        if trace_detail {
                            println!("Frame {} line {}: TRAP #{} at PC=0x{:08X}, next_word=0x{:04X}", frame, _line, trap_num, cpu.pc, next_word);
                        }
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
                        bus.tick_timer((cyc_per_line as i32 - line_cycles).max(0) as u32);
                        line_cycles = cyc_per_line as i32;
                        break;
                    }
                    _ => {}
                }

                // SCC68070 peripheral timers advance with the CPU, cycle by cycle.
                bus.tick_timer((line_cycles - batch_start_cycles).max(0) as u32);
                let timer_lvl = if bus.scc_timer.timer_irq {
                    let lvl = bus.scc_timer.picr1 & 7;
                    if lvl != 0 { lvl } else { 6 }
                } else { 0 };
                let cdic_lvl = if bus.cdic.irq_pending || bus.dvc.irq_pending() { 4 } else { 0 };
                let slave_lvl = if bus.slave.irq_pending { 2 } else { 0 };
                cpu.set_irq(int1_lvl.max(timer_lvl).max(cdic_lvl).max(slave_lvl));
            }

            cycle_debt = (line_cycles - cyc_per_line as i32).clamp(0, 100);
            bus.render_scanline(_line);

            // Clock real-time peripherals (Slave MCU, CDIC, DVC) on the CPU clock
            bus.tick_slave(cyc_per_line);
            bus.tick_cdic(cyc_per_line);
            bus.tick_dvc(cyc_per_line);
        }

        if trace_detail {
            println!("Frame {}: int1_lines={}, timer_lines={}, cdic_lines={}, slave_lines={}",
                frame, int1_count, timer_count, cdic_count, slave_count);
        }

        // Check if framebuffer has non-black content
        let non_black_count = bus.mcd212.framebuffer.iter().filter(|&&p| (p & 0x00FFFFFF) != 0).count();

        if frame % 250 == 0 || (frame > 1500 && frame % 100 == 0) {
            println!("Frame {:4}: PC=0x{:08X} DCR0=0x{:04X} VSR0=0x{:04X} LBA={} non_black={}",
                frame, cpu.pc, bus.mcd212.dcr[0], bus.mcd212.vsr[0], bus.cdic.curr_lba, non_black_count);
        }

        if frame == 100 || frame == 1100 || frame == 1500 || frame == 1900 {
            println!("--- Frame {} Vector Status ---", frame);
            println!("CDIC interrupt_vector register: 0x{:04X}", bus.cdic.interrupt_vector);
            println!("Vector table in RAM:");
            println!("  0x60 (Spurious):  0x{:08X}", bus.read_long(0x60));
            println!("  0x64 (AutoVec 1): 0x{:08X}", bus.read_long(0x64));
            println!("  0x68 (AutoVec 2): 0x{:08X}", bus.read_long(0x68));
            println!("  0x6C (AutoVec 3): 0x{:08X}", bus.read_long(0x6C));
            println!("  0x70 (AutoVec 4): 0x{:08X}", bus.read_long(0x70));
            println!("  0x74 (AutoVec 5): 0x{:08X}", bus.read_long(0x74));
            println!("  0x78 (AutoVec 6): 0x{:08X}", bus.read_long(0x78));
            let vec_num = (bus.cdic.interrupt_vector & 0xFF) as u32;
            println!("  User vec 0x{:02X} (addr 0x{:04X}): 0x{:08X}",
                vec_num, vec_num * 4, bus.read_long(vec_num * 4));

            if frame == 100 {
                println!("=== DISASSEMBLY OF 0x00407F90..0x00407FD0 ===");
                for addr in (0x00407F90u32..0x00407FD0).step_by(16) {
                    let w0 = bus.read_word(addr);
                    let w1 = bus.read_word(addr + 2);
                    let w2 = bus.read_word(addr + 4);
                    let w3 = bus.read_word(addr + 6);
                    let w4 = bus.read_word(addr + 8);
                    let w5 = bus.read_word(addr + 10);
                    let w6 = bus.read_word(addr + 12);
                    let w7 = bus.read_word(addr + 14);
                    println!("  0x{:08X}: {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
                        addr, w0, w1, w2, w3, w4, w5, w6, w7);
                }
                let mut mod_addr = 0x0042A000u32;
                while mod_addr >= 0x00410000 {
                    if bus.read_word(mod_addr) == 0x4AFB {
                        let name_off = bus.read_long(mod_addr + 4) as u32; // size
                        let name_ptr = bus.read_long(mod_addr + 12) as u32;
                        let exec_off = bus.read_long(mod_addr + 20) as u32;
                        let mut name_chars = Vec::new();
                        for i in 0..32 {
                            let b = bus.read_byte(mod_addr + name_ptr + i);
                            if b == 0 || b == 0x0D { break; }
                            name_chars.push(b as char);
                        }
                        let mname: String = name_chars.into_iter().collect();
                        println!("Found OS-9 Module at 0x{:08X}: name='{}', exec=0x{:08X}", mod_addr, mname, mod_addr + exec_off);
                        if mname.contains("cd") {
                            let dispatch = mod_addr + exec_off;
                            let init = bus.read_word(dispatch);
                            let read = bus.read_word(dispatch + 2);
                            let write = bus.read_word(dispatch + 4);
                            let getstt = bus.read_word(dispatch + 6);
                            let setstt = bus.read_word(dispatch + 8);
                            let term = bus.read_word(dispatch + 10);
                            println!("  Dispatch table: init=+0x{:04X}, read=+0x{:04X}, write=+0x{:04X}, getstt=+0x{:04X}, setstt=+0x{:04X} (0x{:08X}), term=+0x{:04X}",
                                init, read, write, getstt, setstt, dispatch + (setstt as i16 as i32 as u32), term);
                        }
                    }
                    mod_addr -= 2;
                }
            }
        }

        if (frame >= 1400 && frame <= 1600 && frame % 20 == 0) || (frame >= 2000 && frame <= 4000 && frame % 50 == 0) {
            let path = format!("robots_frame_{:04}.bmp", frame);
            save_bmp(&path, &bus.mcd212.framebuffer[..]);
            println!("==> Saved screenshot: {}", path);
        }

    }
}

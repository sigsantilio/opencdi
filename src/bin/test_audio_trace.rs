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
use m68k::{CpuCore, CpuType, AddressBus};
use crate::bus::CdiBus;
use crate::mcd212::{TOTAL_SCANLINES, CYCLES_PER_SCANLINE};

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    println!("=== OpenCDi - Audio Trace Test ===");

    let mut bus = CdiBus::new();

    let bios_path = find_file("bios/cdi220b.rom").expect("BIOS cdi220b.rom not found");
    bus.load_rom(&bios_path)?;

    if let Some(nvram_path) = find_file("bios/nvram_mk48t08.bin") {
        let _ = bus.nvram.load_from_file(&nvram_path);
    }

    let game_path = find_file("giochi/Hotel Mario (USA).chd").expect("Game disc not found");
    bus.load_cdrom(&game_path)?;

    let mut cpu = CpuCore::new();
    cpu.set_cpu_type(CpuType::SCC68070);
    cpu.reset(&mut bus);

    for frame in 1..=4500 {
        let play_a6 = 0x0000C000u32;

        if frame == 530 {
            println!("Vec 128 (0x200) = 0x{:08X}", bus.read_long(0x200));
            println!("Vec 36  (0x090) = 0x{:08X}", bus.read_long(0x090));
            println!("Vec 28  (0x070) = 0x{:08X}", bus.read_long(0x070));
            let v128_addr = bus.read_long(0x200);
            println!("At 0x{:06X}: {:08X} {:08X} {:08X}", v128_addr, bus.read_long(v128_addr), bus.read_long(v128_addr+4), bus.read_long(v128_addr+8));
            println!("At 0x654: {:08X} {:08X} {:08X}", bus.read_long(0x654), bus.read_long(0x658), bus.read_long(0x65C));
        }


        if frame == 100 {
            println!("=== RAM BYTES 0x600..0x700 ===");
            for row in (0x600..0x700).step_by(16) {
                let bytes: Vec<String> = (0..16).map(|i| format!("{:02X}", bus.read_byte(row + i))).collect();
                println!("0x{:04X}: {}", row, bytes.join(" "));
            }
        }

        if frame == 865 {
            bus.write_long(play_a6.wrapping_add(0xFFFFC364), 0x00000500);
            bus.write_long(play_a6.wrapping_add(0xFFFF9172), 2);
            bus.write_long(play_a6.wrapping_add(0xFFFF91A6), 1);
            bus.write_long(play_a6.wrapping_add(0xFFFFC344), 29);
        }

        if frame == 1650 { bus.slave.update_mouse(384, 280, 1); }
        if frame == 1660 { bus.slave.update_mouse(384, 280, 0); }

        if frame == 2100 { bus.slave.update_mouse(384, 280, 1); }
        if frame == 2110 { bus.slave.update_mouse(384, 280, 0); }

        if frame == 2650 { bus.slave.update_mouse(384, 280, 1); }
        if frame == 2670 { bus.slave.update_mouse(384, 280, 0); }

        if frame == 2750 { bus.slave.update_mouse(384, 280, 1); }
        if frame == 2770 { bus.slave.update_mouse(384, 280, 0); }

        if frame == 3850 {
            println!(">>> [Frame 3850] Pressing Button 1 to enter stage 1 gameplay!");
            bus.slave.update_mouse(384, 280, 1);

            println!("=== DISASSEMBLY AT 0x278400..0x278430 ===");
            let mut p = 0x278400u32;
            while p < 0x278430 {
                let op = bus.read_word(p);
                let (dis, len) = m68k::dasm::disassemble(p, op, m68k::CpuType::SCC68070);
                let real_len = (len as usize).max(2).min(8);
                let bytes: Vec<String> = (0..real_len).map(|k| format!("{:02X}", bus.read_byte(p + k as u32))).collect();
                println!("  0x{:06X}: {:14} -> {}", p, bytes.join(" "), dis);
                p += real_len as u32;
            }

        }
        if frame == 3870 {
            bus.slave.update_mouse(384, 280, 0);
        }

        if frame == 2000 {
            let _ = save_bmp(&bus, "C:/Users/Gian/.gemini/antigravity/brain/47421117-6e2a-4536-8928-519a0c9b76ed/framebuffer_mario_2000.bmp");
        }
        if frame == 2600 {
            let _ = save_bmp(&bus, "C:/Users/Gian/.gemini/antigravity/brain/47421117-6e2a-4536-8928-519a0c9b76ed/framebuffer_mario_2600.bmp");
        }
        if frame == 3800 {
            let _ = save_bmp(&bus, "C:/Users/Gian/.gemini/antigravity/brain/47421117-6e2a-4536-8928-519a0c9b76ed/framebuffer_mario_3800.bmp");
        }
        if frame == 4000 {
            let _ = save_bmp(&bus, "C:/Users/Gian/.gemini/antigravity/brain/47421117-6e2a-4536-8928-519a0c9b76ed/framebuffer_mario_4000.bmp");
        }
        if frame == 4200 {
            let _ = save_bmp(&bus, "C:/Users/Gian/.gemini/antigravity/brain/47421117-6e2a-4536-8928-519a0c9b76ed/framebuffer_mario_4200.bmp");
        }
        if frame == 4400 {
            let _ = save_bmp(&bus, "C:/Users/Gian/.gemini/antigravity/brain/47421117-6e2a-4536-8928-519a0c9b76ed/framebuffer_mario_4400.bmp");
        }

        if frame >= 2600 && frame % 100 == 0 {
            let q_len = bus.cdic.audio.sample_queue.lock().unwrap().len();
            println!(
                "[Frame {}] CDIC State: cmd=0x{:02X}, disc_cmd=0x{:02X}, lba={}, audio_chan=0x{:04X}, chan=0x{:08X}, dec_map={}, dec_addr=0x{:04X}, sec_cnt={}, total_audio_sectors={}, q_len={}",
                frame,
                bus.cdic.command,
                bus.cdic.disc_command,
                bus.cdic.curr_lba,
                bus.cdic.audio_channel,
                bus.cdic.channel,
                bus.cdic.map_on,
                bus.cdic.map_next,
                bus.cdic.audio_sector_counter,
                bus.cdic.audio.total_sectors_decoded,
                q_len
            );
        }

        for _line in 0..TOTAL_SCANLINES {
            if bus.scc_timer.timer_irq {
                cpu.set_irq(6);
            } else if bus.cdic.irq_pending {
                cpu.set_irq(4);
            } else if bus.slave.irq_pending {
                cpu.set_irq(2);
            } else if bus.mcd212.vblank_irq {
                let mcd_lvl = (bus.scc_timer.picr1 >> 4) & 7;
                let irq_lvl = if mcd_lvl != 0 { mcd_lvl } else { 1 };
                cpu.set_irq(irq_lvl);
            } else {
                cpu.set_irq(0);
            }

            let mut line_cycles = 0;
            while line_cycles < CYCLES_PER_SCANLINE as i32 {
                let budget = (CYCLES_PER_SCANLINE as i32 - line_cycles).max(1);
                bus.current_pc = cpu.pc;

                let batch = cpu.run_for_cycles(&mut bus, budget);
                line_cycles += batch.cycles;

                if cpu.pc >= 0x42BA00 && cpu.pc <= 0x42CD00 && (cpu.pc == 0x42BB54 || cpu.pc == 0x42BB7A || cpu.pc == 0x42CACE || cpu.pc == 0x42B6D8) {
                    println!(">>> HIT QUEUE PC=0x{:06X} at F{} L{:3}, D0=0x{:08X}, A0=0x{:08X}, A1=0x{:08X}, A2=0x{:08X}", cpu.pc, frame, _line, cpu.d(0), cpu.a(0), cpu.a(1), cpu.a(2));
                }
                if cpu.pc == 0x42AE50 {
                    let ret = bus.read_long(cpu.sp());
                    println!(">>> REACHED 0x42AE50 (abort) at F{} L{:3}! return_addr=0x{:06X}, D0=0x{:08X}, D1=0x{:08X}, A0=0x{:08X}, A2=0x{:08X}",
                        frame, _line, ret, cpu.d(0), cpu.d(1), cpu.a(0), cpu.a(2));
                }

                match batch.exit {
                    m68k::CycleBatchExit::TrapInstruction { trap_num } => {
                        let call_pc = cpu.ppc;
                        let sys_code = bus.read_word(call_pc + 2);


                        if trap_num == 1 && frame >= 3840 && frame <= 3860 {
                            println!(
                                ">>> [TRAP #1 F{} L{:3}] code=0x{:04X}, D0=0x{:08X}, D1=0x{:08X}, D2=0x{:08X}, A0=0x{:08X}, A1=0x{:08X}, A2=0x{:08X}, caller=0x{:06X}",
                                frame, _line, sys_code, cpu.d(0), cpu.d(1), cpu.d(2), cpu.a(0), cpu.a(1), cpu.a(2), call_pc
                            );
                        }


                        let trap_cyc = cpu.take_trap_exception(&mut bus, trap_num);
                        line_cycles += trap_cyc;
                    }
                    m68k::CycleBatchExit::AlineTrap { .. } => {
                        let trap_cyc = cpu.take_aline_exception(&mut bus);
                        line_cycles += trap_cyc;
                    }
                    m68k::CycleBatchExit::FlineTrap { .. } => {
                        let trap_cyc = cpu.take_fline_exception(&mut bus);
                        line_cycles += trap_cyc;
                    }
                    m68k::CycleBatchExit::IllegalInstruction { .. } => {
                        let trap_cyc = cpu.take_illegal_exception(&mut bus);
                        line_cycles += trap_cyc;
                    }
                    m68k::CycleBatchExit::Breakpoint { .. } => {
                        let trap_cyc = cpu.take_bkpt_exception(&mut bus);
                        line_cycles += trap_cyc;
                    }
                    m68k::CycleBatchExit::Stopped => {
                        line_cycles = CYCLES_PER_SCANLINE as i32;
                        break;
                    }
                    _ => {}
                }
            }

            let spent = (line_cycles as u32).max(CYCLES_PER_SCANLINE);
            bus.mcd212.tick_cycles(spent);
            bus.tick_cdic(spent);
            bus.tick_timer(spent);
        }
    }

    println!("Finished audio trace test! Total audio sectors: {}", bus.cdic.audio.total_sectors_decoded);
    Ok(())
}

fn save_bmp(bus: &bus::CdiBus, path: &str) -> std::io::Result<()> {
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
    bmp.extend_from_slice(&(-height).to_le_bytes()); // top-down
    bmp.extend_from_slice(&1u16.to_le_bytes()); // planes
    bmp.extend_from_slice(&32u16.to_le_bytes()); // 32 bpp
    bmp.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
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

    std::fs::write(path, &bmp)?;
    println!("Saved framebuffer BMP to {:?}", path);
    Ok(())
}


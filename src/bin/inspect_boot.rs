use std::fs;
use std::path::{Path, PathBuf};
use opencdi::bus::CdiBus;
use m68k::{AddressBus, CpuCore, CpuType};

fn find_file(relative: &str) -> Option<PathBuf> {
    for prefix in &["file utili", "../file utili", "../../file utili", "."] {
        let p = Path::new(prefix).join(relative);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn save_bmp(path: &str, buffer: &[u32; 768 * 560]) {
    let mut data = Vec::with_capacity(768 * 560 * 4 + 54);
    let width = 768u32;
    let height = 560u32;
    let file_size = 54 + width * height * 4;
    data.extend_from_slice(b"BM");
    data.extend_from_slice(&file_size.to_le_bytes());
    data.extend_from_slice(&[0, 0, 0, 0]);
    data.extend_from_slice(&54u32.to_le_bytes());
    data.extend_from_slice(&40u32.to_le_bytes());
    data.extend_from_slice(&(width as i32).to_le_bytes());
    data.extend_from_slice(&(-(height as i32)).to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&32u16.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&(width * height * 4).to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());

    for &pix in buffer.iter() {
        data.extend_from_slice(&pix.to_le_bytes());
    }
    fs::write(path, data).unwrap();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let game_path_str = if args.len() > 1 {
        args[1].clone()
    } else {
        "D:/Emulatori/Mame/Software/cdimono1/Space Ace (USA).chd".to_string()
    };
    let max_frames: u32 = if args.len() > 2 {
        args[2].parse().unwrap_or(3500)
    } else {
        3500
    };

    println!(">>> Inspecting game boot: {}", game_path_str);
    let has_disc = game_path_str != "none" && !game_path_str.is_empty();
    let game_path = PathBuf::from(&game_path_str);
    if has_disc && !game_path.exists() {
        eprintln!("Error: game file not found: {}", game_path_str);
        std::process::exit(1);
    }

    let is_pal = game_path_str.to_lowercase().contains("europe") || game_path_str.to_lowercase().contains("(e)") || !has_disc;

    let mut bus = CdiBus::new();
    let bios_path = find_file("bios/cdi220b.rom").expect("cdi220b.rom non trovata");
    bus.load_rom(&bios_path)?;
    if let Some(nvram_path) = find_file("bios/nvram_mk48t08.bin") {
        let _ = bus.nvram.load_from_file(&nvram_path);
    }
    let dvc_path = find_file("dvc/vmpega.rom").expect("vmpega.rom non trovata");
    bus.load_dvc_rom(&dvc_path)?;
    if has_disc {
        bus.load_cdrom(&game_path)?;
    }
    bus.set_video_mode(if is_pal {
        opencdi::mcd212::VideoMode::Pal
    } else {
        opencdi::mcd212::VideoMode::Ntsc
    });

    let mut cpu = CpuCore::new();
    cpu.set_cpu_type(CpuType::SCC68070);
    cpu.reset(&mut bus);

    let mut disc_launched = false;
    let mut shell_ready_count = 0;
    let mut prev_lba = 0u32;
    let mut path_names: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    let mut pending_open: Option<(String, u32)> = None;

    let _button_pressed = false;
    let mut pa_44080_logged = false;
    let mut srq_traced = false;
    let mut srq_countdown = 0;
    let mut bd13_a6: u32 = 0;
    let mut prev_bd13_state: (u16, u8, u32, u16, u8, u8, u8, u8) = (0xFFFF, 0xFF, 0, 0xFFFF, 0xFF, 0xFF, 0xFF, 0xFF);

    for frame in 1..=max_frames {
        // Se un disco è presente, avvia automaticamente la riproduzione una sola volta quando la shell del BIOS è pronta
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

            if shell_ready_count == 0 {
                let a6_base = bus.read_long(0x0);
                let sys_table = bus.read_long(a6_base + 0x3A8);
                let f_send = bus.read_long(sys_table + 8 * 4);
                println!(">>> F$Send entry point: 0x{:08X}", f_send);
            }
            if shell_ready_count == 0 {
                println!(">>> Trampoline at 0x62C: {:04X} {:04X} {:04X} {:04X} {:04X}",
                    bus.read_word(0x62C), bus.read_word(0x62E), bus.read_word(0x630), bus.read_word(0x632), bus.read_word(0x634));
            }
            if shell_ready_count == 0 {
                let a6_base = bus.read_long(0x0);
                let sys_table = bus.read_long(a6_base + 0x3A8);
                let f_send = bus.read_long(sys_table + 8 * 4);
                println!(">>> F$Send entry point: 0x{:08X}", f_send);
            }
            if shell_ready_count == 0 {
                println!(">>> Trap 0 Vector (0x80) in RAM: 0x{:08X}", bus.read_long(0x80));
            }
            if shell_state >= 2 {
                shell_ready_count += 1;
                if shell_ready_count == 1 || shell_ready_count == 150 {
                    bus.slave.update_mouse(575, 335, 0);
                } else if shell_ready_count == 30 || shell_ready_count == 180 {
                    println!(">>> [Frame {}] Shell BIOS pronta (stato {}): click PLAY CD-I...", frame, shell_state);
                    bus.slave.update_mouse(575, 335, 1);
                } else if shell_ready_count == 50 || shell_ready_count == 200 {
                    bus.slave.update_mouse(575, 335, 0);
                }
            }
            if ((cpu.pc >= 0x00010000 && cpu.pc < 0x00300000) || (cpu.pc >= 0x00D00000 && cpu.pc < 0x00E00000)) && frame > 250 {
                println!(">>> [Frame {}] Disc app running at PC=0x{:06X}", frame, cpu.pc);
                disc_launched = true;
                bus.emulate_wait_states = true;
                let (mx, my) = (bus.slave.device_mouse_x, bus.slave.device_mouse_y);
                bus.slave.update_mouse(mx, my, 0);
            }
        }

        if bus.mcd212.plane_a[0x44080] != 0 && !pa_44080_logged {
            pa_44080_logged = true;
            println!(">>> [Frame {} Line 0] PA at 0x44080 is NON-ZERO! PC=0x{:06X}", frame, cpu.pc);
        }
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

            let int1_lvl = if (bus.scc_timer.lir & 0x80) != 0 {
                let prio = (bus.scc_timer.lir >> 4) & 7;
                if prio != 0 { prio } else { 5 }
            } else { 0 };

            let timer_lvl = if bus.scc_timer.timer_irq {
                bus.scc_timer.picr1 & 7
            } else { 0 };

            let cdic_lvl = if bus.in4_pending() { 4 } else { 0 };
            let slave_lvl = if bus.slave.irq_pending { 2 } else { 0 };

            let highest_ipl = int1_lvl.max(timer_lvl).max(cdic_lvl).max(slave_lvl);
            cpu.set_irq(highest_ipl);

            let mut line_cycles = cycle_debt;
            while line_cycles < cyc_per_line as i32 {
                let remaining = (cyc_per_line as i32 - line_cycles).max(1);
                let has_win = bd13_a6 != 0 && bus.read_long(bd13_a6.wrapping_sub(0x7EAC)) != 0;
                let budget = if has_win || frame >= 5500 { 1 } else { remaining.min(32) };
                bus.current_pc = cpu.pc;
                bus.current_line_cycle = line_cycles.max(0) as u32;
                if cpu.pc >= 0x00DE1080 && cpu.pc < 0x00DF7000 && bd13_a6 == 0 {
                    bd13_a6 = cpu.a(6);
                    println!(">>> [Frame {}] Captured cdi_bd13 A6 = 0x{:08X}", frame, bd13_a6);
                }



                if cpu.pc == 0xDF0E04 {
                    println!(">>> [Frame {} Line {}] HIT PUSH 0x100A at 0xDF0E04 (FD84)!", frame, line);
                }
                if cpu.pc == 0xDF0EBE {
                    println!(">>> [Frame {} Line {}] HIT PUSH 0x100A at 0xDF0EBE (FE3E)!", frame, line);
                }
                if cpu.pc == 0xDF1C56 {
                    println!(">>> [Frame {} Line {}] HIT PUSH 0x100A at 0xDF1C56 (10BD6)! -7032=0x{:X}", frame, line, bus.read_byte(cpu.a(6).wrapping_sub(0x7032)));
                }
                if cpu.pc == 0xDE6F84 {
                    println!(">>> [Frame {} Line {}] HIT 0x005F04: -$7032(a6) CLEARED TO 0! caller_pc=0x{:08X}",
                        frame, line, bus.read_long(cpu.a(7)));
                }
                if cpu.pc == 0xDE6958 {
                    println!(">>> [Frame {} Line {}] CALLED 0x0058D8 (intro setup)!", frame, line);
                }
                if cpu.pc == 0xDE69D6 {
                    println!(">>> [Frame {} Line {}] HIT 0x005956: -$7032(a6) SET TO 1!", frame, line);
                }
                if cpu.pc == 0xDE6EBC {
                    let sp = cpu.a(7);
                    let arg = bus.read_long(sp);
                    println!(">>> [Frame {} Line {}] CALLED 0x005E3C (misc.rtf callback): arg=0x{:08X} (arg>>16=0x{:04X})",
                        frame, line, arg, arg >> 16);
                }
                if cpu.pc == 0xDF0B24 {
                    let sp = cpu.a(7);
                    let w1e = bus.read_long(sp + 0x1E);
                    let w22 = bus.read_long(sp + 0x22);
                    let w24 = bus.read_long(sp + 0x24);
                    println!(">>> [Frame {} Line {}] HIT 0x00FAA4: $1E=0x{:X}, $22=0x{:X}, $24=0x{:X}, -$6f00=0x{:X}",
                        frame, line, w1e, w22, w24, bus.read_long(cpu.a(6).wrapping_sub(0x6F00)));
                }
                if cpu.pc == 0xDE70FC {
                    println!(">>> [Frame {} Line {}] CALLED 0x005F7C (start_game caller)!", frame, line);
                }
                if cpu.pc == 0xDE8CD4 {
                    println!(">>> [Frame {} Line {}] CALLED 0xDE8CD4 (setup_game)!", frame, line);
                }
                if cpu.pc == 0xDED6E8 {
                    let s_ptr = cpu.d(1);
                    let mut s = String::new();
                    if s_ptr > 0x1000 && s_ptr < 0x1000000 {
                        for k in 0..20 {
                            let b = bus.read_byte(s_ptr + k);
                            if b == 0 { break; }
                            s.push(b as char);
                        }
                    }
                    println!(">>> [Frame {} Line {}] LOOKUP SCENE 0xDED6E8: '{}' (ptr=0x{:08X})", frame, line, s, s_ptr);
                }
                if cpu.pc == 0xDF11A2 || cpu.pc == 0xDF123A {
                    let a6 = cpu.a(6);
                    let state = cpu.d(0) & 0xFF;
                    let win_ptr = bus.read_long(a6.wrapping_sub(0x7EAC));
                    let cur_idx = bus.read_word(a6.wrapping_sub(0x711A));
                    let scene_ptr = bus.read_long(a6.wrapping_sub(0x7EBC) + (cur_idx as u32) * 4);
                    let mut scene_name = String::new();
                    let name_ptr = bus.read_long(scene_ptr);
                    if name_ptr > 0x1000 && name_ptr < 0x1000000 {
                        for k in 0..20 {
                            let b = bus.read_byte(name_ptr + k);
                            if b == 0 { break; }
                            scene_name.push(b as char);
                        }
                    }
                    println!(">>> [Frame {}] SCENE ACTIVE (PC=0x{:06X}): idx={}, scene='{}' (0x{:08X}), state_7ea0={}, win_ptr=0x{:08X}",
                        frame, cpu.pc, cur_idx, scene_name, scene_ptr, state, win_ptr);
                    if win_ptr != 0 {
                        let w_start = bus.read_long(win_ptr);
                        let w_end = bus.read_long(win_ptr + 4);
                        println!("    Decision Window [{}..{}]:", w_start, w_end);
                        for ch in 0..6u32 {
                            let off = ch * 6;
                            let btype = bus.read_word(win_ptr + off + 8) & 0xFF;
                            let tptr = bus.read_long(win_ptr + off + 0xA);
                            let mut tstr = String::new();
                            if tptr > 0x1000 && tptr < 0x1000000 {
                                for k in 0..20 {
                                    let b = bus.read_byte(tptr + k);
                                    if b == 0 { break; }
                                    tstr.push(b as char);
                                }
                            }
                            println!("      Choice {}: branch_type={}, target=0x{:08X} ('{}')", ch, btype, tptr, tstr);
                        }
                    }
                }
                if cpu.pc == 0xDF1D18 {
                    let a6 = cpu.a(6);
                    let t = bus.read_long(a6.wrapping_sub(0x7EF0));
                    println!(">>> [Frame {}] 0x5008 START TIME CAPTURED: -7ec8 = {} (0x{:08X})", frame, t, t);
                }
                if cpu.pc == 0xDF18C8 {
                    println!(">>> [Frame {} Line {}] HIT 0x010848 (event 0x200C preloader)!", frame, line);
                }
                if cpu.pc == 0xDF11EE {
                    let a6 = cpu.a(6);
                    let cur = bus.read_word(a6.wrapping_sub(0x711A));
                    println!(">>> [Frame {} Line {}] HIT 0x01016E (event 0x100A advance scene): cur_711A={}", frame, line, cur);
                }
                if cpu.pc == 0xDF21B4 {
                    let sp = cpu.a(7);
                    let evt = bus.read_long(sp + 0x26);
                    if (evt != 0x5002 && evt != 0x8001) || frame % 50 == 0 {
                        println!(">>> [Frame {} Line {}] DISPATCH EVENT 0x{:04X}", frame, line, evt);
                    }
                }
                if cpu.pc == 0xDF1D68 || cpu.pc == 0xDF1D8C {
                    let sp = cpu.a(7);
                    let a6 = cpu.a(6);
                    let elapsed = bus.read_long(sp + 0xA);
                    let win_ptr = bus.read_long(a6.wrapping_sub(0x7EAC));
                    let w_start = bus.read_long(win_ptr);
                    let w_end = bus.read_long(win_ptr + 4);
                    let state = bus.read_byte(a6.wrapping_sub(0x7EA0));
                    let choice = bus.read_word(a6.wrapping_sub(0x7E9E));
                    if frame % 10 == 0 || state == 1 {
                        println!(">>> [Frame {}] DECISION CHECK (PC=0x{:06X}): state={}, elapsed={}, win=[{}..{}], choice={}",
                            frame, cpu.pc, state, elapsed, w_start, w_end, choice);
                    }
                    // When decision window opens (state == 1) and no choice made yet, simulate moving left (dx = -30)!
                    if cpu.pc == 0xDF1D8C && choice == 0 {
                        let (mx, my) = (bus.slave.device_mouse_x, bus.slave.device_mouse_y);
                        println!(">>> [Frame {}] SIMULATING LEFT INPUT IN DECISION WINDOW!", frame);
                        bus.slave.update_mouse(mx.saturating_sub(30), my, 0);
                    }
                }
                if cpu.pc == 0xDF1EFE {
                    let a6 = cpu.a(6);
                    let choice = bus.read_word(a6.wrapping_sub(0x7E9E));
                    let win_ptr = bus.read_long(a6.wrapping_sub(0x7EAC));
                    let off = (choice as u32) * 6;
                    let branch_type = bus.read_word(win_ptr + off + 8) & 0xFF;
                    let target_ptr = bus.read_long(win_ptr + off + 0xA);
                    let mut target_str = String::new();
                    if target_ptr > 0x1000 && target_ptr < 0x1000000 {
                        for k in 0..20 {
                            let b = bus.read_byte(target_ptr + k);
                            if b == 0 { break; }
                            target_str.push(b as char);
                        }
                    }
                    println!(">>> [Frame {}] DECISION WINDOW EXPIRED at 0xDF1EFE! choice={}, branch_type={}, target_ptr=0x{:08X} ('{}')",
                        frame, choice, branch_type, target_ptr, target_str);
                }
                if cpu.pc == 0xDF1DA0 {
                    println!(">>> [Frame {}] IMMEDIATE BRANCH at 0xDF1DA0!", frame);
                }
                if cpu.pc == 0xDF1EE6 {
                    println!(">>> [Frame {}] DEFERRED BRANCH at 0xDF1EE6!", frame);
                }
                if cpu.pc == 0xDEFF96 {
                    let a6 = cpu.a(6);
                    let dir = bus.read_long(a6.wrapping_sub(0x7A5E));
                    let state = bus.read_byte(a6.wrapping_sub(0x7EA0));
                    println!(">>> [Frame {}] PLAYER DIRECTION REGISTERED at 0xDEFF96! dir={}, state_7ea0={}", frame, dir, state);
                }
                if cpu.pc == 0xDE9380 {
                    let sp = cpu.a(7);
                    let arg = bus.read_long(sp + 4);
                    println!(">>> [Frame {}] SCENE PLAY CALLBACK 0xDE9380! arg=0x{:08X} (err=0x{:04X})", frame, arg, arg >> 16);
                }
                if cpu.pc == 0xDF1168 {
                    let a6 = cpu.a(6);
                    println!(">>> [Frame {}] EVENT 0x100B at 0xDF1168: -7020={}, -7018={}",
                        frame, bus.read_byte(a6.wrapping_sub(0x7020)), bus.read_byte(a6.wrapping_sub(0x7018)));
                }
                if cpu.pc == 0xDF101C {
                    let a6 = cpu.a(6);
                    println!(">>> [Frame {}] EVENT 0x100C at 0xDF101C: -701e={}",
                        frame, bus.read_byte(a6.wrapping_sub(0x701E)));
                }


                if cpu.pc == 0xDF6BAA {
                    println!(">>> [Frame {} Line {}] SIGNAL INTERCEPT in cdi_bd13! Sig=0x{:X}, Handler=0x{:08X}",
                        frame, line, cpu.d(1), bus.read_long(cpu.a(6).wrapping_sub(0x7312)));
                }
                if cpu.pc == 0xDEF99C {
                    println!(">>> ENTERED 0xDEF99C (startup before main game loop) at Frame {}!", frame);
                }
                if cpu.pc == 0xDF3796 {
                    println!(">>> load_scene(0xDF3796) CALLED at Frame {}! D0=0x{:X}, D1=0x{:X}, A0=0x{:X}", frame, cpu.d(0), cpu.d(1), cpu.a(0));
                }

                if cpu.pc == 0xDF143A {
                    let a6 = cpu.a(6);
                    let sp = cpu.a(7);
                    let i_cnt = bus.read_long(sp + 6);
                    if i_cnt == 0 || (frame >= 1700 && frame <= 1705 && i_cnt < 10) {
                        let cur = bus.read_word(a6.wrapping_sub(0x711A)) as u32;
                        let desc = bus.read_long(a6.wrapping_sub(0x7EBC) + cur * 4);
                        let f2c = if desc > 0 && desc < 0x2000000 { bus.read_long(desc + 0x2C) } else { 0 };
                        let plst = if f2c > 0 && f2c < 0x2000000 { bus.read_long(f2c + 0x8) } else { 0 };
                        println!(">>> [Fr {} L {}] LOOP 0xDF143A: i={}, cur={}, desc=0x{:08X}, f2c=0x{:08X}, plst=0x{:08X}",
                            frame, line, i_cnt, cur, desc, f2c, plst);
                    }
                }
                if cpu.pc == 0xDF149E {
                    println!(">>> REACHED DF149E (exited loop) at Frame {} Line {}!", frame, line);
                }
                let batch = cpu.run_for_cycles(&mut bus, budget);
                let mut batch_cycles = batch.cycles + bus.consume_wait_cycles();

                if srq_traced && srq_countdown > 0 {
                    println!("   TRAP RET: PC=0x{:06X}, D0=0x{:X}, D1=0x{:X}, SR=0x{:04X}", cpu.pc, cpu.d(0), cpu.d(1), cpu.get_sr());
                    srq_countdown -= 1;
                    if srq_countdown == 0 {
                        srq_traced = false;
                    }
                }

                if let Some((ref name, ret_pc)) = pending_open {
                    if cpu.pc == ret_pc {
                        let path = cpu.d(0) as u32;
                        let err = (cpu.get_sr() & 1) != 0;
                        println!("   -> [Frame {}] Opened '{}' as path {} (error={})", frame, name, path, err);
                        path_names.insert(path, name.clone());
                        pending_open = None;
                    }
                }

                match batch.exit {
                    m68k::CycleBatchExit::TrapInstruction { trap_num } => {
                        if trap_num == 0 {
                            let next_word = bus.read_word(cpu.pc);
                            let caller_pc = cpu.pc;
                            let trace_now = disc_launched;
                            match next_word {
                                0x0084 | 0x0083 => {
                                    let mut name = String::new();
                                    for i in 0..64 {
                                        let b = bus.read_byte(cpu.a(0).wrapping_add(i));
                                        if b == 0 || b == 0x0D || b == b' ' { break; }
                                        name.push(b as char);
                                    }
                                    println!("[Frame {}] {} '{}' mode=0x{:X} at PC=0x{:06X}",
                                        frame, if next_word == 0x0084 { "I$Create" } else { "I$Open" },
                                        name, cpu.d(0), caller_pc);
                                    pending_open = Some((name, caller_pc + 2));
                                }
                                0x008F => {
                                    let path = cpu.d(0) as u32;
                                    let code = cpu.d(1) as u16;
                                    let pname = path_names.get(&path).cloned().unwrap_or_else(|| format!("Path {}", path));
                                    if trace_now {
                                        println!("[Frame {}] I$GetStt on '{}' (path {}): code=0x{:04X} (A0=0x{:08X}) at PC=0x{:06X}",
                                            frame, pname, path, code, cpu.a(0), caller_pc);
                                    }
                                }
                                0x008E => {
                                    let path = cpu.d(0) as u32;
                                    let code = cpu.d(1) as u16;
                                    let pname = path_names.get(&path).cloned().unwrap_or_else(|| format!("Path {}", path));
                                    if trace_now {
                                        println!("[Frame {}] I$SetStt on '{}' (path {}): code=0x{:04X}, D2=0x{:X}, D3=0x{:X}, A0=0x{:08X} at PC=0x{:06X}",
                                            frame, pname, path, code, cpu.d(2), cpu.d(3), cpu.a(0), caller_pc);
                                    }
                                }
                                0x0001 | 0x0002 | 0x0003 | 0x0005 => {
                                    let mut mod_name = String::new();
                                    for i in 0..64 {
                                        let b = bus.read_byte(cpu.a(0).wrapping_add(i));
                                        if b == 0 || b == 0x0D || b == b' ' { break; }
                                        mod_name.push(b as char);
                                    }
                                    let call_name = match next_word {
                                        0x0001 => "F$Link",
                                        0x0002 => "F$Load",
                                        0x0003 => "F$Fork",
                                        0x0005 => "F$Chain",
                                        _ => "F$Module",
                                    };
                                    println!("[Frame {}] {} '{}' at PC=0x{:06X}", frame, call_name, mod_name, caller_pc);
                                }
                                0x0006 => {
                                    if trace_now {
                                        println!("[Frame {}] F$Exit code=0x{:X} at PC=0x{:06X}", frame, cpu.d(1), caller_pc);
                                    }
                                }
                                0x0008 => {
                                    if trace_now && (cpu.d(1) != 0x5002 || frame % 50 == 0) {
                                        println!("[Frame {}] F$Send to proc=0x{:X}, sig=0x{:X} at PC=0x{:06X}",
                                            frame, cpu.d(0), cpu.d(1), caller_pc);
                                    }
                                }
                                0x0088 => {
                                    let path = cpu.d(0) as u32;
                                    let pname = path_names.get(&path).cloned().unwrap_or_else(|| format!("Path {}", path));
                                    if trace_now {
                                        let sp = cpu.a(7);
                                        let ret1 = bus.read_long(sp);
                                        let ret2 = bus.read_long(sp + 4);
                                        let ret3 = bus.read_long(sp + 8);
                                        let ret4 = bus.read_long(sp + 12);
                                        println!("[Frame {}] I$Read on '{}' (path {}): D1={}, D2={}, buf=0x{:08X} at PC=0x{:06X}, stack=[0x{:08X}, 0x{:08X}, 0x{:08X}, 0x{:08X}]",
                                            frame, pname, path, cpu.d(1), cpu.d(2), cpu.a(0), caller_pc, ret1, ret2, ret3, ret4);
                                    }
                                    if frame >= 1670 && frame <= 1680 {
                                        srq_traced = true;
                                        srq_countdown = 20;
                                    }
                                }
                                0x0089 => {
                                    let path = cpu.d(0) as u32;
                                    let pname = path_names.get(&path).cloned().unwrap_or_else(|| format!("Path {}", path));
                                    if trace_now {
                                        println!("[Frame {}] I$Write on '{}' (path {}): count={}, buf=0x{:08X} at PC=0x{:06X}",
                                            frame, pname, path, cpu.d(2), cpu.a(0), caller_pc);
                                    }
                                }
                                _ => {
                                    if frame >= 1670 && frame <= 1720 {
                                        println!("[Frame {}] OS-9 Trap code=0x{:04X} at PC=0x{:06X}, D0=0x{:X}, D1=0x{:X}, D2=0x{:X}, D3=0x{:X}, A0=0x{:X}, A1=0x{:X}",
                                            frame, next_word, caller_pc, cpu.d(0), cpu.d(1), cpu.d(2), cpu.d(3), cpu.a(0), cpu.a(1));
                                    }
                                }
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
                        println!("[Frame {} Line {}] ILLEGAL INSTRUCTION at PC=0x{:06X} (op=0x{:04X})",
                            frame, line, cpu.pc, bus.read_word(cpu.pc));
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
                        line_cycles = cyc_per_line as i32;
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
            cycle_debt = (line_cycles - cyc_per_line as i32).clamp(0, 100);

            bus.render_scanline(line);

            bus.tick_slave(cyc_per_line);
            bus.tick_cdic(cyc_per_line);
            bus.tick_dvc(cyc_per_line);
        }

        if disc_launched {
            if frame % 100 == 0 {
                let dvc_nz = bus.dvc.framebuffer.iter().filter(|&&p| (p & 0x00FFFFFF) != 0).count();
                let pa_nz = bus.mcd212.plane_a.iter().filter(|&&b| b != 0).count();
                let pb_nz = bus.mcd212.plane_b.iter().filter(|&&b| b != 0).count();
                println!(">>> [Frame {} DIAG] PC=0x{:06X}, ICMR=0x{:08X}, TCR=0x{:08X}, VSR=[0x{:X}, 0x{:X}], DCR=[0x{:04X}, 0x{:04X}], DVC dec={}, pres={}, fb_nz={}/{} (w={}, h={}, valid={}), PA_nz={}, PB_nz={}",
                    frame, cpu.pc, bus.mcd212.image_coding_method, bus.mcd212.transparency_control,
                    bus.mcd212.get_vsr::<0>(), bus.mcd212.get_vsr::<1>(),
                    bus.mcd212.dcr[0], bus.mcd212.dcr[1],
                    bus.dvc.frames_decoded_count, bus.dvc.frames_presented_count,
                    dvc_nz, bus.dvc.framebuffer.len(), bus.dvc.video_width, bus.dvc.video_height, bus.dvc.has_valid_frame,
                    pa_nz, pb_nz);
                if frame == 1700 {
                    println!("=== SCANNING MODULES IN DVC RAM1 ===");
                    for i in (0..bus.dvc.ram1.len().saturating_sub(64)).step_by(2) {
                        let w = u16::from_be_bytes([bus.dvc.ram1[i], bus.dvc.ram1[i+1]]);
                        if w == 0x4AFC {
                            let size = u32::from_be_bytes([bus.dvc.ram1[i+4], bus.dvc.ram1[i+5], bus.dvc.ram1[i+6], bus.dvc.ram1[i+7]]);
                            let name_off = u32::from_be_bytes([bus.dvc.ram1[i+12], bus.dvc.ram1[i+13], bus.dvc.ram1[i+14], bus.dvc.ram1[i+15]]) as usize;
                            if name_off < bus.dvc.ram1.len() - i {
                                let mut mname = String::new();
                                for k in 0..32 {
                                    let b = bus.dvc.ram1[i + name_off + k];
                                    if b == 0 || b == 0x0D { break; }
                                    mname.push(b as char);
                                }
                                println!("  Module '{}' at 0x{:06X}, size=0x{:X}", mname, 0xD00000 + i, size);
                            }
                        }
                    }
                }
                if frame == 1700 || frame == 1800 {
                    let ptr = cpu.a(6).wrapping_sub(0x698A);
                    let w0 = bus.read_word(ptr);
                    let target = bus.read_long(ptr + 2);
                    println!("   -$698a(a6): op=0x{:04X}, target=0x{:08X}", w0, target);
                    println!("   DCP=[0x{:X}, 0x{:X}], DCA=[0x{:X}, 0x{:X}], VSR=[0x{:X}, 0x{:X}]",
                        bus.mcd212.get_dcp::<0>(), bus.mcd212.get_dcp::<1>(),
                        bus.mcd212.dca[0], bus.mcd212.dca[1],
                        bus.mcd212.get_vsr::<0>(), bus.mcd212.get_vsr::<1>());
                    println!("   PA[0..16]: {:02X?}", &bus.mcd212.plane_a[0..16]);
                    println!("   PA[0x400..0x420]: {:02X?}", &bus.mcd212.plane_a[0x400..0x420]);
                    println!("   PB[0..16]: {:02X?}", &bus.mcd212.plane_b[0..16]);
                    println!("   PB[0x400..0x420]: {:02X?}", &bus.mcd212.plane_b[0x400..0x420]);
                    let dca0 = (bus.mcd212.dca[0] & 0x7FFFC) as usize;
                    let dca1 = (bus.mcd212.dca[1] & 0x7FFFC) as usize;
                    println!("   PA at DCA0 (0x{:X}): {:02X?}", dca0, &bus.mcd212.plane_a[dca0..dca0+32]);
println!("=== Disassembling cdi_bd13 at 0xDF4180 ===");
for pc in (0xDF4170..0xDF4220).step_by(2) {
    let w = bus.read_word(pc);
    print!(" {:04X}", w);
    if (pc - 0xDF4170) % 16 == 14 { println!(); }
}
println!();

for off in (0x73A60..0x73DE0).step_by(64) {
    println!("     DCA 0x{:05X}: {:02X?}", off, &bus.mcd212.plane_a[off..off+32]);
}
                    // Find non-zero ranges in plane_a and plane_b
                    let mut pa_ranges = Vec::new();
                    let mut in_range = false;
                    let mut r_start = 0;
                    for (i, &b) in bus.mcd212.plane_a.iter().enumerate() {
                        if b != 0 && !in_range { in_range = true; r_start = i; }
                        else if b == 0 && in_range { in_range = false; pa_ranges.push((r_start, i)); }
                    }
                    if in_range { pa_ranges.push((r_start, bus.mcd212.plane_a.len())); }
                    println!("   PA at 0x7FF54: {:02X?}", &bus.mcd212.plane_a[0x7FF54..0x7FF54+32]);
println!("=== Disassembly at 0xDF4160..0xDF4220 ===");
for pc in (0xDF4160..0xDF4220).step_by(2) {
    print!("{:04X} ", bus.read_word(pc));
}
println!();

                    println!("   PB at 0x7AB14: {:02X?}", &bus.mcd212.plane_b[0x7AB14..0x7AB14+32]);
                    for (name, plane) in [("PA", &bus.mcd212.plane_a), ("PB", &bus.mcd212.plane_b)] {
                        for i in 0..plane.len() - 4 {
                            if (plane[i+1] == 0x04 || plane[i+1] == 0x24) && plane[i+2] == 0x40 && (plane[i+3] == 0xC0 || plane[i+3] == 0x80) {
                                println!("  Found pointer in {} at 0x{:05X}: {:02X?}", name, i, &plane[i..i+4]);
                            }
                        }
                    }
                    println!(">>> Signal handler at Frame 1700: 0x{:08X}", bus.read_long(cpu.a(6).wrapping_sub(0x7312)));
println!("=== Non-zero ranges in Plane A ===");
for (i, chunk) in bus.mcd212.plane_a.chunks(4096).enumerate() {
    let nz = chunk.iter().filter(|&&b| b != 0).count();
    if nz > 0 {
        println!("  PA 0x{:05X}..0x{:05X}: {} non-zero bytes", i*4096, (i+1)*4096, nz);
    }
}
println!("=== Non-zero ranges in Plane B ===");
for (i, chunk) in bus.mcd212.plane_b.chunks(4096).enumerate() {
    let nz = chunk.iter().filter(|&&b| b != 0).count();
    if nz > 0 {
        println!("  PB 0x{:05X}..0x{:05X}: {} non-zero bytes", i*4096, (i+1)*4096, nz);
    }
}
println!("   PA at 0x43FC0..0x44100:");
                    for off in (0x43FC0..0x44100).step_by(64) {
                        println!("     0x{:05X}: {:02X?}", off, &bus.mcd212.plane_a[off..off+32]);
                    }
                    // Scan plane_a and plane_b for RELOAD DCP (0x30) or RELOAD VSR (0x40/0x50)
                    for (name, plane) in [("PA", &bus.mcd212.plane_a), ("PB", &bus.mcd212.plane_b)] {
                        println!("--- Scanning {} for 0x30/0x40/0x50 ---", name);
                        let mut i = 0;
                        let mut count = 0;
                        while i < plane.len() - 4 && count < 30 {
                            let op = plane[i];
                            let target = u32::from_be_bytes([plane[i], plane[i+1], plane[i+2], plane[i+3]]) & 0x003FFFFF;
                            if op == 0x30 && target > 0x1000 && target < 0x80000 {
                                println!("  {} at 0x{:05X}: RELOAD DCP -> 0x{:05X}", name, i, target);
                                count += 1;
                                i += 60;
                            } else if (op == 0x40 || op == 0x50) && target > 0x1000 && target < 0x80000 {
                                println!("  {} at 0x{:05X}: RELOAD VSR ({:02X}) -> 0x{:05X}", name, i, op, target);
                                count += 1;
                                i += 60;
                            }
                            i += 4;
                        }
                    }

                    let mut pb_ranges = Vec::new();
                    let mut in_range_b = false;
                    let mut rb_start = 0;
                    for (i, &b) in bus.mcd212.plane_b.iter().enumerate() {
                        if b != 0 && !in_range_b { in_range_b = true; rb_start = i; }
                        else if b == 0 && in_range_b { in_range_b = false; pb_ranges.push((rb_start, i)); }
                    }
                    if in_range_b { pb_ranges.push((rb_start, bus.mcd212.plane_b.len())); }
                    println!("=== DVC RAM1 at 0xDEF78E ===");
                    let off = 0xDEF78E - 0xD00000;
                    println!("  Bytes: {:02X?}", &bus.dvc.ram1[off..off+64]);
                    for k in (off..off+0x1000).step_by(4) {
                        let val = u32::from_be_bytes([bus.dvc.ram1[k], bus.dvc.ram1[k+1], bus.dvc.ram1[k+2], bus.dvc.ram1[k+3]]);
                        if val > 0x1000 && val < 0x200000 {
                            println!("  Pointer at 0x{:06X}: 0x{:08X}", 0xD00000 + k, val);
                        }
                    }
for (i, w) in bus.dvc.ram1.windows(4).enumerate() {
    let val = u32::from_be_bytes([w[0], w[1], w[2], w[3]]);
    if val == 0x000440C0 || val == 0x002440C0 || val == 0x00044080 || val == 0x00244080 || val == 0x00048930 {
        println!("  FOUND TARGET in DVC RAM1 at 0x{:06X}: 0x{:08X}", 0xD00000 + i, val);
    }
}
for (i, w) in bus.mcd212.plane_a.windows(4).enumerate() {
    let val = u32::from_be_bytes([w[0], w[1], w[2], w[3]]);
    if val == 0x000440C0 || val == 0x002440C0 || val == 0x00044080 || val == 0x00244080 || val == 0x00048930 {
        println!("  FOUND TARGET in Plane A at 0x{:06X}: 0x{:08X}", i, val);
        println!("   Context at 0x{:06X}: {:02X?}", i.saturating_sub(32), &bus.mcd212.plane_a[i.saturating_sub(32)..i+32]);
    }
}
for (i, w) in bus.mcd212.plane_b.windows(4).enumerate() {
    let val = u32::from_be_bytes([w[0], w[1], w[2], w[3]]);
    if val == 0x000440C0 || val == 0x002440C0 || val == 0x00044080 || val == 0x00244080 || val == 0x00048930 {
        println!("  FOUND TARGET in Plane B at 0x{:06X}: 0x{:08X}", 0x200000 + i, val);
    }
}

                    println!("   PB at DCA1 (0x{:X}): {:02X?}", dca1, &bus.mcd212.plane_b[dca1..dca1+32]);
                    println!("=== ICA script at 0x727F0 ===");
                    for off in (0x727F0..0x73000).step_by(4) {
                        let cmd = u32::from_be_bytes([bus.mcd212.plane_a[off], bus.mcd212.plane_a[off+1], bus.mcd212.plane_a[off+2], bus.mcd212.plane_a[off+3]]);
                        println!("  0x{:05X}: 0x{:08X}", off, cmd);
                        if (cmd >> 24) < 0x10 { break; }
                    }
                    println!("=== ICA script at 0x7FFB0 ===");
                    for off in (0x7FFB0..0x80000).step_by(4) {
                        let cmd = u32::from_be_bytes([bus.mcd212.plane_a[off], bus.mcd212.plane_a[off+1], bus.mcd212.plane_a[off+2], bus.mcd212.plane_a[off+3]]);
                        println!("  0x{:05X}: 0x{:08X}", off, cmd);
                        if (cmd >> 24) < 0x10 { break; }
                    }
                    println!("=== DCP0 script at 0x73520 ===");
                    for off in (0x73520..0x73600).step_by(4) {
                        let cmd = u32::from_be_bytes([bus.mcd212.plane_a[off], bus.mcd212.plane_a[off+1], bus.mcd212.plane_a[off+2], bus.mcd212.plane_a[off+3]]);
                        println!("  0x{:05X}: 0x{:08X}", off, cmd);
                        if (cmd >> 24) < 0x10 { break; }
                    }
                    println!("   PB at 0x7B840: {:02X?}", &bus.mcd212.plane_b[0x7B840..0x7B840+32]);
                    let dcp0 = (bus.mcd212.get_dcp::<0>() & 0x7FFFC) as usize;
                    let dcp1 = (bus.mcd212.get_dcp::<1>() & 0x7FFFC) as usize;
                    println!("   PA at DCP0 (0x{:X}): {:02X?}", dcp0, &bus.mcd212.plane_a[dcp0..dcp0+64]);
                    println!("   PB at DCP1 (0x{:X}): {:02X?}", dcp1, &bus.mcd212.plane_b[dcp1..dcp1+64]);
                }
            }
            // No manual click at 2750: let SQ001ST001SG001 play into the first interactive scene
        }

        if bd13_a6 != 0 && frame >= 1945 {
            let cur_idx = bus.read_word(bd13_a6.wrapping_sub(0x711A));
            let state_7ea0 = bus.read_byte(bd13_a6.wrapping_sub(0x7EA0));
            let win_ptr = bus.read_long(bd13_a6.wrapping_sub(0x7EAC));
            let choice = bus.read_word(bd13_a6.wrapping_sub(0x7E9E));
            let f_7020 = bus.read_byte(bd13_a6.wrapping_sub(0x7020));
            let f_702e = bus.read_byte(bd13_a6.wrapping_sub(0x702E));
            let f_7016 = bus.read_byte(bd13_a6.wrapping_sub(0x7016));
            let f_7018 = bus.read_byte(bd13_a6.wrapping_sub(0x7018));
            let t_cur = bus.read_long(bd13_a6.wrapping_sub(0x7EF0));
            let t_start = bus.read_long(bd13_a6.wrapping_sub(0x7EC8));
            let cur_tuple = (cur_idx, state_7ea0, win_ptr, choice, f_7020, f_702e, f_7016, f_7018);
            if cur_tuple != prev_bd13_state || (win_ptr != 0 && frame % 10 == 0) {
                prev_bd13_state = cur_tuple;
                let scene_ptr = bus.read_long(bd13_a6.wrapping_sub(0x7EBC) + ((cur_idx & 3) as u32) * 4);
                let mut sname = String::new();
                if scene_ptr > 0x1000 && scene_ptr < 0x1000000 {
                    let nptr = bus.read_long(scene_ptr);
                    if nptr > 0x1000 && nptr < 0x1000000 {
                        for k in 0..20 {
                            let b = bus.read_byte(nptr + k);
                            if b == 0 { break; }
                            sname.push(b as char);
                        }
                    }
                }
                let (w_s, w_e) = if win_ptr > 0x1000 && win_ptr < 0x1000000 {
                    (bus.read_long(win_ptr), bus.read_long(win_ptr + 4))
                } else { (0, 0) };
                println!(
                    ">>> [Frame {} BD13 STATE] idx={} ('{}'), state_7ea0={}, win=0x{:08X} [{}..{}], choice={}, t_cur={}, t_start={}, elapsed={}, flags(7020={},702e={},7016={},7018={})",
                    frame, cur_idx, sname, state_7ea0, win_ptr, w_s, w_e, choice, t_cur, t_start, t_cur.wrapping_sub(t_start), f_7020, f_702e, f_7016, f_7018
                );
            }
        }

        if bus.cdic.curr_lba != prev_lba {
            println!("[Frame {}] LBA changed to {}", frame, bus.cdic.curr_lba);
            prev_lba = bus.cdic.curr_lba;
        }

        if (frame >= 1600 && frame % 100 == 0) || frame % 500 == 0 || frame == 2750 || frame == 2780 {
            let fname = format!("test_frame_{:04}.bmp", frame);
            save_bmp(&fname, &bus.mcd212.framebuffer);
            println!("[Frame {}] Saved screenshot {}, PC=0x{:06X}, SP=0x{:08X}", frame, fname, cpu.pc, cpu.a(7));
        }
    }

    Ok(())
}

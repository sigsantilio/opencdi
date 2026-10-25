use opencdi::bus::CdiBus;
use m68k::{CpuCore, CpuType, AddressBus};
use std::path::{Path, PathBuf};
use std::fs;

fn find_file(relative: &str) -> Option<PathBuf> {
    for prefix in &["file utili", "../file utili", "../../file utili"] {
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

    for &pix in buffer[..(width * height) as usize].iter() {
        data.extend_from_slice(&pix.to_le_bytes());
    }
    fs::write(path, data).unwrap();
}

fn save_bmp_dvc(path: &str, buffer: &[u32], width: u32, height: u32) {
    let mut data = Vec::with_capacity((width * height * 4 + 54) as usize);
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

    for &pix in buffer[..(width * height) as usize].iter() {
        data.extend_from_slice(&pix.to_le_bytes());
    }
    fs::write(path, data).unwrap();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bus = CdiBus::new();
    let bios_path = find_file("bios/cdi220b.rom").expect("cdi220b.rom non trovata");
    bus.load_rom(&bios_path)?;
    if let Some(nvram_path) = find_file("bios/nvram_mk48t08.bin") {
        let _ = bus.nvram.load_from_file(&nvram_path);
    }
    let dvc_path = find_file("dvc/vmpega.rom").expect("vmpega.rom non trovata");
    bus.load_dvc_rom(&dvc_path)?;
    let game_path = find_file("giochi/Dragon's Lair (USA).chd").expect("game not found");
    bus.load_cdrom(&game_path)?;
    bus.set_video_mode(opencdi::mcd212::VideoMode::Ntsc);

    let mut cpu = CpuCore::new();
    cpu.set_cpu_type(CpuType::SCC68070);
    cpu.reset(&mut bus);

    let mut disc_launched = false;
    let mut shell_ready_count = 0;

    let mut prev_lba = 0u32;
    let mut path_names: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    let mut pending_open: Option<(String, u32)> = None;

    let mut trace_kernel_steps = 0;

    let mut prev_807a = 0u16;

    for frame in 1..=3500 {
        let total_lines = bus.mcd212.video_mode.total_scanlines();
        let cyc_per_line = bus.mcd212.video_mode.cycles_per_scanline();
        let mut cycle_debt = 0i32;

        for line in 0..total_lines {
            bus.tick_scanline(line);
            bus.render_scanline(line);

            if bus.mcd212.vblank_irq {
                bus.scc_timer.lir |= 0x80;
            }

            let int1_lvl = if (bus.scc_timer.lir & 0x80) != 0 {
                let prio = (bus.scc_timer.lir >> 4) & 7;
                if prio != 0 { prio } else { 5 }
            } else { 0 };

            let timer_lvl = if bus.scc_timer.timer_irq {
                bus.scc_timer.picr1 & 7
            } else { 0 };

            let cdic_lvl = if bus.cdic.irq_pending || bus.dvc.irq_pending() {
                4
            } else { 0 };
            let slave_lvl = if bus.slave.irq_pending { 2 } else { 0 };

            let highest_ipl = int1_lvl.max(timer_lvl).max(cdic_lvl).max(slave_lvl);
            cpu.set_irq(highest_ipl);

            let mut line_cycles = -cycle_debt;
            while line_cycles < cyc_per_line as i32 {
                let remaining = (cyc_per_line as i32 - line_cycles).max(1);
                let is_lair_code = (cpu.pc >= 0x270000 && cpu.pc < 0x280000) || (cpu.pc >= 0x00D04000 && cpu.pc < 0x00D06000);
                let budget = if (frame >= 2139 && frame <= 2143) || (frame >= 2704 && frame <= 2707) {
                    1
                } else if frame >= 2698 && frame <= 2725 {
                    1
                } else {
                    remaining.min(32)
                };
                bus.current_pc = cpu.pc;
                bus.current_line_cycle = line_cycles.max(0) as u32;

                if (frame >= 2139 && frame <= 2142) || (frame >= 2700 && frame <= 2707) {
                    if cpu.pc >= 0x270000 && cpu.pc < 0x280000 {
                        let op = bus.read_word(cpu.pc);
                        if (op & 0xFF00) == 0x6100 { // BSR.B / BSR.W
                            let target = if (op & 0xFF) == 0 {
                                let disp16 = bus.read_word(cpu.pc + 2) as i16 as i32;
                                (cpu.pc + 2).wrapping_add(disp16 as u32)
                            } else {
                                let disp8 = (op & 0xFF) as i8 as i32;
                                (cpu.pc + 2).wrapping_add(disp8 as u32)
                            };
                            println!("   [FLOW F{}] BSR 0x{:06X} -> 0x{:06X} (SP=0x{:08X}, D0=0x{:X}, A0=0x{:08X})",
                                frame, cpu.pc, target, cpu.a(7), cpu.d(0), cpu.a(0));
                        } else if (op & 0xFFC0) == 0x4E80 || (op & 0xFFC0) == 0x4EC0 { // JSR
                            println!("   [FLOW F{}] JSR 0x{:06X} (op=0x{:04X}, SP=0x{:08X}, D0=0x{:X}, A0=0x{:08X})",
                                frame, cpu.pc, op, cpu.a(7), cpu.d(0), cpu.a(0));
                        } else if op == 0x4E75 { // RTS
                            let ret = bus.read_long(cpu.a(7));
                            println!("   [FLOW F{}] RTS 0x{:06X} -> 0x{:06X} (D0=0x{:X})",
                                frame, cpu.pc, ret, cpu.d(0));
                        }
                    }
                }

                let cur_807a = bus.read_word(0x00D0007A);
                if cur_807a != prev_807a {
                    println!(">>> [Frame {} Line {} PC=0x{:06X}] VAR 0x00D0007A changed: 0x{:04X} -> 0x{:04X}", frame, line, cpu.pc, prev_807a, cur_807a);
                    prev_807a = cur_807a;
                }
                if cpu.pc == 0x00272BE4 || cpu.pc == 0x00272D70 || cpu.pc == 0x00276464 || cpu.pc == 0x00276500 || cpu.pc == 0x00276672 || cpu.pc == 0x00274470 || cpu.pc == 0x00274582 || cpu.pc == 0x00275540 || cpu.pc == 0x00273B00 || cpu.pc == 0x00273BCE || cpu.pc == 0x00277650 || cpu.pc == 0x00272CE2 || cpu.pc == 0x00272B72 {
                    let ret = bus.read_long(cpu.a(7));
                    println!(">>> [Frame {} Line {} HIT SPECIAL PC=0x{:06X}] ret=0x{:06X}, D0=0x{:X}, A0=0x{:08X}, A6=0x{:08X}, 0x00D0007A=0x{:X}",
                        frame, line, cpu.pc, ret, cpu.d(0), cpu.a(0), cpu.a(6), cur_807a);
                }
                if cpu.pc == 0x00274454 {
                    let a6 = cpu.a(6);
                    let cur = bus.read_long(a6.wrapping_add(0xFFFF809A));
                    let end = bus.read_long(a6.wrapping_add(0xFFFF809E));
                    println!("   [Frame {} Line {} AT 0x274454] current=0x{:08X}, end=0x{:08X}", frame, line, cur, end);
                }
                if cpu.pc == 0x00274470 {
                    let a6 = cpu.a(6);
                    println!("   [0x274470 ENTRY] A6=0x{:08X}, 0x80D2(A6)=0x{:08X}, 0xC39A(A6)=0x{:04X}",
                        a6, bus.read_long(a6.wrapping_add(0xFFFF80D2)), bus.read_word(a6.wrapping_add(0xFFFFC39A)));
                }
                if cpu.pc == 0x002744AE {
                    let a6 = cpu.a(6);
                    println!("   [0x2744AE TST -$3C66(A6)] val=0x{:04X}", bus.read_word(a6.wrapping_add(0xFFFFC39A)));
                }
                if cpu.pc == 0x00274556 {
                    let a6 = cpu.a(6);
                    println!("   [0x274556 TST -$3C66(A6)] val=0x{:04X}", bus.read_word(a6.wrapping_add(0xFFFFC39A)));
                }
                if cpu.pc == 0x00274582 {
                    println!("   [0x274582 CALLING 0x275540]");
                }


                if cpu.pc == 0x0042AC4A {
                    let sp = cpu.a(7);
                    let ret = bus.read_long(sp);
                    let sig = cpu.d(1);
                    let a2 = cpu.a(2);
                    let proc_id = bus.read_word(a2 + 8);
                    println!("   [Frame {} CDFM SEND_SIG] sig=0x{:04X}, target_proc=0x{:X}, caller=0x{:06X}, A2=0x{:08X}",
                        frame, sig, proc_id, ret, a2);
                }

                if cpu.pc == 0x0042AC62 {
                    let sr = cpu.get_sr();
                    let carry = (sr & 1) != 0;
                    let err = cpu.d(1);
                    println!("   [Frame {} F$Send RETURNED] carry={}, D1=0x{:X}", frame, carry, err);
                }

                if cpu.pc == 0x00274DEE && (frame >= 2714 && frame <= 2950) {
                    let d0 = cpu.d(0);
                    let d1 = cpu.d(1);
                    let sp = cpu.a(7);
                    println!("   >>> [Frame {} ENTER 0x274DEE] D0=0x{:X}, D1=0x{:X}, SP=0x{:08X}, ret=0x{:08X}",
                        frame, d0, d1, sp, bus.read_long(sp));
                }

                if cpu.pc == 0x002751C0 && (frame >= 2714 && frame <= 2950) {
                    let head = bus.read_word(0x00D00180);
                    let tail = bus.read_word(0x00D00182);
                    let sig = bus.read_long(cpu.a(7) + 12);
                    println!("   >>> [Frame {} PUSH RING BUFFER] head={}, tail={}, sig=0x{:04X}",
                        frame, head, tail, sig);
                }

                if cpu.pc == 0x00275F50 && (frame >= 2714 && frame <= 2950) {
                    let head = bus.read_word(0x00D00180);
                    let tail = bus.read_word(0x00D00182);
                    let ring_base = bus.read_long(cpu.a(6).wrapping_sub(0x7E84));
                    let popped = bus.read_long(ring_base + (tail as u32) * 4);
                    println!("   >>> [Frame {} POP RING BUFFER] head={}, tail={}, popped_sig=0x{:04X}, ring_base=0x{:08X}",
                        frame, head, tail, popped, ring_base);
                }

                if cpu.pc == 0x00275F38 {
                    println!("   >>> [Frame {} CALLING SS_ABORT AT 0x275F38!] 4(SP)={}, 6(SP)={}, 1C(SP)={}, 1A(SP)={}",
                        frame, bus.read_word(cpu.a(7) + 4), bus.read_word(cpu.a(7) + 6),
                        bus.read_word(cpu.a(7) + 0x1C), bus.read_word(cpu.a(7) + 0x1A));
                }

                if cpu.pc == 0x00275EFC && (frame >= 2714 && frame <= 2950) {
                    println!("   >>> [Frame {} HIT 0x100C HANDLER AT 0x275EFC! CLR 8(SP)]", frame);
                }

                if cpu.pc == 0x00272B34 || cpu.pc == 0x00272B54 || cpu.pc == 0x00272B8C || cpu.pc == 0x00273156 || cpu.pc == 0x00274470 {
                    let a6 = cpu.a(6);
                    let flag_val = bus.read_word(a6.wrapping_add(0xFFFF807A));
                    println!("   >>> [Frame {} HIT PC=0x{:06X}] A6=0x{:08X}, $807A(A6)=0x{:04X}, D0=0x{:08X}",
                        frame, cpu.pc, a6, flag_val, cpu.d(0));
                }

                if cpu.pc == 0x00E54550 || cpu.pc == 0x00E51F3E || cpu.pc == 0x00E527F6 || cpu.pc == 0x00E52840 || cpu.pc == 0x00E52AFC {
                    println!("   >>> [Frame {} FMVDRV PC=0x{:06X}] A0=0x{:08X}, A2=0x{:08X}, A3=0x{:08X}, D0=0x{:08X}, D1=0x{:08X}",
                        frame, cpu.pc, cpu.a(0), cpu.a(2), cpu.a(3), cpu.d(0), cpu.d(1));
                }

                if cpu.pc == 0x00273EBC || cpu.pc == 0x00271BD2 || cpu.pc == 0x00276654 || cpu.pc == 0x0027668E || cpu.pc == 0x00277F8E || cpu.pc == 0x00272B54 || cpu.pc == 0x00272B56 || cpu.pc == 0x00276466 {
                    let a6 = cpu.a(6);
                    let addr_signed = a6.wrapping_add(0xFFFF807A);
                    let addr_unsigned = a6.wrapping_add(0x0000807A);
                    println!("   >>> [Frame {} HIT PC=0x{:06X}] A6=0x{:08X}, val@signed=0x{:04X}, val@unsigned=0x{:04X}, D0=0x{:08X}",
                        frame, cpu.pc, a6, bus.read_word(addr_signed), bus.read_word(addr_unsigned), cpu.d(0));
                }

                if cpu.pc == 0x0027EDE0 || cpu.pc == 0x0027EDE2 {
                    let sp = cpu.a(7);
                    println!("   >>> [Frame {} SIGNAL TRAMPOLINE 0x{:06X}] D0=0x{:X}, D1=0x{:X}, SP=0x{:08X}, ret=0x{:08X}",
                        frame, cpu.pc, cpu.d(0), cpu.d(1), sp, bus.read_long(sp));
                }

                if cpu.pc == 0x00275E88 && (frame >= 2715 && frame <= 2950) {
                    let head = bus.read_word(0x00D00180);
                    let tail = bus.read_word(0x00D00182);
                    let sp = cpu.a(7);
                    let ring_base = bus.read_long(cpu.a(6).wrapping_sub(0x7E84));
                    if head != tail || frame % 10 == 0 {
                        print!("   [Frame {} LOOP 0x275E88] head={}, tail={}, 8(SP)={}, 4(SP)={}, 6(SP)={}, 1C(SP)={}, 1A(SP)={} | ring[0..8]:",
                            frame, head, tail, bus.read_word(sp + 8), bus.read_word(sp + 4), bus.read_word(sp + 6),
                            bus.read_word(sp + 0x1C), bus.read_word(sp + 0x1A));
                        for i in 0..8 {
                            print!(" {:04X}", bus.read_long(ring_base + i * 4));
                        }
                        println!();
                    }
                }

                let batch = cpu.run_for_cycles(&mut bus, budget);
                line_cycles += batch.cycles + bus.consume_wait_cycles();

                if let Some((ref name, ret_pc)) = pending_open {
                    if cpu.pc == ret_pc {
                        let path = cpu.d(0) as u32;
                        println!("   -> [Frame {}] Opened '{}' as path {} (error={})",
                            frame, name, path, (cpu.get_sr() & 1) != 0);
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
                                    println!("[Frame {}] {} '{}' (mode=0x{:X}) -> caller PC=0x{:06X}",
                                        frame, if next_word == 0x0084 { "I$Open" } else { "I$Create" },
                                        name, cpu.d(0), caller_pc);
                                    pending_open = Some((name, caller_pc + 2));
                                }
                                0x008E => {
                                    let path = cpu.d(0) as u32;
                                    let code = cpu.d(1);
                                    let d2 = cpu.d(2);
                                    let d3 = cpu.d(3);
                                    let a0 = cpu.a(0);
                                    let pname = path_names.get(&path).cloned().unwrap_or_else(|| format!("Path {}", path));
                                    println!("[Frame {}] I$SetStt on '{}' (path {}): code=0x{:X}, D2=0x{:X}, D3=0x{:X}, A0=0x{:08X} at PC=0x{:06X}",
                                        frame, pname, path, code, d2, d3, a0, caller_pc);
                                    if caller_pc == 0x27A342 || caller_pc == 0x27A46A || caller_pc == 0x27A874 {
                                        let sp = cpu.a(7);
                                        println!("       STACK DUMP @ 0x{:06X}: SP=0x{:08X}: [0]=0x{:08X}, [4]=0x{:08X}, [8]=0x{:08X}, [12]=0x{:08X}, [16]=0x{:08X}, [20]=0x{:08X}",
                                            caller_pc, sp, bus.read_long(sp), bus.read_long(sp + 4), bus.read_long(sp + 8),
                                            bus.read_long(sp + 12), bus.read_long(sp + 16), bus.read_long(sp + 20));
                                    }
                                    if caller_pc == 0x27BEB0 {
                                        let sp = cpu.a(7);
                                        println!("       ENTER 0x27BEB0 (SS_Play on Path {})! SP=0x{:08X}, A0=0x{:08X}, D2=0x{:X}, D3=0x{:X}", path, sp, a0, d2, d3);
                                        for i in 0..10 {
                                            println!("         SP+{:02X}: 0x{:08X}", i * 4, bus.read_long(sp + i * 4));
                                        }
                                        println!("       PCB @ A0 (0x{:08X}):", a0);
                                        for i in 0..4 {
                                            let off = i * 16;
                                            println!("         +{:02X}: {:08X} {:08X} {:08X} {:08X}",
                                                off,
                                                bus.read_long(a0 + off),
                                                bus.read_long(a0 + off + 4),
                                                bus.read_long(a0 + off + 8),
                                                bus.read_long(a0 + off + 12));
                                        }
                                    }
                                }
                                0x008D => {
                                    let path = cpu.d(0) as u32;
                                    let code = cpu.d(1);
                                    let pname = path_names.get(&path).cloned().unwrap_or_else(|| format!("Path {}", path));
                                    if caller_pc == 0x27A43C {
                                        let sp = cpu.a(7);
                                        println!("[Frame {}] ENTER 0x27A43C (GetStt on Path {})! SP=0x{:08X}",
                                            frame, path, sp);
                                    }
                                    if trace_now && (code != 0 && code != 0x80000000) {
                                        println!("[Frame {}] I$GetStt on '{}' (path {}): code=0x{:X}, D2=0x{:X}, D3=0x{:X}, A0=0x{:08X} at PC=0x{:06X}",
                                            frame, pname, path, code, cpu.d(2), cpu.d(3), cpu.a(0), caller_pc);
                                    }
                                }
                                0x0007 => {
                                    println!("[Frame {}] F$SigSvc: handler=0x{:08X} at PC=0x{:06X}",
                                        frame, cpu.a(0), caller_pc);
                                }
                                0x0008 => {
                                    if trace_now {
                                        println!("[Frame {}] F$Send to proc=0x{:X}, sig=0x{:X} at PC=0x{:06X}",
                                            frame, cpu.d(0), cpu.d(1), caller_pc);
                                        if frame == 2716 {
                                            trace_kernel_steps = 150;
                                        }
                                    }
                                }
                                0x0088 => {
                                    let path = cpu.d(0) as u32;
                                    let pname = path_names.get(&path).cloned().unwrap_or_else(|| format!("Path {}", path));
                                    if trace_now {
                                        println!("[Frame {}] I$Read on '{}' (path {}): count={}, buf=0x{:08X} at PC=0x{:06X}",
                                            frame, pname, path, cpu.d(2), cpu.a(0), caller_pc);
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
                                    if trace_now && next_word != 0x0004 { // skip F$Sleep
                                        println!("[Frame {}] TRAP #0 0x{:04X} at PC=0x{:06X}", frame, next_word, caller_pc);
                                    }
                                }
                            }
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
                        line_cycles = cyc_per_line as i32;
                        break;
                    }
                    _ => {}
                }
            }

            bus.tick_slave(cyc_per_line);
            bus.tick_cdic(cyc_per_line);
            bus.tick_dvc(cyc_per_line);
            bus.tick_timer(cyc_per_line);
        }

        if frame >= 2770 && frame <= 2800 {
            println!("[Frame {} START] PC=0x{:06X}, SR=0x{:04X}", frame, cpu.pc, cpu.get_sr());
        }

        if frame >= 2900 && frame <= 3200 {
            if frame % 10 == 0 {
                println!("[Frame {}] LBA={}, cmd=0x{:02X}, disc_cmd=0x{:02X}, DVC fifo={}, decoded={}, playing={}, armed={}",
                    frame, bus.cdic.curr_lba, bus.cdic.command, bus.cdic.disc_command,
                    bus.dvc.dvc_stream_fifo.len(), bus.dvc.frames_decoded_count,
                    bus.dvc.video_playing, bus.dvc.video_armed);
            }
        }

        if bus.cdrom.is_some() && !disc_launched {
            let (effective_shell_base, shell_state) = if bus.dvc.is_fitted() {
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
                    println!(">>> [Frame {}] Shell BIOS click PLAY CD-I...", frame);
                    bus.slave.update_mouse(575, 335, 1);
                } else if shell_ready_count == 50 || shell_ready_count == 200 {
                    bus.slave.update_mouse(575, 335, 0);
                }
            }
            if ((cpu.pc >= 0x00010000 && cpu.pc < 0x00300000) || (cpu.pc >= 0x00D00000 && cpu.pc < 0x00E00000)) && frame > 250 {
                println!(">>> [Frame {}] Disc app running at PC=0x{:06X}", frame, cpu.pc);
                disc_launched = true;
                bus.slave.update_mouse(384, 280, 0);
            }
        }

        if frame == 2700 {
            println!(">>> [Frame 2700] USER PRESSES BUTTON 1 ON 'PRESS ANY BUTTON' SCREEN!");
            bus.slave.update_mouse(384, 280, 1);

            let mut dis_out = String::new();
            for &(start, end, label) in &[
                (0x274190u32, 0x274240u32, "Signal intercept handler at 0x2741AC"),
                (0x275400u32, 0x275560u32, "Dispatcher 0x275430"),
                (0x273B00u32, 0x273C20u32, "Scene transition dispatcher (0x273B00)"),
                (0x275600u32, 0x275850u32, "Intro Play caller and 0x2757A6 handler"),
                (0x274400u32, 0x274600u32, "Function 0x274470"),
                (0x272B00u32, 0x272E00u32, "Attract / Start Gameplay 0x272B00..0x272E00"),
                (0x271AEEu32, 0x271C00u32, "Input wait loop 0x271AEE"),
                (0x272900u32, 0x272B00u32, "Dispatcher 0x272900"),
                (0x275A00u32, 0x276300u32, "Functions 0x275A00 - 0x276300"),
                (0x276400u32, 0x276500u32, "Function at 0x276400 (calls before/after DVC config)"),
                (0x276500u32, 0x276660u32, "DVC Config function (0x276500)"),
                (0x27BE00u32, 0x27BF00u32, "SS wrappers at 0x27BE00"),
                (0x27EDC0u32, 0x27EE20u32, "Signal intercept wrapper at 0x27EDC0"),
                (0x0042AC00u32, 0x0042AD00u32, "cdfm F$Send routine at 0x42AC00"),
            ] {
                dis_out.push_str(&format!("--- Disassembling {} ---\n", label));
                let mut pc = start;
                while pc < end {
                    let op = bus.read_word(pc);
                    let (dis, len) = m68k::dasm::disassemble(pc, op, m68k::CpuType::SCC68070);
                    let bytes: Vec<String> = (0..len).map(|i| format!("{:02X}", bus.read_byte(pc + i as u32))).collect();
                    dis_out.push_str(&format!("  0x{:06X}: {:<16} {:<32}\n", pc, bytes.join(" "), dis));
                    pc += len as u32;
                }
            }
            fs::write("lair_disasm.txt", dis_out).unwrap();

            println!("--- Searching for references to 0x276090, 0x276500, 0x275B50 in plane_b ---");
            for offset in (0..bus.mcd212.plane_b.len() - 4).step_by(2) {
                let pc = 0x200000 + offset as u32;
                let word = ((bus.mcd212.plane_b[offset] as u16) << 8) | (bus.mcd212.plane_b[offset + 1] as u16);
                let next_word = ((bus.mcd212.plane_b[offset + 2] as u16) << 8) | (bus.mcd212.plane_b[offset + 3] as u16);
                if word == 0x6100 { // BSR.W
                    let target = (pc + 2).wrapping_add(next_word as i16 as i32 as u32);
                    if target == 0x276090 || target == 0x276500 || target == 0x275B50 {
                        println!("  Found BSR.W to 0x{:06X} at 0x{:06X}", target, pc);
                    }
                }
                let long = ((word as u32) << 16) | (next_word as u32);
                if long == 0x00276090 || long == 0x00276500 || long == 0x00275B50 {
                    println!("  Found Long 0x{:06X} at 0x{:06X}", long, pc);
                }
            }

            println!("--- Dumping RAM 0x00D05600..0x00D05900 at frame 2700 ---");
            for addr in (0x00D05600u32..0x00D05900u32).step_by(16) {
                let bytes: Vec<String> = (0..16).map(|i| format!("{:02X}", bus.read_byte(addr + i))).collect();
                println!("  0x{:08X}: {}", addr, bytes.join(" "));
            }
        } else if frame == 2720 {
            println!(">>> [Frame 2720] USER RELEASES BUTTON 1!");
            bus.slave.update_mouse(384, 280, 0);
        }

        if frame >= 2700 && frame <= 2850 {
            if bus.cdic.curr_lba != prev_lba {
                println!("[Frame {}] LBA changed to {}", frame, bus.cdic.curr_lba);
                prev_lba = bus.cdic.curr_lba;
            }
            if frame % 10 == 0 {
                println!("[Frame {}] LBA={}, cmd=0x{:02X}, disc_cmd=0x{:02X}, DVC fifo={}, decoded={}, playing={}, armed={}",
                    frame, bus.cdic.curr_lba, bus.cdic.command, bus.cdic.disc_command,
                    bus.dvc.dvc_stream_fifo.len(), bus.dvc.frames_decoded_count,
                    bus.dvc.video_playing, bus.dvc.video_armed);
            }
        }

        if frame == 3805 || frame == 3810 || frame == 3820 || frame == 3830 || frame == 3850 {
            println!(">>> SAVING FRAME {} (decoded={})", frame, bus.dvc.frames_decoded_count);
            save_bmp(&format!("frame_{:04}.bmp", frame), &bus.mcd212.framebuffer);
        }

        if frame == 2170 || frame == 2175 || frame == 2200 || frame == 2300 || frame == 2400 || frame == 2500 || frame == 2600 || frame == 2730 || frame == 2740 || frame == 2750 || frame == 2900 || frame == 2940 || frame == 3000 || frame == 3100 || frame == 3200 || frame == 3300 || frame == 3400 || frame == 3500 {
            save_bmp_dvc(&format!("dvc_fb_{:04}.bmp", frame), &bus.dvc.framebuffer[..], bus.dvc.video_width as u32, bus.dvc.video_height as u32);
            save_bmp(&format!("frame_{:04}.bmp", frame), &bus.mcd212.framebuffer);
            let icm = bus.mcd212.image_coding_method;
            let tcr = bus.mcd212.transparency_control;
            let po = bus.mcd212.plane_order;
            let dcr = bus.mcd212.dcr;
            let ddr = bus.mcd212.ddr;
            println!(">>> [Frame {} DIAGNOSTIC] ICM=0x{:08X}, TCR=0x{:08X}, PO=0x{:04X}, DCR=[0x{:04X}, 0x{:04X}], DDR=[0x{:04X}, 0x{:04X}]",
                frame, icm, tcr, po, dcr[0], dcr[1], ddr[0], ddr[1]);
            println!("    External video line[384] = 0x{:08X}, DVC FB center = 0x{:08X}",
                bus.mcd212.external_video_line[384], bus.dvc.framebuffer[(bus.dvc.video_height / 2) * bus.dvc.video_width + bus.dvc.video_width / 2]);
        }

        if frame % 500 == 0 {
            println!("[Frame {}] PC=0x{:06X} | DVC: fifo={}, decoded={}, dcr=[0x{:04X}, 0x{:04X}], icm=0x{:08X}, tc=0x{:08X}",
                frame, cpu.pc, bus.dvc.dvc_stream_fifo.len(), bus.dvc.frames_decoded_count,
                bus.mcd212.dcr[0], bus.mcd212.dcr[1], bus.mcd212.image_coding_method, bus.dvc.display_timecode
            );
            save_bmp(&format!("frame_{:04}.bmp", frame), &bus.mcd212.framebuffer);
        }
    }
    Ok(())
}

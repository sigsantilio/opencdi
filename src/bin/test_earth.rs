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
#[path = "../machine.rs"]
mod machine;
#[path = "../mcd212.rs"]
mod mcd212;
#[path = "../nvram.rs"]
mod nvram;
#[path = "../scc_timer.rs"]
mod scc_timer;
#[path = "../slave.rs"]
mod slave;

use std::path::{Path, PathBuf};
use crate::machine::Machine;

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

fn save_bmp(fb: &[u32], path: &str) -> std::io::Result<()> {
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

    std::fs::write(path, bmp)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let disc_path = PathBuf::from(r"D:\Emulatori\Mame\Software\cdimono1\Earth Command - The Future of Our World is in Your Hands (USA).chd");
    if !disc_path.exists() {
        eprintln!("Earth Command CHD not found at {:?}", disc_path);
        return Ok(());
    }

    let mut m = Machine::new();
    let bios_path = find_file("bios/cdi220b.rom").expect("cdi220b.rom missing");
    m.load_bios(&bios_path)?;

    if let Some(nvram_path) = find_file("bios/nvram_mk48t08.bin") {
        let _ = m.bus.nvram.load_from_file(&nvram_path);
    }
    if let Some(dvc_path) = find_file("dvc/vmpega.rom") {
        let _ = m.load_dvc_rom(&dvc_path);
    }

    m.load_disc(&disc_path)?;
    println!("Loaded CD-ROM: {:?}", disc_path);
    m.bus.set_video_mode(crate::mcd212::VideoMode::Ntsc);
    println!("Configured VideoMode::Ntsc");

    m.reset();

    let mut disc_launched = false;
    let mut shell_ready_count = 0;

    let artifact_dir = r"C:\Users\Gian\.gemini\antigravity\brain\3c367666-9892-4238-9468-c77864b7ddc9";

    for frame in 1..=8650 {
        if m.bus.cdrom.is_some() && !disc_launched {
            let (_base, shell_state) = if m.bus.dvc.is_fitted() {
                let s_dvc = m.bus.peek_long(0x00D08090u32.wrapping_add(0xFFFF9172)).unwrap_or(0);
                if s_dvc >= 2 {
                    (0x00D08090u32, s_dvc)
                } else {
                    (0x0000C000u32, m.bus.peek_long(0x0000C000u32.wrapping_add(0xFFFF9172)).unwrap_or(0))
                }
            } else {
                (0x0000C000u32, m.bus.peek_long(0x0000C000u32.wrapping_add(0xFFFF9172)).unwrap_or(0))
            };

            if shell_state == 2 {
                shell_ready_count += 1;
                let phase = shell_ready_count % 60;
                if phase == 1 {
                    println!(">>> [Frame {}] Shell BIOS pronta (stato 2): click PLAY CD-I at (575, 335)", frame);
                    m.bus.slave.update_mouse(575, 335, 1);
                } else if phase == 26 {
                    println!(">>> [Frame {}] Release PLAY CD-I click", frame);
                    m.bus.slave.update_mouse(575, 335, 0);
                }
            }

            if ((m.cpu.pc >= 0x00010000 && m.cpu.pc < 0x00300000) || (m.cpu.pc >= 0x00D00000 && m.cpu.pc < 0x00E00000)) && frame > 250 {
                println!(">>> [Frame {}] Applicazione disco in esecuzione in RAM (PC=0x{:06X})!", frame, m.cpu.pc);
                disc_launched = true;
                m.bus.slave.update_mouse(384, 280, 0);
            }
        }

        // Clicks to advance through screens and test menus
        if disc_launched {
            if frame < 2200 {
                if frame % 300 == 0 && frame >= 1400 {
                    println!(">>> [Frame {}] Click Button 1 to advance intro...", frame);
                    m.bus.slave.update_mouse(384, 280, 1);
                } else if frame % 300 == 20 && frame >= 1400 {
                    m.bus.slave.update_mouse(384, 280, 0);
                }
            } else {
                // Interactive menu testing
                match frame {
                    2400 => { println!(">>> [Frame {}] Moving to Button 1 (Rainbow)", frame); m.bus.slave.update_mouse(95, 145, 0); }
                    2450 => { println!(">>> [Frame {}] Clicking Button 1 (Rainbow)", frame); m.bus.slave.update_mouse(95, 145, 1); }
                    2470 => { m.bus.slave.update_mouse(95, 145, 0); }

                    3400 => { println!(">>> [Frame {}] Moving to Button 2 (Eye)", frame); m.bus.slave.update_mouse(95, 200, 0); }
                    3450 => { println!(">>> [Frame {}] Clicking Button 2 (Eye)", frame); m.bus.slave.update_mouse(95, 200, 1); }
                    3470 => { m.bus.slave.update_mouse(95, 200, 0); }

                    4400 => { println!(">>> [Frame {}] Moving to Button 3 (Grid)", frame); m.bus.slave.update_mouse(95, 255, 0); }
                    4450 => { println!(">>> [Frame {}] Clicking Button 3 (Grid)", frame); m.bus.slave.update_mouse(95, 255, 1); }
                    4470 => { m.bus.slave.update_mouse(95, 255, 0); }

                    5400 => { println!(">>> [Frame {}] Moving to Button 4 (Transmissions)", frame); m.bus.slave.update_mouse(95, 310, 0); }
                    5450 => { println!(">>> [Frame {}] Clicking Button 4 (Transmissions)", frame); m.bus.slave.update_mouse(95, 310, 1); }
                    5470 => { m.bus.slave.update_mouse(95, 310, 0); }

                    6400 => { println!(">>> [Frame {}] Moving to Button 5 (Next month / Arrow)", frame); m.bus.slave.update_mouse(95, 425, 0); }
                    6450 => { println!(">>> [Frame {}] Clicking Button 5 (Next month / Arrow)", frame); m.bus.slave.update_mouse(95, 425, 1); }
                    6470 => { m.bus.slave.update_mouse(95, 425, 0); }

                    7400 => { println!(">>> [Frame {}] Moving to North America (230, 200)", frame); m.bus.slave.update_mouse(230, 200, 0); }
                    7450 => { println!(">>> [Frame {}] Clicking North America", frame); m.bus.slave.update_mouse(230, 200, 1); }
                    7470 => { m.bus.slave.update_mouse(230, 200, 0); }

                    8400 => { println!(">>> [Frame {}] Moving to Europe (430, 180)", frame); m.bus.slave.update_mouse(430, 180, 0); }
                    8450 => { println!(">>> [Frame {}] Clicking Europe", frame); m.bus.slave.update_mouse(430, 180, 1); }
                    8470 => { m.bus.slave.update_mouse(430, 180, 0); }

                    _ => {}
                }
            }
        }

        if frame == 7600 {
            println!("=== Frame {} detailed analysis ===", frame);
            let icm_a = (m.bus.mcd212.image_coding_method & 0x0F) as u8;
            let icm_b = ((m.bus.mcd212.image_coding_method >> 8) & 0x0F) as u8;
            let dcr_a = m.bus.mcd212.dcr[0];
            let dcr_b = m.bus.mcd212.dcr[1];
            let dca_a = m.bus.mcd212.dca[0];
            let dca_b = m.bus.mcd212.dca[1];
            let dcp_a = m.bus.mcd212.get_dcp::<0>();
            let dcp_b = m.bus.mcd212.get_dcp::<1>();
            let csrw = m.bus.mcd212.csrw[0];
            let po = m.bus.mcd212.plane_order;
            let tc = m.bus.mcd212.transparency_control;
            let dyuv_a = m.bus.mcd212.dyuv_abs_start[0];
            let dyuv_b = m.bus.mcd212.dyuv_abs_start[1];
            println!("icm_a={:02X} icm_b={:02X} | dcr_a={:04X} dcr_b={:04X} | csrw={:04X} po={:08X} tc={:08X}",
                icm_a, icm_b, dcr_a, dcr_b, csrw, po, tc);
            println!("dcp_a=0x{:06X} dcp_b=0x{:06X} | dca_a=0x{:06X} dca_b=0x{:06X}",
                dcp_a, dcp_b, dca_a, dca_b);
            println!("dyuv_a=0x{:06X} dyuv_b=0x{:06X}", dyuv_a, dyuv_b);
            println!("matte_control: {:08X?}", m.bus.mcd212.matte_control);
            println!("icm_a={:02X} icm_b={:02X} | dcr_a={:04X} dcr_b={:04X} | csrw={:04X} po={:08X} tc={:08X}",
                icm_a, icm_b, dcr_a, dcr_b, csrw, po, tc);
            println!("dcp_a=0x{:06X} dcp_b=0x{:06X} | dca_a=0x{:06X} dca_b=0x{:06X}",
                dcp_a, dcp_b, dca_a, dca_b);

        }

        m.run_frame();

        if frame == 7500 {
            println!("=== Frame 7500 detailed analysis ===");
            println!("CPU PC={:08X} A5={:08X}", m.cpu.pc, m.cpu.dar[13]);
            let icm_a = (m.bus.mcd212.image_coding_method & 0x0F) as u8;
            let icm_b = ((m.bus.mcd212.image_coding_method >> 8) & 0x0F) as u8;
            let dcr_a = m.bus.mcd212.dcr[0];
            let dcr_b = m.bus.mcd212.dcr[1];
            let dca_a = m.bus.mcd212.dca[0];
            let dca_b = m.bus.mcd212.dca[1];
            let dcp_a = m.bus.mcd212.get_dcp::<0>();
            let dcp_b = m.bus.mcd212.get_dcp::<1>();
            let csrw = m.bus.mcd212.csrw[0];
            let po = m.bus.mcd212.plane_order;
            let tc = m.bus.mcd212.transparency_control;
            let dyuv_a = m.bus.mcd212.dyuv_abs_start[0];
            let dyuv_b = m.bus.mcd212.dyuv_abs_start[1];
            println!("icm_a={:02X} icm_b={:02X} | dcr_a={:04X} dcr_b={:04X} | csrw={:04X} po={:08X} tc={:08X}",
                icm_a, icm_b, dcr_a, dcr_b, csrw, po, tc);
            println!("dcp_a=0x{:06X} dcp_b=0x{:06X} | dca_a=0x{:06X} dca_b=0x{:06X}",
                dcp_a, dcp_b, dca_a, dca_b);
            println!("dyuv_a=0x{:06X} dyuv_b=0x{:06X}", dyuv_a, dyuv_b);

            let path_a = format!("{}\\plane_a_7500.bin", artifact_dir);
            let _ = std::fs::write(&path_a, &m.bus.mcd212.plane_a[..]);
            let path_b = format!("{}\\plane_b_7500.bin", artifact_dir);
            let _ = std::fs::write(&path_b, &m.bus.mcd212.plane_b[..]);
            let path_clut = format!("{}\\clut_7500.bin", artifact_dir);
            let clut_bytes: Vec<u8> = m.bus.mcd212.clut.iter().flat_map(|c| c.to_le_bytes()).collect();
            let _ = std::fs::write(&path_clut, &clut_bytes);
            println!("Saved plane_a_7500.bin, plane_b_7500.bin, clut_7500.bin");
        }

        if frame == 7600 {
            let path_a = format!("{}\\plane_a_7600.bin", artifact_dir);
            let _ = std::fs::write(&path_a, &m.bus.mcd212.plane_a[..]);
            let path_b = format!("{}\\plane_b_7600.bin", artifact_dir);
            let _ = std::fs::write(&path_b, &m.bus.mcd212.plane_b[..]);
            let path_clut = format!("{}\\clut_7600.bin", artifact_dir);
            let clut_bytes: Vec<u8> = m.bus.mcd212.clut.iter().flat_map(|c| c.to_le_bytes()).collect();
            let _ = std::fs::write(&path_clut, &clut_bytes);
            println!("Saved plane_a_7600.bin, plane_b_7600.bin, clut_7600.bin");
        }

        if frame >= 7560 && frame <= 7800 && frame % 50 == 0 {
            let path = format!("{}\\clean_head_frame_{}.bmp", artifact_dir, frame);
            let _ = save_bmp(m.framebuffer(), &path);
            println!("Saved clean_head_frame_{}.bmp", frame);
        }

        if frame == 7800 {
            println!("Stopping after frame 7800");
            break;
        }
    }

    println!("Simulation finished!");
    Ok(())
}

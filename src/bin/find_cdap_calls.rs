use std::fs;
use std::path::Path;

fn main() {
    let mut rom_bytes = None;
    for prefix in &["file utili/bios/cdi220b.rom", "../file utili/bios/cdi220b.rom", "../../file utili/bios/cdi220b.rom", "bios/cdi220b.rom"] {
        if let Ok(b) = fs::read(Path::new(prefix)) {
            rom_bytes = Some(b);
            break;
        }
    }
    let rom = rom_bytes.expect("ROM not found");

    let base = 0x295C0;
    let target_off = (0x42ACDC - 0x4295C0) as i32;

    println!("Searching for calls to +0x{:04X} in cdapdriv...", target_off);
    for off in 0..0x4000 {
        let p = base + off;
        if rom[p] == 0x61 && rom[p + 1] == 0x00 {
            let disp = i16::from_be_bytes([rom[p + 2], rom[p + 3]]) as i32;
            let dest = off as i32 + 2 + disp;
            if dest == target_off {
                println!("  BSR.W at +0x{:04X} (0x{:06X})", off, 0x07B530 + off as u32);
            }
        }
        if rom[p] == 0x61 && rom[p + 1] != 0x00 && rom[p + 1] != 0xFF {
            let disp = rom[p + 1] as i8 as i32;
            let dest = off as i32 + 2 + disp;
            if dest == target_off {
                println!("  BSR.S at +0x{:04X} (0x{:06X})", off, 0x07B530 + off as u32);
            }
        }
    }
}

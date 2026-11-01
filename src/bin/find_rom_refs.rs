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

    println!("Searching for references to 3FFE in BIOS ROM...");
    for i in 0..rom.len() - 1 {
        if rom[i] == 0x3F && rom[i + 1] == 0xFE {
            println!("  0x{:06X}", 0x400000 + i as u32);
        }
    }

    println!("\nSearching for references to 200C in BIOS ROM...");
    for i in 0..rom.len() - 1 {
        if rom[i] == 0x20 && rom[i + 1] == 0x0C {
            println!("  0x{:06X}", 0x400000 + i as u32);
        }
    }
}

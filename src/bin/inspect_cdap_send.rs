use std::fs;
use std::path::Path;
use m68k::dasm::disassemble;
use m68k::CpuType;

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
    println!("=== Disassembling cdapdriv +0x0FE0..+0x1040 ===");
    let mut off = base + 0x0FE0;
    while off < base + 0x1040 {
        let pc = 0x07B530 + (off - base) as u32;
        let op = u16::from_be_bytes([rom[off], rom[off + 1]]);
        let (text, len) = disassemble(pc, op, CpuType::SCC68070);
        let bytes: Vec<String> = (0..len as usize).map(|i| format!("{:02X}", rom[off + i])).collect();
        println!("0x{:06X} (+0x{:04X}): {:<20} {}", pc, off - base, bytes.join(" "), text);
        off += (len.max(2)) as usize;
    }
}

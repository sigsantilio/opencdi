use std::fs;
use m68k::dasm::disassemble;
use m68k::CpuType;

fn main() {
    let bin = fs::read("cdi_bd13.bin").expect("cdi_bd13.bin");
    let base_ram = 0xDE1080u32;

    println!("=== Disassembly around 0xDF4000..0xDF4100 ===");
    let mut off = (0xDF3FE0u32 - base_ram) as usize;
    while off < (0xDF40E0u32 - base_ram) as usize {
        let pc = base_ram + off as u32;
        let op = u16::from_be_bytes([bin[off], bin[off + 1]]);
        let (text, len) = disassemble(pc, op, CpuType::SCC68070);
        let bytes: Vec<String> = (0..len as usize).map(|i| format!("{:02X}", bin[off + i])).collect();
        println!("0x{:06X}: {:<20} {}", pc, bytes.join(" "), text);
        off += (len.max(2)) as usize;
    }
}

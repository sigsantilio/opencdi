use std::fs;

fn main() {
    let bin = fs::read("cdi_bd13.bin").expect("cdi_bd13.bin");
    let base_ram = 0xDE1080u32;

    println!("Searching for 0x200C in cdi_bd13.bin...");
    for i in 0..bin.len() - 1 {
        let w = u16::from_be_bytes([bin[i], bin[i + 1]]);
        if w == 0x200C {
            let pc = base_ram + i as u32;
            println!("  0x{:06X} (offset +0x{:05X})", pc, i);
        }
    }
}

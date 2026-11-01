use std::path::{Path, PathBuf};
use opencdi::cdrom::CdRom;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let disc_path = if Path::new("file utili/giochi/7th Guest, The (USA).chd").exists() {
        PathBuf::from("file utili/giochi/7th Guest, The (USA).chd")
    } else {
        PathBuf::from("../file utili/giochi/7th Guest, The (USA).chd")
    };
    let mut cd = CdRom::open(disc_path)?;
    let mut buf = [0u8; 2352];

    println!("Scanning 7th Guest directories...");
    for lba in 16..=2500 {
        if cd.read_sector(lba, &mut buf) {
            let data = &buf[24..2072];
            for i in 0..data.len().saturating_sub(16) {
                if &data[i..i+4] == b"T7G_" || &data[i..i+4] == b"VIRG" || &data[i..i+4] == b"TRIL" || &data[i..i+4] == b"BOOK" || &data[i..i+4] == b"INTR" || &data[i..i+4] == b"GHOS" {
                    let s = String::from_utf8_lossy(&data[i..i+24]);
                    println!("Found at LBA {}: {}", lba, s);
                }
            }
        }
    }
    Ok(())
}

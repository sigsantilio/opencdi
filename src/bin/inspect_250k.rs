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

    cd.read_sector(250305, &mut buf);
    println!("LBA 250305 submode=0x{:02X} chan={} coding=0x{:02X} data[24..32]={:02X?}",
        buf[18], buf[17], buf[19], &buf[24..32]);
    Ok(())
}

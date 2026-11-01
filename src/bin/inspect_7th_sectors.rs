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

    for lba in 2790..=2850 {
        if cd.read_sector(lba, &mut buf) {
            let sm = buf[18];
            let chan = buf[17];
            let coding = buf[19];
            println!("LBA {} submode=0x{:02X} chan={} coding=0x{:02X} data[24..32]={:02X?}",
                lba, sm, chan, coding, &buf[24..32]);
        }
    }
    Ok(())
}

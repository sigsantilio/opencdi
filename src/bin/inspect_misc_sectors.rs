use std::path::PathBuf;
use opencdi::cdrom::CdRom;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let disc_path = "D:/Emulatori/Mame/Software/cdimono1/Brain Dead 13 (Europe).chd";
    let mut cd = CdRom::open(PathBuf::from(disc_path))?;

    let mut buf = [0u8; 2352];
    for lba in 4085..=4095 {
        if cd.read_sector(lba, &mut buf) {
            println!("\n=== Sector {} ===", lba);
            println!("Header MSF: {:02X}:{:02X}:{:02X} Mode {:02X}", buf[12], buf[13], buf[14], buf[15]);
            println!("Subheader: File={:02X} Chan={:02X} Submode={:02X} Coding={:02X}",
                buf[16], buf[17], buf[18], buf[19]);
            let payload = &buf[24..24 + 128];
            print!("Payload first 128 bytes: ");
            for (i, &b) in payload.iter().enumerate() {
                if i % 16 == 0 { print!("\n  {:04X}: ", i); }
                print!("{:02X} ", b);
            }
            println!();
            // Also print ASCII if printable
            print!("ASCII: ");
            for &b in payload {
                if b >= 0x20 && b < 0x7F { print!("{}", b as char); } else { print!("."); }
            }
            println!();
        }
    }

    Ok(())
}

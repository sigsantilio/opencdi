#[path = "../cdrom.rs"]
mod cdrom;

use std::path::{Path, PathBuf};
use crate::cdrom::{CdRom, CD_SECTOR_RAW_SIZE};

fn find_file(rel: &str) -> Option<PathBuf> {
    for c in [
        Path::new(rel).to_path_buf(),
        Path::new("..").join(rel),
        Path::new("file utili").join(rel),
        Path::new("..").join("file utili").join(rel),
    ] {
        if c.exists() { return Some(c); }
    }
    None
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "giochi/Dragon's Lair (USA).chd".to_string());
    let game_path = find_file(&arg).expect("Game not found");
    let mut cd = CdRom::open(&game_path)?;
    println!("Scanning CD-i volume descriptor for: {:?}", game_path);

    let mut sector_buf = [0u8; CD_SECTOR_RAW_SIZE];
    let mut swapped = false;
    for lba in [16, 150, 166] {
        if cd.read_sector(lba, &mut sector_buf) {
            if sector_buf[0] == 0xFF && sector_buf[1] == 0x00 {
                swapped = true;
            }
            if swapped {
                for i in (0..CD_SECTOR_RAW_SIZE).step_by(2) {
                    sector_buf.swap(i, i + 1);
                }
            }
            let data = &sector_buf[24..24 + 2048];
            if &data[1..6] == b"CD-I " || &data[1..6] == b"CD001" {
                println!("Found Volume Descriptor at LBA {}: Type=0x{:02X}, ID='{}' (swapped={})",
                    lba, data[0], String::from_utf8_lossy(&data[1..6]), swapped);

                let root_rec = &data[140..178];
                let root_lba = u32::from_le_bytes([root_rec[2], root_rec[3], root_rec[4], root_rec[5]]);
                let root_size = u32::from_le_bytes([root_rec[10], root_rec[11], root_rec[12], root_rec[13]]);
                println!("Root directory: LBA {}, size {}", root_lba, root_size);
                dump_dir(&mut cd, root_lba, root_size, "", swapped);
                break;
            }
        }
    }

    Ok(())
}

fn dump_dir(cd: &mut CdRom, start_lba: u32, size: u32, indent: &str, swapped: bool) {
    let mut sector_buf = [0u8; CD_SECTOR_RAW_SIZE];
    let num_sectors = (size + 2047) / 2048;

    for s in 0..num_sectors {
        let lba = start_lba + s;
        if !cd.read_sector(lba, &mut sector_buf) { break; }
        if swapped {
            for i in (0..CD_SECTOR_RAW_SIZE).step_by(2) {
                sector_buf.swap(i, i + 1);
            }
        }
        let data = &sector_buf[24..24 + 2048];
        let mut offset = 0;
        while offset < 2048 {
            let rec_len = data[offset] as usize;
            if rec_len == 0 { break; }
            if offset + rec_len > 2048 || offset + 33 > 2048 { break; }

            let file_lba = u32::from_le_bytes([data[offset + 2], data[offset + 3], data[offset + 4], data[offset + 5]]);
            let file_size = u32::from_le_bytes([data[offset + 10], data[offset + 11], data[offset + 12], data[offset + 13]]);
            let flags = data[offset + 25];
            let name_len = data[offset + 32] as usize;
            let name = if name_len > 0 && offset + 33 + name_len <= 2048 {
                String::from_utf8_lossy(&data[offset + 33..offset + 33 + name_len]).to_string()
            } else {
                String::new()
            };

            let is_dir = (flags & 2) != 0;
            println!("{}{} (LBA {}, size {}, flags 0x{:02X})", indent, name, file_lba, file_size, flags);

            if is_dir && name != "." && name != ".." && !name.is_empty() && !name.starts_with('\0') {
                dump_dir(cd, file_lba, file_size, &format!("{}  ", indent), swapped);
            }

            offset += rec_len;
        }
    }
}

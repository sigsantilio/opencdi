use std::path::PathBuf;
use opencdi::cdrom::CdRom;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let disc_path = "D:/Emulatori/Mame/Software/cdimono1/Brain Dead 13 (Europe).chd";
    let mut cd = CdRom::open(PathBuf::from(disc_path))?;

    let mut buf = [0u8; 2352];
    for lba in 7685..=7700 {
        if cd.read_sector(lba, &mut buf) {
            let submode = buf[22];
            let coding = buf[23];
            let chan = buf[21] & 0x1F;
            let file_num = buf[20];
            let is_video = (submode & 0x40) != 0;
            let is_audio = (submode & 0x04) != 0;
            let is_data = (submode & 0x08) != 0;
            println!("LBA {}: file={}, chan={}, submode=0x{:02X} (V={}, A={}, D={}), coding=0x{:02X}, sync={:02X?} header={:02X?}",
                lba, file_num, chan, submode, is_video, is_audio, is_data, coding,
                &buf[0..4], &buf[16..20]);
        }
    }
    Ok(())
}

use std::path::PathBuf;
use opencdi::cdrom::CdRom;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let disc_path = "D:/Emulatori/Mame/Software/cdimono1/Brain Dead 13 (Europe).chd";
    let mut cd = CdRom::open(PathBuf::from(disc_path))?;

    let mut buf = [0u8; 2352];
    let mut first_video = None;
    let mut last_video = None;
    let mut first_audio = None;
    let mut last_audio = None;
    let mut video_ranges = Vec::new();
    let mut in_video = false;
    let mut range_start = 0;

    for lba in 2855..=4885 {
        if !cd.read_sector(lba, &mut buf) { break; }
        let submode = buf[18];
        let chan = buf[17];
        let is_video = (submode & 0x02) != 0;
        let is_audio = (submode & 0x04) != 0;

        if is_video {
            if first_video.is_none() { first_video = Some(lba); }
            last_video = Some(lba);
            if !in_video {
                in_video = true;
                range_start = lba;
            }
        } else {
            if in_video {
                in_video = false;
                video_ranges.push((range_start, lba - 1));
            }
        }

        if is_audio {
            if first_audio.is_none() { first_audio = Some((lba, chan)); }
            last_audio = Some((lba, chan));
        }
    }
    if in_video {
        video_ranges.push((range_start, 4885));
    }

    println!("First video sector: {:?}", first_video);
    println!("Last video sector: {:?}", last_video);
    println!("First audio sector: {:?}", first_audio);
    println!("Last audio sector: {:?}", last_audio);
    println!("Video ranges count: {}", video_ranges.len());
    for r in &video_ranges {
        println!("  Video range: LBA {} .. {} ({} sectors)", r.0, r.1, r.1 - r.0 + 1);
    }

    Ok(())
}

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

    let mut stream_start = None;
    let mut last_stream = 0;

    for lba in 2840..=100000 {
        if cd.read_sector(lba, &mut buf) {
            let is_mpeg = &buf[24..28] == b"\x00\x00\x01\xBA";
            if is_mpeg {
                if stream_start.is_none() {
                    stream_start = Some(lba);
                }
                last_stream = lba;
            } else if stream_start.is_some() && (lba - last_stream) > 10 {
                let len = last_stream - stream_start.unwrap() + 1;
                if len > 500 {
                    println!("Large MPEG Stream: LBA {} .. {} ({} sectors = {:.1}s)",
                        stream_start.unwrap(), last_stream, len, len as f32 / 75.0);
                }
                stream_start = None;
            }
        }
    }
    Ok(())
}

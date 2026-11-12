use std::path::Path;
use opencdi::machine::Machine;
use opencdi::mcd212::{SCREEN_WIDTH, SCREEN_HEIGHT};

fn save_bmp(fb: &[u32], path: &str) -> std::io::Result<()> {
    let width = SCREEN_WIDTH as i32;
    let height = SCREEN_HEIGHT as i32;
    let file_size = 54 + (width * height * 4) as u32;

    let mut bmp = Vec::with_capacity(file_size as usize);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&file_size.to_le_bytes());
    bmp.extend_from_slice(&[0, 0, 0, 0]);
    bmp.extend_from_slice(&54u32.to_le_bytes());

    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&width.to_le_bytes());
    bmp.extend_from_slice(&(-height).to_le_bytes()); // top-down
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&32u16.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(width * height * 4).to_le_bytes());
    bmp.extend_from_slice(&2835u32.to_le_bytes());
    bmp.extend_from_slice(&2835u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());

    for &pixel in fb {
        bmp.extend_from_slice(&pixel.to_le_bytes());
    }

    std::fs::write(path, bmp)
}

#[test]
fn test_pavarotti_cdi_ready_boot() {
    let bios_path = Path::new("..").join("file utili").join("bios").join("cdi220b.rom");
    if !bios_path.exists() {
        eprintln!("BIOS not found, skipping test");
        return;
    }
    let cue_path = Path::new("..").join("file utili").join("giochi").join("Pavarotti - O Sole Mio (Europe).cue");
    if !cue_path.exists() {
        eprintln!("Pavarotti CUE not found, skipping test");
        return;
    }

    let mut machine = Machine::new();
    machine.load_bios(&bios_path).expect("Failed to load BIOS");
    let dvc_path = Path::new("..").join("file utili").join("dvc").join("vmpega.rom");
    if dvc_path.exists() {
        let _ = machine.load_dvc(&dvc_path);
    }
    machine.load_disc(&cue_path).expect("Failed to load disc");
    machine.reset();

    // Verify disc type and volume ID
    {
        let cd = machine.bus.cdrom.as_ref().expect("Disc should be loaded");
        assert_eq!(cd.disc_type, 0x10, "CD-i Ready disc must be recognized as CD-i (0x10)");
        assert_eq!(cd.volume_id, "Pav1", "Volume ID must match Pav1");
    }

    // Run for 600 frames
    for _ in 0..1200 {
        machine.run_frame();
    }

    let fb = machine.framebuffer();
    let non_zero_pixels = fb.iter().filter(|&&p| (p & 0x00FFFFFF) != 0).count();
    println!("Frame 600 rendered pixels: {} / {}", non_zero_pixels, fb.len());
    let _ = save_bmp(fb, "pavarotti_frame1200.bmp");
    assert!(non_zero_pixels > 1000, "Screen should not be completely blank");
}

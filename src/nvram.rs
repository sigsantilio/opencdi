use std::fs;
use std::path::{Path, PathBuf};
use log::{info, warn};

pub struct Timekeeper {
    pub ram: [u8; 8192],
    pub dirty: bool,
    pub dirty_frames: u32,
    // Empty: no persistence (harness, libretro); the GUI host sets the file.
    pub save_path: PathBuf,
}

impl Timekeeper {
    pub fn new() -> Self {
        let mut tk = Self {
            ram: [0; 8192],
            dirty: false,
            dirty_frames: 0,
            save_path: PathBuf::new(),
        };
        tk.init_default();
        tk
    }

    pub fn load_from_file<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<()> {
        let p = path.as_ref();
        self.save_path = p.to_path_buf();
        let data = fs::read(p)?;
        let len = data.len().min(8192);
        self.ram[..len].copy_from_slice(&data[..len]);
        self.dirty = false;
        self.dirty_frames = 0;
        Ok(())
    }

    pub fn save_to_file(&mut self) -> std::io::Result<()> {
        if let Some(parent) = self.save_path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = fs::create_dir_all(parent);
            }
        }
        fs::write(&self.save_path, &self.ram[..])?;
        self.dirty = false;
        self.dirty_frames = 0;
        info!("NVRAM salvata su disco in {:?}", self.save_path);
        Ok(())
    }

    pub fn flush_if_dirty(&mut self) {
        if self.dirty && !self.save_path.as_os_str().is_empty() {
            if let Err(e) = self.save_to_file() {
                warn!("Impossibile salvare NVRAM in {:?}: {}", self.save_path, e);
            }
        }
    }

    pub fn clear(&mut self) -> std::io::Result<()> {
        self.ram.fill(0);
        self.init_default();
        self.dirty = true;
        self.flush_if_dirty();
        Ok(())
    }

    fn init_default(&mut self) {
        // MK48T08 timekeeper registers are at the top (8184..8191)
        // Set standard flags: Calibration = 0, Seconds = 0 (oscillator running, bit 7 = 0)
        self.ram[8184] = 0x00; // Control
        self.ram[8185] = 0x00; // Seconds (bit 7 = 0 enables oscillator)
        self.ram[8186] = 0x00; // Minutes
        self.ram[8187] = 0x12; // Hours (12:00)
        self.ram[8188] = 0x01; // Day of week
        self.ram[8189] = 0x01; // Date
        self.ram[8190] = 0x01; // Month
        self.ram[8191] = 0x95; // Year (1995)
    }

    pub fn read(&self, offset: u32) -> u8 {
        let idx = (offset as usize) % 8192;
        self.ram[idx]
    }

    pub fn write(&mut self, offset: u32, val: u8) {
        let idx = (offset as usize) % 8192;
        if self.ram[idx] != val {
            self.ram[idx] = val;
            // Mark dirty for NVRAM user storage area (0..8184)
            if idx < 8184 {
                self.dirty = true;
                self.dirty_frames = 0;
            }
        }
    }
}

opencdi::impl_state!(Timekeeper { ram } skip { dirty, dirty_frames, save_path });

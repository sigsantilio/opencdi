// 32 KB battery-backed RAM at 0x320000 on the high byte lane: one byte at every even address.
pub const NVRAM32_SIZE: usize = 32 * 1024;

/// DS1216 recognition pattern: 64 bits on data bit 0 of consecutive writes, least significant bit first.
pub const DS1216_PATTERN: u64 = 0x5CA3_3AC5_5CA3_3AC5;

/// Fixed power-on time, BCD: hundredths, seconds, minutes, hours (24 h), day (oscillator on), date, month, year (1995-01-01 12:00:00).
pub const DS1216_FIXED_TIME: [u8; 8] = [0x00, 0x00, 0x00, 0x12, 0x01, 0x01, 0x01, 0x95];

/// DS1216 SmartWatch under the RAM: a read resets the match, 64 matching writes open 64 clock cycles on data bit 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ds1216 {
    /// Clock registers, BCD; frozen so runs stay repeatable, changed only by a clock write.
    pub regs: [u8; 8],
    /// Pattern bits matched so far (0-64); 64 means the clock owns the next cycles.
    pub matched: u8,
    /// A write did not match; further writes are ignored until the next read.
    pub failed: bool,
    /// Clock cycles done since recognition (0-63).
    pub bit: u8,
    /// Bits written during the current clock cycles, committed after the 64th.
    pub shift: u64,
}

impl Default for Ds1216 {
    fn default() -> Self {
        Self { regs: DS1216_FIXED_TIME, matched: 0, failed: false, bit: 0, shift: 0 }
    }
}

impl Ds1216 {
    fn active(&self) -> bool {
        self.matched == 64
    }

    fn end_cycle(&mut self) {
        self.bit += 1;
        if self.bit == 64 {
            self.matched = 0;
            self.bit = 0;
        }
    }

    /// Read cycle: the next register bit while recognised, else `None` (a memory read, which resets the match).
    pub fn read(&mut self) -> Option<u8> {
        if !self.active() {
            self.matched = 0;
            self.failed = false;
            return None;
        }
        let b = (u64::from_le_bytes(self.regs) >> self.bit) & 1;
        self.end_cycle();
        Some(0xFE | b as u8)
    }

    /// Write cycle: true when the clock took it (memory inhibited), false when it goes to the RAM.
    pub fn write(&mut self, value: u8) -> bool {
        let b = (value & 1) as u64;
        if self.active() {
            self.shift |= b << self.bit;
            if self.bit == 63 {
                self.regs = self.shift.to_le_bytes();
                self.shift = 0;
            }
            self.end_cycle();
            return true;
        }
        if !self.failed {
            if b == (DS1216_PATTERN >> self.matched) & 1 {
                self.matched += 1;
                self.shift = 0;
            } else {
                self.matched = 0;
                self.failed = true;
            }
        }
        false
    }
}

pub struct Nvram32 {
    pub ram: Vec<u8>,
    pub clock: Ds1216,
    pub dirty: bool, // RAM changed since the last file write
    pub save_path: Option<std::path::PathBuf>, // file the RAM persists to; None keeps it in memory only
}

impl Default for Nvram32 {
    fn default() -> Self {
        Self { ram: vec![0; NVRAM32_SIZE], clock: Ds1216::default(), dirty: false, save_path: None }
    }
}

impl Nvram32 {

    /// Byte index for a bus offset inside the 64 KB window; `None` on the unconnected low byte lane.
    pub fn index(offset: u32) -> Option<usize> {
        if offset & 1 == 0 { Some((offset as usize >> 1) % NVRAM32_SIZE) } else { None }
    }

    /// Side-effect-free read of the RAM byte for traces; never goes through the clock.
    pub fn peek(&self, i: usize) -> u8 {
        self.ram[i]
    }

    /// CPU read of chip byte `i`; the clock sees every cycle of the chip.
    pub fn read(&mut self, i: usize) -> u8 {
        self.clock.read().unwrap_or(self.ram[i])
    }

    /// CPU write of chip byte `i`; pattern writes also reach the RAM, clock data writes do not.
    pub fn write(&mut self, i: usize, value: u8) {
        if !self.clock.write(value) && self.ram[i] != value {
            self.ram[i] = value;
            self.dirty = true;
        }
    }

    pub fn load_bytes(&mut self, data: &[u8]) {
        let len = data.len().min(NVRAM32_SIZE);
        self.ram[..len].copy_from_slice(&data[..len]);
    }

    /// Persist the RAM to `path` from now on, loading it first when the file exists; returns whether it was loaded.
    pub fn attach_file(&mut self, path: &std::path::Path) -> std::io::Result<bool> {
        self.save_path = Some(path.to_path_buf());
        self.dirty = false;
        if !path.is_file() {
            return Ok(false);
        }
        self.load_bytes(&std::fs::read(path)?);
        Ok(true)
    }

    /// Write the RAM to the attached file when it changed.
    pub fn flush_if_dirty(&mut self) {
        let Some(path) = self.save_path.as_ref().filter(|_| self.dirty) else { return };
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(path, &self.ram) {
            Ok(()) => {
                self.dirty = false;
                log::info!("Mono-IV NVRAM saved to {:?}", path);
            }
            Err(e) => log::warn!("Cannot save Mono-IV NVRAM to {:?}: {}", path, e),
        }
    }

    /// Empty the RAM and delete the attached file.
    pub fn clear(&mut self) {
        self.ram.fill(0);
        self.dirty = false;
        if let Some(path) = &self.save_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Nvram32 {
    pub fn save(&self, w: &mut Vec<u8>) {
        super::put_vec(w, &self.ram);
        let c = &self.clock;
        w.extend_from_slice(&c.regs);
        w.extend_from_slice(&[c.matched, c.failed as u8, c.bit]);
        w.extend_from_slice(&c.shift.to_le_bytes());
    }

    pub fn load(&mut self, r: &mut super::BlobReader) -> Option<()> {
        let ram = r.vec().filter(|v| v.len() == NVRAM32_SIZE)?;
        self.ram.copy_from_slice(&ram);
        let c = &mut self.clock;
        c.regs.copy_from_slice(r.bytes(8)?);
        (c.matched, c.failed, c.bit, c.shift) = (r.u8()?, r.bool()?, r.u8()?, r.u64()?);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_persists_to_attached_file() {
        let path = std::env::temp_dir().join(format!("opencdi-nvram490-{}.bin", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut a = Nvram32::default();
        assert!(!a.attach_file(&path).unwrap());
        a.write(10, 0x42);
        assert!(a.dirty);
        a.flush_if_dirty();
        assert!(!a.dirty && std::fs::metadata(&path).unwrap().len() == NVRAM32_SIZE as u64);
        let mut b = Nvram32::default();
        assert!(b.attach_file(&path).unwrap());
        assert_eq!(b.peek(10), 0x42);
        b.clear();
        assert!(!path.exists() && b.peek(10) == 0);
    }

    // Same sequence as the 490 clock module: save byte, 64 pattern writes, 64 reads, restore.
    fn bios_read(n: &mut Nvram32) -> (u32, u32) {
        let saved = n.read(0);
        let mut p: u32 = 0x5CA3_3AC5;
        for _ in 0..64 {
            n.write(0, p as u8);
            p = p.rotate_right(1);
        }
        let mut words = [0u32; 2];
        for w in words.iter_mut() {
            for _ in 0..32 {
                *w = (*w | (n.read(0) & 1) as u32).rotate_right(1);
            }
        }
        n.write(0, saved);
        (words[0], words[1])
    }

    #[test]
    fn ds1216_reads_fixed_time_and_leaves_ram() {
        let mut n = Nvram32::default();
        n.ram[0] = 0x5A;
        let (t, d) = bios_read(&mut n);
        assert_eq!((t, d), (0x1200_0000, 0x9501_0101));
        assert_eq!(n.ram[0], 0x5A);
        assert_eq!(n.read(0), 0x5A);
    }

    #[test]
    fn ds1216_mismatch_and_reads_keep_memory_mode() {
        let mut n = Nvram32::default();
        n.write(4, 0);
        for _ in 0..70 {
            n.write(4, 0xFF);
        }
        assert_eq!(n.read(4), 0xFF);
        let mut p = DS1216_PATTERN;
        for k in 0..64 {
            if k == 10 {
                n.read(2);
            }
            n.write(2, p as u8);
            p >>= 1;
        }
        assert!(!n.clock.active());
    }

    #[test]
    fn ds1216_write_sets_the_time() {
        let mut n = Nvram32::default();
        n.read(0);
        let mut p = DS1216_PATTERN;
        for _ in 0..64 {
            n.write(0, p as u8);
            p >>= 1;
        }
        let new = u64::from_le_bytes([0x00, 0x30, 0x45, 0x21, 0x13, 0x24, 0x12, 0x99]);
        for k in 0..64 {
            n.write(0, (new >> k) as u8 & 1);
        }
        assert_eq!(n.clock.regs, new.to_le_bytes());
        assert_eq!(n.ram[0], 0);
        let (t, d) = bios_read(&mut n);
        assert_eq!((t, d), (0x2145_3000, 0x9912_2413));
    }
}

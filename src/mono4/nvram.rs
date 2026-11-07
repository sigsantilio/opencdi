// 32 KB battery-backed RAM at 0x320000 on the high byte lane: one byte at every even address.
pub const NVRAM32_SIZE: usize = 32 * 1024;

pub struct Nvram32 {
    pub ram: Vec<u8>,
}

impl Default for Nvram32 {
    fn default() -> Self {
        let mut n = Self { ram: vec![0; NVRAM32_SIZE] };
        n.init_default();
        n
    }
}

impl Nvram32 {
    // Clock registers at the top hold a constant time (12:00, 1995-01-01), so runs stay repeatable.
    fn init_default(&mut self) {
        let top = NVRAM32_SIZE - 8;
        self.ram[top..].copy_from_slice(&[0x00, 0x00, 0x00, 0x12, 0x01, 0x01, 0x01, 0x95]);
    }

    /// Byte index for a bus offset inside the 64 KB window; `None` on the unconnected low byte lane.
    pub fn index(offset: u32) -> Option<usize> {
        if offset & 1 == 0 { Some((offset as usize >> 1) % NVRAM32_SIZE) } else { None }
    }

    pub fn load_bytes(&mut self, data: &[u8]) {
        let len = data.len().min(NVRAM32_SIZE);
        self.ram[..len].copy_from_slice(&data[..len]);
    }
}

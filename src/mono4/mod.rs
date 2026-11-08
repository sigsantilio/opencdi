// Mono-IV (CD-i 470/490) board peripherals: CIAP disc and audio interface, IKAT system controller, 32 KB NVRAM.
pub mod ciap;
pub mod ikat;
pub mod nvram;

use std::collections::HashSet;

/// Most distinct unmodelled accesses each device reports before it only counts them.
pub const LOUD_LIMIT: usize = 64;

/// Reports each unmodelled register or address once on stderr, up to `LOUD_LIMIT` keys.
#[derive(Debug, Default, Clone)]
pub struct Loud {
    seen: HashSet<u32>,
    pub count: u64,
}

impl Loud {
    /// Counts the access; true when it should be printed (first time for this key, below the limit).
    pub fn first(&mut self, key: u32) -> bool {
        self.count += 1;
        self.seen.len() < LOUD_LIMIT && self.seen.insert(key)
    }
}

/// The peripherals that exist only on the Mono-IV board.
#[derive(Default)]
pub struct Mono4 {
    pub ciap: ciap::Ciap,
    pub ikat: ikat::Ikat,
    pub nvram: nvram::Nvram32,
    pub unmapped: Loud,
    pub unmapped_w: Loud,
}

impl Mono4 {
    /// Power-on state of the chips; NVRAM keeps its contents.
    pub fn reset(&mut self) {
        self.ciap.reset();
        self.ikat.reset();
    }
}

/// Little-endian reader over a state blob; every read returns None past the end.
pub struct BlobReader<'a>(&'a [u8]);

impl<'a> BlobReader<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Self(b)
    }
    pub fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }
    pub fn u8(&mut self) -> Option<u8> {
        self.bytes(1).map(|b| b[0])
    }
    pub fn bool(&mut self) -> Option<bool> {
        self.u8().map(|b| b != 0)
    }
    pub fn u16(&mut self) -> Option<u16> {
        self.bytes(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn u32(&mut self) -> Option<u32> {
        self.bytes(4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Option<u64> {
        self.bytes(8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    }
    pub fn vec(&mut self) -> Option<Vec<u8>> {
        let n = self.u32()? as usize;
        self.bytes(n).map(<[u8]>::to_vec)
    }
    pub fn done(&self) -> bool {
        self.0.is_empty()
    }
}

pub fn put_vec(w: &mut Vec<u8>, v: &[u8]) {
    w.extend_from_slice(&(v.len() as u32).to_le_bytes());
    w.extend_from_slice(v);
}

impl Mono4 {
    /// Chip state plus the IMPEG cartridge registers, for savestates.
    pub fn save_blob(&self, dvc: &crate::dvc::Dvc) -> Vec<u8> {
        let mut w = Vec::new();
        self.ciap.save(&mut w);
        self.ikat.save(&mut w);
        self.nvram.save(&mut w);
        put_vec(&mut w, &dvc.impeg_regs[..]);
        w.extend_from_slice(&[dvc.impeg_pio as u8, dvc.impeg_hi_valid as u8]);
        w
    }

    pub fn load_blob(&mut self, blob: &[u8], dvc: &mut crate::dvc::Dvc) -> Option<()> {
        let mut r = BlobReader::new(blob);
        self.ciap.load(&mut r)?;
        self.ikat.load(&mut r)?;
        self.nvram.load(&mut r)?;
        let regs = r.vec().filter(|v| v.len() == dvc.impeg_regs.len())?;
        dvc.impeg_regs.copy_from_slice(&regs);
        (dvc.impeg_pio, dvc.impeg_hi_valid) = (r.bool()?, r.bool()?);
        r.done().then_some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_state_round_trips() {
        let mut dvc = crate::dvc::Dvc::new();
        let mut a = Mono4::default();
        a.ciap.ram[0x100] = 0xA5;
        a.ciap.regs[3] = 0x1234;
        a.ciap.lba = 77;
        a.ciap.audio_next = 1;
        a.ikat.mode = 3;
        a.ikat.pointer = (10, 20, 4);
        a.ikat.ports[2].cmd = vec![1, 2, 3];
        a.ikat.ports[1].reply.extend([9, 8]);
        a.nvram.ram[5] = 0x42;
        a.nvram.clock.matched = 17;
        dvc.impeg_pio = true;
        let blob = a.save_blob(&dvc);
        let (mut b, mut dvc2) = (Mono4::default(), crate::dvc::Dvc::new());
        assert!(b.load_blob(&blob, &mut dvc2).is_some());
        assert_eq!(b.save_blob(&dvc2), blob);
        assert!(b.ciap.ram[0x100] == 0xA5 && b.ciap.lba == 77 && b.nvram.ram[5] == 0x42 && dvc2.impeg_pio);
        assert!(b.load_blob(&blob[..blob.len() - 1], &mut dvc2).is_none());
    }
}

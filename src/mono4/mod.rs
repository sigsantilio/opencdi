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

opencdi::impl_state!(Mono4 { ciap, ikat, nvram } skip { unmapped, unmapped_w });

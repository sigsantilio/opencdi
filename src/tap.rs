// Env-gated debug taps (stderr, off by default) and always-on event counters; none of them changes emulated state.

/// Trace switches, read once when the bus is created.
#[derive(Debug, Default, Clone, Copy)]
pub struct Taps {
    pub mmio: bool,
    pub ikat: bool,
    pub ciap: bool,
    pub iack: bool,
    pub pc: bool,
}

impl Taps {
    /// `CDI_MMIO_TRACE` (bus window 0x300000-0x32FFFF), `CDI_IKAT_TRACE`, `CDI_CIAP_TRACE`, `CDI_IACK_TRACE`, `CDI_PC_TRACE`.
    pub fn from_env() -> Self {
        let on = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty() && v != "0");
        Self {
            mmio: on("CDI_MMIO_TRACE"),
            ikat: on("CDI_IKAT_TRACE"),
            ciap: on("CDI_CIAP_TRACE"),
            iack: on("CDI_IACK_TRACE"),
            pc: on("CDI_PC_TRACE"),
        }
    }
}

/// Counts that tests and the harness read; never saved in a state.
#[derive(Debug, Default, Clone)]
pub struct Counters {
    pub iack: [u64; 8],
    pub iack_this_frame: u64,
    pub iack_max_frame: u64,
    pub unmapped: u64,
    pub bus_errors: u64,
    pub frames: u64,
}

impl Counters {
    pub fn iack_total(&self) -> u64 {
        self.iack.iter().sum()
    }

    /// Closes a frame for the per-frame acknowledge maximum.
    pub fn end_frame(&mut self) {
        self.iack_max_frame = self.iack_max_frame.max(self.iack_this_frame);
        self.iack_this_frame = 0;
        self.frames += 1;
    }

    pub fn summary(&self) -> String {
        format!(
            "iack={} iack_l2={} iack_l4={} iack_l5={} iack_l6={} iack_max_frame={} unmapped={}",
            self.iack_total(), self.iack[2], self.iack[4], self.iack[5], self.iack[6], self.iack_max_frame, self.unmapped
        )
    }
}

/// True for the device window the `CDI_MMIO_TRACE` tap covers.
#[inline(always)]
pub fn in_mmio_window(addr: u32) -> bool {
    (0x300000..=0x32FFFF).contains(&addr)
}

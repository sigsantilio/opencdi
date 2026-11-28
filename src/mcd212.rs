use std::sync::atomic::{AtomicBool, Ordering};

static STUDIO_LEVELS: AtomicBool = AtomicBool::new(true);

/// Selects studio levels (black 16, white 235, passed through) or full range (16..235 stretched to 0..255).
pub fn set_studio_levels(on: bool) {
    STUDIO_LEVELS.store(on, Ordering::Relaxed);
}

pub fn studio_levels() -> bool {
    STUDIO_LEVELS.load(Ordering::Relaxed)
}

/// Maps one studio-level component to the selected output range.
pub fn level_out(v: i32, studio: bool) -> u32 {
    if studio { v.clamp(0, 255) as u32 } else { (((v - 16) * 298 + 128) >> 8).clamp(0, 255) as u32 }
}

/// Maps a full-range component (0..255) onto studio levels (16..235) for fixed display colours.
pub fn full_to_studio(c: u32) -> u32 {
    16 + (c * 219 + 127) / 255
}

/// Converts a fixed full-range ARGB colour (backdrop, cursor) to the selected output range.
pub fn fixed_color_out(argb: u32) -> u32 {
    if !studio_levels() {
        return argb;
    }
    let ch = |s: u32| full_to_studio((argb >> s) & 0xFF) << s;
    0xFF00_0000 | ch(16) | ch(8) | ch(0)
}

pub const SCREEN_WIDTH: usize = 768;
pub const SCREEN_HEIGHT: usize = 560;
pub const TOTAL_SCANLINES: usize = 312;
pub const CYCLES_PER_SCANLINE: u32 = 960; // 15 MHz CPU / 15,625 Hz (PAL default)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoMode {
    Pal,
    Ntsc,
}

impl VideoMode {
    #[inline]
    pub fn total_scanlines(self) -> usize {
        match self {
            VideoMode::Pal => 312,
            VideoMode::Ntsc => 262,
        }
    }

    // CPU clock is half the video crystal: 30 MHz (PAL) or 30.2098 MHz (NTSC).
    #[inline]
    pub fn cpu_clock_hz(self) -> u32 {
        match self {
            VideoMode::Pal => 15_000_000,
            VideoMode::Ntsc => 15_104_900,
        }
    }

    // 960 CPU cycles per line on both standards (15,625 Hz PAL, 15,734 Hz NTSC lines).
    #[inline]
    pub fn cycles_per_scanline(self) -> u32 {
        960
    }

    // One CD sector at 75 sectors per second: 200,000 cycles (PAL) or 201,398 (NTSC).
    #[inline]
    pub fn cycles_per_sector(self) -> u32 {
        self.cpu_clock_hz() / 75
    }

    #[inline]
    pub fn active_scanlines(self) -> usize {
        match self {
            VideoMode::Pal => 280,
            VideoMode::Ntsc => 240,
        }
    }

    #[inline]
    pub fn vblank_lines(self) -> usize {
        match self {
            VideoMode::Pal => 32,
            VideoMode::Ntsc => 22,
        }
    }

    #[inline]
    pub fn fps(self) -> f64 {
        match self {
            VideoMode::Pal => 50.0,
            VideoMode::Ntsc => 60.0,
        }
    }

    #[inline]
    pub fn frame_duration(self) -> std::time::Duration {
        match self {
            VideoMode::Pal => std::time::Duration::from_nanos(20_000_000), // 50 Hz (20.00ms)
            VideoMode::Ntsc => std::time::Duration::from_nanos(16_666_667), // 60 Hz (~16.67ms)
        }
    }
}

pub const CSR1R_DA: u8 = 0x80; // Display Active
pub const CSR1R_PA: u8 = 0x20; // Parity (field 1/2)

pub const CSR1W_ST_BIT: u16 = 1 << 1;
pub const CSR1W_DD_BIT: u16 = 3;
pub const CSR1W_DD2_MASK: u16 = 0x0300;
pub const CSR1W_DD2_SHIFT: u32 = 8;

pub const CSR2R_IT1: u8 = 0x04; // Interrupt 1
pub const CSR2R_IT2: u8 = 0x02; // Interrupt 2
pub const CSR2R_BE: u8 = 0x01;  // Bus Error
pub const CSRW_DI: u16 = 0x8000; // CSR1W/CSR2W: disable the channel's interrupt request (IT1/IT2 status still set)

pub const DCR_DCA_BIT: u16 = 1 << 8;
pub const DCR_ICA_BIT: u16 = 1 << 9;
pub const DCR_SM_BIT: u16 = 1 << 12; // Scan Mode: 0 = non-interlaced, 1 = interlaced
pub const DCR_FD_BIT: u16 = 1 << 13;
pub const DCR_CF_BIT: u16 = 1 << 14;
pub const DCR_DE_BIT: u16 = 1 << 15;

pub const DDR_FT: u16 = 0x0300;
pub const DDR_FT_BMP: u16 = 0x0000;
pub const DDR_FT_BMP2: u16 = 0x0100;
pub const DDR_FT_RLE: u16 = 0x0200;
pub const DDR_FT_MOSAIC: u16 = 0x0300;

pub const ICM_OFF: u8 = 0x0;
pub const ICM_CLUT8: u8 = 0x1;
pub const ICM_RGB555: u8 = 0x1;
pub const ICM_CLUT7: u8 = 0x3;
pub const ICM_CLUT77: u8 = 0x4;
pub const ICM_DYUV: u8 = 0x5;
pub const ICM_CLUT4: u8 = 0xB;

pub const TCR_ALWAYS: u8 = 0x0;
pub const TCR_KEY: u8 = 0x1;
pub const TCR_RGB: u8 = 0x2;
pub const TCR_MF0: u8 = 0x3;
pub const TCR_MF1: u8 = 0x4;
pub const TCR_MF0_KEY1: u8 = 0x5;
pub const TCR_MF1_KEY1: u8 = 0x6;
pub const TCR_DISABLE_MX: u32 = 0x0080_0000;
pub const ICM_EV: u32 = 1 << 18;

pub const CURCNT_COLOR: u32 = 0x0000_000F;
pub const CURCNT_CUW: u32 = 0x0000_8000;
pub const CURCNT_COF_SHIFT: u32 = 16;
pub const CURCNT_CON_SHIFT: u32 = 19;
pub const CURCNT_BLKC: u32 = 0x0040_0000;
pub const CURCNT_EN: u32 = 0x0080_0000;

pub const S_4BPP_DISPLAY_COLOR: [u32; 16] = [
    0xFF00_0000, 0xFF00_007B, 0xFF00_7B00, 0xFF00_7B7B,
    0xFF7B_0000, 0xFF7B_007B, 0xFF7B_7B00, 0xFF7B_7B7B,
    0xFF00_0000, 0xFF00_00F9, 0xFF00_F900, 0xFF00_F9F9,
    0xFFF9_0000, 0xFFF9_00F9, 0xFFF9_F900, 0xFFF9_F9F9,
];

pub struct Mcd212 {
    pub plane_a: Box<[u8; 512 * 1024]>,
    pub plane_b: Box<[u8; 512 * 1024]>,

    pub csrr: [u8; 2],  // [0] = CSR1R (status ch1), [1] = CSR2R (status ch2)
    pub csrw: [u16; 2], // [0] = CSR1W (control ch1), [1] = CSR2W (control ch2)
    pub dcr: [u16; 2],  // [0] = DCR1, [1] = DCR2
    pub vsr: [u16; 2],  // [0] = VSR1, [1] = VSR2
    pub ddr: [u16; 2],  // [0] = DDR1, [1] = DDR2
    pub dcp: [u16; 2],  // [0] = DCP1, [1] = DCP2
    pub dca: [u32; 2],  // [0] = DCA1, [1] = DCA2

    pub clut: [u32; 256],
    pub clut_bank: [u8; 2],
    pub image_coding_method: u32,
    pub transparency_control: u32,
    pub plane_order: u32,
    pub transparent_color: [u32; 2],
    pub mask_color: [u32; 2],
    pub dyuv_abs_start: [u32; 2],

    pub cursor_position: u32,
    pub cursor_control: u32,
    pub cursor_pattern: [u16; 16],
    pub matte_control: [u32; 8],
    pub backdrop_color: u32,
    pub mosaic_hold: [u32; 2],
    pub weight_latch: [u8; 2],
    pub weight_factor: [[u8; 768]; 2],
    pub matte_flag: [[bool; 768]; 2],

    pub blink_time: u16,
    pub blink_active: bool,

    pub s_4bpp_color: [u32; 16],
    pub delta_y_lut: [u8; 256],
    pub delta_uv_lut: [u8; 256],
    pub dyuv_limit_lut: [u32; 0x300],
    pub dyuv_u_to_b: [i16; 256],
    pub dyuv_u_to_g: [i16; 256],
    pub dyuv_v_to_g: [i16; 256],
    pub dyuv_v_to_r: [i16; 256],

    pub framebuffer: Box<[u32; SCREEN_WIDTH * SCREEN_HEIGHT]>,
    pub interlace_field: Box<[[u32; 768]; 312]>,

    pub scanline: usize,
    pub scanline_cycles: u32,
    pub vblank_irq: bool,
    pub video_mode: VideoMode,
    pub external_video_line: [u32; 768],
    pub external_video_present: [bool; 768],
    pub trace_dca: bool,
    // Executed ICA/DCA instructions (line, path, word) for the OS-9 tracer's display log; None when off.
    pub dc_log: Option<Vec<(u16, u8, u32)>>,
}

impl Mcd212 {
    pub fn new() -> Self {
        static S_4BPP_COLOR: [u32; 16] = [
            0xFF101010, 0xFF10107A, 0xFF107A10, 0xFF107A7A,
            0xFF7A1010, 0xFF7A107A, 0xFF7A7A10, 0xFF7A7A7A,
            0xFF101010, 0xFF1010E6, 0xFF10E610, 0xFF10E6E6,
            0xFFE61010, 0xFFE610E6, 0xFFE6E610, 0xFFE6E6E6,
        ];

        let mut delta_y_lut = [0u8; 256];
        let mut delta_uv_lut = [0u8; 256];
        static S_DYUV_DELTAS: [u8; 16] = [
            0, 1, 4, 9, 16, 27, 44, 79, 128, 177, 212, 229, 240, 247, 252, 255,
        ];
        for d in 0..256 {
            delta_y_lut[d] = S_DYUV_DELTAS[d & 15];
            delta_uv_lut[d] = S_DYUV_DELTAS[d >> 4];
        }

        let mut dyuv_limit_lut = [0u32; 0x300];
        for w in 0..0x300 {
            let limit = if w < 0x100 {
                0
            } else if w < 0x200 {
                (w - 0x100) as u32
            } else {
                0xFF
            };
            dyuv_limit_lut[w] = limit;
        }

        let mut dyuv_u_to_b = [0i16; 256];
        let mut dyuv_u_to_g = [0i16; 256];
        let mut dyuv_v_to_g = [0i16; 256];
        let mut dyuv_v_to_r = [0i16; 256];
        for sw in 0..256isize {
            dyuv_u_to_b[sw as usize] = ((444 * (sw - 128)) / 256) as i16;
            dyuv_u_to_g[sw as usize] = (-(86 * (sw - 128)) / 256) as i16;
            dyuv_v_to_g[sw as usize] = (-(179 * (sw - 128)) / 256) as i16;
            dyuv_v_to_r[sw as usize] = ((351 * (sw - 128)) / 256) as i16;
        }

        let mut mcd = Self {
            plane_a: vec![0u8; 512 * 1024].into_boxed_slice().try_into().unwrap(),
            plane_b: vec![0u8; 512 * 1024].into_boxed_slice().try_into().unwrap(),

            csrr: [0; 2],
            csrw: [0; 2],
            dcr: [0; 2],
            vsr: [0; 2],
            ddr: [0; 2],
            dcp: [0; 2],
            dca: [0; 2],

            clut: [0; 256],
            clut_bank: [0; 2],
            image_coding_method: 0,
            transparency_control: 0,
            plane_order: 0,
            transparent_color: [0; 2],
            mask_color: [0; 2],
            dyuv_abs_start: [0; 2],

            cursor_position: 0,
            cursor_control: 0,
            cursor_pattern: [0; 16],
            matte_control: [0; 8],
            backdrop_color: 0,
            mosaic_hold: [0; 2],
            weight_latch: [0; 2],
            weight_factor: [[0; 768]; 2],
            matte_flag: [[false; 768]; 2],

            blink_time: 0,
            blink_active: false,

            s_4bpp_color: S_4BPP_COLOR,
            delta_y_lut,
            delta_uv_lut,
            dyuv_limit_lut,
            dyuv_u_to_b,
            dyuv_u_to_g,
            dyuv_v_to_g,
            dyuv_v_to_r,

            framebuffer: vec![0u32; SCREEN_WIDTH * SCREEN_HEIGHT].into_boxed_slice().try_into().unwrap(),
            interlace_field: vec![[0u32; 768]; 312].into_boxed_slice().try_into().unwrap(),

            scanline: 0,
            scanline_cycles: 0,
            vblank_irq: false,
            video_mode: VideoMode::Pal,
            external_video_line: [0xFF00_0000; 768],
            external_video_present: [false; 768],
            trace_dca: false,
            dc_log: None,
        };
        mcd.init_default_palette();
        mcd
    }

    fn init_default_palette(&mut self) {
        for i in 0..256 {
            let gray = i as u32;
            self.clut[i] = 0xFF00_0000 | (gray << 16) | (gray << 8) | gray;
        }
    }

    pub fn reset(&mut self) {
        self.plane_a.fill(0);
        self.plane_b.fill(0);
        self.csrr = [0; 2];
        self.csrw = [0; 2];
        self.dcr = [0; 2];
        self.vsr = [0; 2];
        self.ddr = [0; 2];
        self.dcp = [0; 2];
        self.dca = [0; 2];
        self.clut_bank = [0; 2];
        self.image_coding_method = 0;
        self.transparency_control = 0;
        self.plane_order = 0;
        self.transparent_color = [0; 2];
        self.mask_color = [0; 2];
        self.dyuv_abs_start = [0; 2];
        self.cursor_position = 0;
        self.cursor_control = 0;
        self.cursor_pattern = [0; 16];
        self.matte_control = [0; 8];
        self.backdrop_color = 0;
        self.mosaic_hold = [0; 2];
        self.weight_latch = [0; 2];
        self.weight_factor = [[0; 768]; 2];
        self.matte_flag = [[false; 768]; 2];
        self.blink_time = 0;
        self.blink_active = false;
        self.framebuffer.fill(0);
        for line in self.interlace_field.iter_mut() {
            line.fill(0);
        }
        self.scanline = 0;
        self.scanline_cycles = 0;
        self.vblank_irq = false;
        self.external_video_line.fill(0xFF00_0000);
        self.external_video_present.fill(false);
        self.init_default_palette();
    }

    /// Interrupt output: IT1 or IT2 set while its channel's disable bit is clear.
    pub fn int_line(&self) -> bool {
        let it1 = (self.csrr[1] & CSR2R_IT1) != 0 && (self.csrw[0] & CSRW_DI) == 0;
        let it2 = (self.csrr[1] & CSR2R_IT2) != 0 && (self.csrw[1] & CSRW_DI) == 0;
        it1 || it2
    }

    pub fn read_reg(&mut self, offset: u32) -> u16 {
        match offset & 0x1E {
            0x00 => {
                // CSR2 Read (Offset 0x01 / 0x00)
                let data = self.csrr[1];
                self.csrr[1] &= !(CSR2R_IT1 | CSR2R_IT2);
                if (data & (CSR2R_IT1 | CSR2R_IT2)) != 0 {
                    self.vblank_irq = false;
                }
                (data as u16) | ((data as u16) << 8)
            }
            0x02 => self.dcr[1],
            0x04 => self.vsr[1],
            0x08 => self.ddr[1],
            0x0A => self.dcp[1],

            0x10 => {
                // CSR1 Read (Offset 0x11 / 0x10)
                let data = self.csrr[0];
                (data as u16) | ((data as u16) << 8)
            }
            0x12 => self.dcr[0],
            0x14 => self.vsr[0],
            0x18 => self.ddr[0],
            0x1A => self.dcp[0],

            _ => 0x0000,
        }
    }

    pub fn write_reg(&mut self, offset: u32, val: u16) {
        log::trace!(">>> MCD212 WRITE reg 0x{:02X} = 0x{:04X}", offset & 0x1E, val);
        match offset & 0x1E {
            0x00 => self.csrw[1] = val,
            0x02 => {
                self.dcr[1] = val;
            }
            0x04 => {
                self.vsr[1] = val;
            }
            0x08 => self.ddr[1] = val,
            0x0A => {
                self.dcp[1] = val;
                self.dca[1] = (self.dca[1] & 0xFFFF_0000) | (val as u32);
            }

            0x10 => self.csrw[0] = val,
            0x12 => {
                self.dcr[0] = val;
            }
            0x14 => self.vsr[0] = val,
            0x18 => self.ddr[0] = val,
            0x1A => {
                self.dcp[0] = val;
                self.dca[0] = (self.dca[0] & 0xFFFF_0000) | (val as u32);
            }

            _ => {}
        }
    }

    /// Calculates the DTACK delay cycles for VRAM access on Path 0 (Plane A) or Path 1 (Plane B)
    /// based on hardware bus arbitration between CPU and MCD-212 display scanout.
    /// Per Philips MCD-212 specification, DRAM accesses require 4 master clocks (2 SCC68070 clocks)
    /// during available system slots, plus additional wait-states when contending with display DMA.
    #[inline(always)]
    pub fn ram_dtack_cycles<const PATH: usize>(&self, line_cycle: u32) -> u32 {
        // No contending for Ch.1/Ch.2 timing slots if display is disabled or during VBLANK (display inactive)
        if (self.dcr[0] & DCR_DE_BIT) == 0 || (self.csrr[0] & CSR1R_DA) == 0 {
            return 2;
        }

        // No contending for Ch.1/Ch.2 timing slots if a relevant Path is disabled
        if (self.dcr[PATH] & DCR_ICA_BIT) == 0 {
            return 2;
        }

        let cyc_per_line = self.video_mode.cycles_per_scanline();
        let x = ((line_cycle as u64 * 480) / cyc_per_line.max(1) as u64) as u32;
        let y = self.scanline;
        let x_outside_active_display = x >= 408;

        // No contending during the final 8-pixel area on all lines
        if x >= 472 {
            return 2;
        }

        let ica_height = self.video_mode.vblank_lines();

        // No contending during the free-run area of ICA lines
        if y < ica_height && x_outside_active_display {
            return 2;
        }

        // No contending during the free-run area of DCA lines if DCA is disabled
        if (self.dcr[PATH] & DCR_DCA_BIT) == 0 && x_outside_active_display {
            return 2;
        }

        // System access is restricted to the last 5 out of every 16 CLKs (30 MHz master ticks).
        let slot_cycle = (line_cycle * 2) & 0x0F;
        if slot_cycle >= 11 {
            return 2;
        }

        // Stalled waiting for the system slot (1 to 5 CPU clocks)
        2 + (((11 - slot_cycle) + 1) >> 1)
    }

    /// Calculates the DTACK delay cycles for CD-RTOS Main ROM access based on CSR1W configuration.
    #[inline(always)]
    pub fn rom_dtack_cycles(&self) -> u32 {
        // Typical ROM access delay: 1 wait cycle (2 clocks)
        // With CSR1W DD configured, 0 to 2 wait cycles
        if (self.csrw[0] & (1 << CSR1W_DD_BIT)) == 0 {
            return 1;
        }
        let dd2 = ((self.csrw[0] & CSR1W_DD2_MASK) >> CSR1W_DD2_SHIFT) as usize;
        match dd2 & 3 {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => 1,
        }
    }

    #[inline(always)]
    pub fn get_vsr<const PATH: usize>(&self) -> u32 {
        (((self.dcr[PATH] & 0x3F) as u32) << 16) | (self.vsr[PATH] as u32)
    }

    #[inline(always)]
    pub fn set_vsr<const PATH: usize>(&mut self, value: u32) {
        self.vsr[PATH] = (value & 0xFFFF) as u16;
        self.dcr[PATH] = (self.dcr[PATH] & 0xFFC0) | (((value >> 16) & 0x3F) as u16);
    }

    #[inline(always)]
    pub fn get_dcp<const PATH: usize>(&self) -> u32 {
        (((self.ddr[PATH] & 0x3F) as u32) << 16) | (self.dcp[PATH] as u32)
    }

    #[inline(always)]
    pub fn set_dcp<const PATH: usize>(&mut self, value: u32) {
        self.dcp[PATH] = (value & 0xFFFF) as u16;
        self.ddr[PATH] = (self.ddr[PATH] & 0xFFC0) | (((value >> 16) & 0x3F) as u16);
    }

    #[inline(always)]
    pub fn set_display_parameters<const PATH: usize>(&mut self, value: u8) {
        self.ddr[PATH] = (self.ddr[PATH] & 0xF0FF) | (((value as u16) & 0x0F) << 8);
        self.dcr[PATH] = (self.dcr[PATH] & 0xF7FF) | (((value as u16) & 0x10) << 7);
    }

    pub fn set_register<const PATH: usize>(&mut self, reg: u8, val: u32) {
        match reg {
            0x80..=0xBF => {
                let clut_idx = (self.clut_bank[PATH] as usize) * 64 + ((reg - 0x80) as usize);
                if clut_idx < 256 {
                    self.clut[clut_idx] = 0xFF00_0000 | (val & 0x00FC_FCFC);
                }
            }
            0xC0 => {
                if PATH == 0 {
                    self.image_coding_method = val;
                }
            }
            0xC1 => {
                if PATH == 0 {
                    self.transparency_control = val;
                }
            }
            0xC2 => {
                if PATH == 0 {
                    self.plane_order = val & 7;
                }
            }
            0xC3 => {
                self.clut_bank[PATH] = if PATH != 0 {
                    2 | ((val & 1) as u8)
                } else {
                    (val & 3) as u8
                };
            }
            0xC4 => {
                if PATH == 0 {
                    self.transparent_color[0] = val & 0x00FC_FCFC;
                }
            }
            0xC6 => {
                if PATH == 1 {
                    self.transparent_color[1] = val & 0x00FC_FCFC;
                }
            }
            0xC7 => {
                if PATH == 0 {
                    self.mask_color[0] = val & 0x00FC_FCFC;
                }
            }
            0xC9 => {
                if PATH == 1 {
                    self.mask_color[1] = val & 0x00FC_FCFC;
                }
            }
            0xCA => {
                if PATH == 0 {
                    self.dyuv_abs_start[0] = val;
                }
            }
            0xCB => {
                if PATH == 1 {
                    self.dyuv_abs_start[1] = val;
                }
            }
            0xCD => {
                if PATH == 0 {
                    self.cursor_position = val;
                }
            }
            0xCE => {
                if PATH == 0 {
                    self.cursor_control = val;
                }
            }
            0xCF => {
                if PATH == 0 {
                    let idx = ((val >> 16) & 0x0F) as usize;
                    self.cursor_pattern[idx] = (val & 0xFFFF) as u16;
                }
            }
            0xD0..=0xD7 => {
                self.matte_control[(reg & 7) as usize] = val;
            }
            0xD8 => {
                if PATH == 0 {
                    self.backdrop_color = val;
                }
            }
            0xD9 => {
                if PATH == 0 {
                    self.mosaic_hold[0] = val;
                }
            }
            0xDA => {
                if PATH == 1 {
                    self.mosaic_hold[1] = val;
                }
            }
            0xDB => {
                if PATH == 0 {
                    self.weight_latch[0] = (val & 0x3F) as u8;
                }
            }
            0xDC => {
                if PATH == 1 {
                    self.weight_latch[1] = (val & 0x3F) as u8;
                }
            }
            _ => {}
        }
    }

    /// Evaluates matte weight factors and region flags for the current active scanline.
    /// Per MCD-212 silicon (`mcd212rev0.pdf` p. 62), weight latches persist across scanlines
    /// while matte region flags (`MF0`/`MF1`) reset to `false` at `x = 0` of each scanline.
    fn evaluate_line_mattes(&mut self) {
        let width = 768;
        let num_mattes = if (self.image_coding_method & (1 << 19)) != 0 { 2 } else { 1 };

        let mut latched_mf = [false, false];
        let mut latched_wf = self.weight_latch;
        let mut matte_idx = [0usize, 4usize];

        for x in 0..width {
            for matte in 0..num_mattes {
                let max_matte_id = (if num_mattes == 2 { 4 } else { 8 }) + (if matte != 0 { 4 } else { 0 });
                if matte_idx[matte] >= max_matte_id {
                    continue;
                }
                let matte_ctrl = self.matte_control[matte_idx[matte]];
                if x == ((matte_ctrl & 0x3FF) as usize) {
                    let matte_op = ((matte_ctrl >> 20) & 0x0F) as u8;
                    let wf = ((matte_ctrl >> 10) & 0x3F) as u8;
                    let flag = if num_mattes == 2 { matte } else { ((matte_ctrl >> 16) & 1) as usize };
                    match matte_op {
                        0 => matte_idx[matte] = 8,
                        4 | 6 => latched_wf[((matte_op >> 1) & 1) as usize] = wf,
                        8 | 9 => latched_mf[flag] = (matte_op & 1) != 0,
                        12..=15 => {
                            latched_wf[((matte_op >> 1) & 1) as usize] = wf;
                            latched_mf[flag] = (matte_op & 1) != 0;
                        }
                        _ => {}
                    }
                    matte_idx[matte] += 1;
                }
            }
            self.weight_factor[0][x] = latched_wf[0];
            self.weight_factor[1][x] = latched_wf[1];
            self.matte_flag[0][x] = latched_mf[0];
            self.matte_flag[1][x] = latched_mf[1];
        }
        self.weight_latch = latched_wf;
    }

    #[inline]
    pub fn effective_active_scanlines(&self) -> usize {
        if self.video_mode == VideoMode::Ntsc || (self.dcr[0] & DCR_FD_BIT) != 0 {
            240
        } else {
            280
        }
    }

    #[inline(always)]
    pub fn get_screen_width(&self) -> usize {
        if (self.dcr[0] & DCR_CF_BIT) == 0 || (self.csrw[0] & CSR1W_ST_BIT) != 0 {
            720
        } else {
            768
        }
    }

    #[inline(always)]
    pub fn get_border_width(&self) -> usize {
        if (self.dcr[0] & DCR_CF_BIT) == 0 || (self.csrw[0] & CSR1W_ST_BIT) != 0 {
            24
        } else {
            0
        }
    }

    pub fn tick_scanline(&mut self, line: usize) -> bool {
        self.scanline = line;
        let mut frame_completed = false;
        let vblank_lines = self.video_mode.vblank_lines();
        let active_lines = self.effective_active_scanlines();

        if line == 0 {
            // End of frame / Start of VBLANK
            self.csrr[0] &= !CSR1R_DA;

            // Process ICA: Path 1 and Path 0 run in lockstep (one instruction per step; Path 0 wins same-step ties)
            self.process_ica();

            if (self.dcr[0] & DCR_DCA_BIT) != 0 {
                self.dca[0] = self.get_dcp::<0>();
            }
            if (self.dcr[1] & DCR_DCA_BIT) != 0 {
                self.dca[1] = self.get_dcp::<1>();
            }

            // Cursor Blink
            self.blink_time += 5 + if (self.dcr[0] & DCR_FD_BIT) != 0 { 1 } else { 0 };
            let con = ((self.cursor_control >> CURCNT_CON_SHIFT) & 7) as u16;
            let cof = ((self.cursor_control >> CURCNT_COF_SHIFT) & 7) as u16;
            if !self.blink_active && con > 0 && self.blink_time >= con * 60 {
                self.blink_active = true;
                self.blink_time = 0;
            }
            if self.blink_active && (cof == 0 || self.blink_time >= cof * 60) {
                self.blink_active = false;
                self.blink_time = 0;
            }

            frame_completed = true;
        } else if line >= vblank_lines && line < vblank_lines + active_lines {
            // Active display lines (DCA runs during HBLANK after VSR scanout in render_scanline)
            self.csrr[0] |= CSR1R_DA;
        } else {
            // VBLANK lines
            self.csrr[0] &= !CSR1R_DA;
        }

        frame_completed
    }

    pub fn render_line(&mut self, line: usize) {
        let total_lines = self.video_mode.total_scanlines();
        let vblank_lines = self.video_mode.vblank_lines();
        let active_lines = self.effective_active_scanlines();

        if line >= vblank_lines && line < total_lines {
            let active_line = line - vblank_lines;
            if active_line < active_lines {
                self.render_scanline(active_line);
            }

            if line == total_lines - 1 {
                self.csrr[0] ^= CSR1R_PA;
            }
        }
    }

    pub fn tick_cycles(&mut self, cycles: u32) -> bool {
        self.scanline_cycles += cycles;
        let mut frame_completed = false;
        let cyc_per_line = self.video_mode.cycles_per_scanline();
        let total_lines = self.video_mode.total_scanlines();

        while self.scanline_cycles >= cyc_per_line {
            self.scanline_cycles -= cyc_per_line;
            let next_line = (self.scanline + 1) % total_lines;
            frame_completed |= self.tick_scanline(next_line);
        }

        frame_completed
    }

    #[inline(always)]
    fn read_dc_word<const PATH: usize>(&self, byte_addr: usize) -> Option<u32> {
        let plane = if PATH == 0 { &self.plane_a[..] } else { &self.plane_b[..] };
        let bytes: [u8; 4] = plane.get(byte_addr..byte_addr + 4)?.try_into().ok()?;
        Some(u32::from_be_bytes(bytes))
    }

    fn step_ica<const PATH: usize>(&mut self, addr: &mut u32) -> bool {
        let Some(cmd) = self.read_dc_word::<PATH>((*addr as usize) * 2) else {
            return false;
        };
        *addr += 2;
        if let Some(log) = self.dc_log.as_mut() { log.push((self.scanline as u16, PATH as u8, cmd)); }

        match (cmd >> 24) as u8 {
            0x00..=0x0F => false, // STOP
            0x10..=0x1F => true,  // NOP
            0x20..=0x2F => {
                // RELOAD DCP
                self.set_dcp::<PATH>(cmd & 0x003F_FFFC);
                true
            }
            0x30..=0x3F => {
                // RELOAD DCP and STOP
                self.set_dcp::<PATH>(cmd & 0x003F_FFFC);
                false
            }
            0x40..=0x4F => {
                // RELOAD VSR (ICA jump)
                *addr = ((cmd & 0x0007_FFFF) / 2) as u32;
                true
            }
            0x50..=0x5F => {
                // RELOAD VSR and STOP
                self.set_vsr::<PATH>(cmd & 0x003F_FFFF);
                false
            }
            0x60..=0x6F => {
                // INTERRUPT
                self.csrr[1] |= 1 << (2 - PATH);
                self.vblank_irq |= (self.csrw[PATH] & CSRW_DI) == 0;
                true
            }
            0x78..=0x7F => {
                // RELOAD DISPLAY PARAMETERS
                self.set_display_parameters::<PATH>((cmd & 0x1F) as u8);
                true
            }
            reg => {
                self.set_register::<PATH>(reg, cmd & 0x00FF_FFFF);
                true
            }
        }
    }

    fn process_ica(&mut self) {
        // LCT depends on current frame parity
        let is_interlaced = (self.dcr[0] & DCR_SM_BIT) != 0;
        let start = if is_interlaced && (self.csrr[0] & CSR1R_PA) != 0 {
            0x202
        } else {
            0x200
        };
        let mut addr = [start, start];
        let mut active = [
            (self.dcr[0] & DCR_ICA_BIT) != 0,
            (self.dcr[1] & DCR_ICA_BIT) != 0,
        ];

        let max_to_process = 32 * 120;
        for _ in 0..max_to_process {
            if !active[0] && !active[1] {
                break;
            }
            if active[1] {
                active[1] = self.step_ica::<1>(&mut addr[1]);
            }
            if active[0] {
                active[0] = self.step_ica::<0>(&mut addr[0]);
            }
        }
    }

    fn step_dca<const PATH: usize>(&mut self, addr: &mut usize) -> (bool, bool) {
        let Some(cmd) = self.read_dc_word::<PATH>(*addr * 2) else {
            return (false, false);
        };
        *addr += 2;
        if let Some(log) = self.dc_log.as_mut() { log.push((self.scanline as u16, PATH as u8, cmd)); }

        match (cmd >> 24) as u8 {
            0x00..=0x0F => (false, false), // STOP
            0x10..=0x1F => (true, false),  // NOP
            0x20..=0x2F => (true, false),  // RELOAD DCP (NOP in DCA)
            0x30..=0x3F => {
                // RELOAD DCP and STOP
                self.set_dcp::<PATH>(cmd & 0x003F_FFFC);
                self.dca[PATH] = cmd & 0x0007_FFFC;
                (false, true)
            }
            0x40..=0x4F => {
                // RELOAD VSR
                self.set_vsr::<PATH>(cmd & 0x003F_FFFF);
                (true, false)
            }
            0x50..=0x5F => {
                // RELOAD VSR and STOP
                self.set_vsr::<PATH>(cmd & 0x003F_FFFF);
                (false, false)
            }
            0x60..=0x6F => {
                // INTERRUPT
                self.csrr[1] |= 1 << (2 - PATH);
                self.vblank_irq |= (self.csrw[PATH] & CSRW_DI) == 0;
                (true, false)
            }
            0x78..=0x7F => {
                // RELOAD DISPLAY PARAMETERS
                self.set_display_parameters::<PATH>((cmd & 0x1F) as u8);
                (true, false)
            }
            reg => {
                self.set_register::<PATH>(reg, cmd & 0x00FF_FFFF);
                (true, false)
            }
        }
    }

    fn process_dca(&mut self) {
        let start_addr = [
            ((self.dca[0] & 0x0007_FFFC) / 2) as usize,
            ((self.dca[1] & 0x0007_FFFC) / 2) as usize,
        ];
        let mut addr = start_addr;
        let mut active = [
            (self.dcr[0] & DCR_DCA_BIT) != 0,
            (self.dcr[1] & DCR_DCA_BIT) != 0,
        ];
        let mut reloaded_dcp = [false, false];

        for _ in 0..16 {
            if !active[0] && !active[1] {
                break;
            }
            if active[1] {
                let (cont, rel) = self.step_dca::<1>(&mut addr[1]);
                active[1] = cont;
                reloaded_dcp[1] |= rel;
            }
            if active[0] {
                let (cont, rel) = self.step_dca::<0>(&mut addr[0]);
                active[0] = cont;
                reloaded_dcp[0] |= rel;
            }
        }

        if (self.dcr[0] & DCR_DCA_BIT) != 0 && !reloaded_dcp[0] {
            self.dca[0] = ((start_addr[0] + 32) * 2) as u32;
        }
        if (self.dcr[1] & DCR_DCA_BIT) != 0 && !reloaded_dcp[1] {
            self.dca[1] = ((start_addr[1] + 32) * 2) as u32;
        }
    }

    #[inline(always)]
    fn get_icm<const PATH: usize>(&self) -> u8 {
        if PATH == 0 {
            (self.image_coding_method & 0x0F) as u8
        } else {
            ((self.image_coding_method >> 8) & 0x0F) as u8
        }
    }

    #[inline(always)]
    fn get_transparency_control<const PATH: usize>(&self) -> u8 {
        if PATH == 0 {
            (self.transparency_control & 0x0F) as u8
        } else {
            ((self.transparency_control >> 8) & 0x0F) as u8
        }
    }

    #[inline(always)]
    fn byte_to_clut<const PATH: usize>(icm: u8, byte: u8, clut_select: bool) -> u8 {
        match icm {
            1 => byte, // CLUT8
            3 => (if PATH != 0 { 0x80 } else { 0 }) | (byte & 0x7F), // CLUT7
            4 => {
                if PATH == 0 {
                    (if clut_select { 0x80 } else { 0 }) | (byte & 0x7F)
                } else {
                    0
                }
            }
            11 => (if PATH != 0 { 0x80 } else { 0 }) | (byte & 0x0F), // CLUT4
            _ => 0,
        }
    }

    pub fn process_vsr<const PATH: usize>(&mut self, pixels: &mut [u32; 768], transparent: &mut [bool; 768]) {
        let icm = self.get_icm::<PATH>();
        let tp_ctrl = self.get_transparency_control::<PATH>();
        let width = self.get_screen_width();

        let mut vsr = self.get_vsr::<PATH>();
        let _other_path = if PATH == 0 { 1 } else { 0 };
        let mut vsr2 = if PATH == 1 { self.get_vsr::<0>() } else { self.get_vsr::<1>() };

        if icm == 0 || vsr == 0 {
            pixels.fill(self.s_4bpp_color[0]);
            let tp_ctrl_type = tp_ctrl & 0x07;
            let tp_check_parity = (tp_ctrl & 0x08) == 0;
            let tp_always = (tp_ctrl_type == TCR_ALWAYS) && tp_check_parity;
            transparent.fill(tp_always);
            return;
        }

        let decoding_mode = self.ddr[PATH] & DDR_FT;
        let mosaic_enable = (self.ddr[PATH] & DDR_FT) == DDR_FT_MOSAIC;
        let mosaic_factor = 1usize << (((self.ddr[PATH] & 0x0C00) >> 10) + 1);

        let dyuv_abs_start = self.dyuv_abs_start[PATH];
        let mut y = ((dyuv_abs_start >> 16) & 0xFF) as u8;
        let mut u = ((dyuv_abs_start >> 8) & 0xFF) as u8;
        let mut v = (dyuv_abs_start & 0xFF) as u8;

        let mask_bits = (!self.mask_color[PATH]) & 0x00FC_FCFC;
        let tp_color_match = self.transparent_color[PATH] & mask_bits;
        let tp_ctrl_type = tp_ctrl & 0x07;

        let use_rgb_tp_bit = tp_ctrl_type == TCR_RGB;
        let tp_check_parity = (tp_ctrl & 0x08) == 0;
        let tp_always = (tp_ctrl_type == TCR_ALWAYS) && tp_check_parity;
        let matte_flag_index = ((!tp_ctrl_type) & 1) as usize;
        let use_matte_flag = tp_ctrl_type >= TCR_MF0 && tp_ctrl_type <= TCR_MF1_KEY1;
        let is_dyuv_rgb = (icm == ICM_DYUV) || ((icm == ICM_RGB555) && (PATH == 1));
        let use_color_key = !is_dyuv_rgb && ((tp_ctrl_type == TCR_KEY) || (tp_ctrl_type == TCR_MF0_KEY1) || (tp_ctrl_type == TCR_MF1_KEY1));

        if icm == ICM_DYUV {
            let mut x = 0;
            while x < width {
                let byte0 = if PATH == 0 { self.plane_a[(vsr & 0x0007_FFFF) as usize] } else { self.plane_b[(vsr & 0x0007_FFFF) as usize] };
                vsr += 1;
                let byte1 = if PATH == 0 { self.plane_a[(vsr & 0x0007_FFFF) as usize] } else { self.plane_b[(vsr & 0x0007_FFFF) as usize] };
                vsr += 1;

                let y2 = y.wrapping_add(self.delta_y_lut[byte0 as usize]);
                y = y2.wrapping_add(self.delta_y_lut[byte1 as usize]);
                u = u.wrapping_add(self.delta_uv_lut[byte0 as usize]);
                v = v.wrapping_add(self.delta_uv_lut[byte1 as usize]);

                let r0 = self.dyuv_limit_lut[(0x100 + y2 as isize + self.dyuv_v_to_r[v as usize] as isize).clamp(0, 0x2FF) as usize];
                let g0 = self.dyuv_limit_lut[(0x100 + y2 as isize + self.dyuv_u_to_g[u as usize] as isize + self.dyuv_v_to_g[v as usize] as isize).clamp(0, 0x2FF) as usize];
                let b0 = self.dyuv_limit_lut[(0x100 + y2 as isize + self.dyuv_u_to_b[u as usize] as isize).clamp(0, 0x2FF) as usize];
                let color0 = 0xFF00_0000 | (r0 << 16) | (g0 << 8) | b0;

                let byte2 = if PATH == 0 { self.plane_a[(vsr & 0x0007_FFFF) as usize] } else { self.plane_b[(vsr & 0x0007_FFFF) as usize] };
                let byte3 = if PATH == 0 { self.plane_a[((vsr + 1) & 0x0007_FFFF) as usize] } else { self.plane_b[((vsr + 1) & 0x0007_FFFF) as usize] };
                let u8 = u.wrapping_add(self.delta_uv_lut[byte2 as usize]);
                let v8 = v.wrapping_add(self.delta_uv_lut[byte3 as usize]);
                let u6 = (u >> 1) + (u8 >> 1) + (u & u8 & 1);
                let v6 = (v >> 1) + (v8 >> 1) + (v & v8 & 1);

                let r1 = self.dyuv_limit_lut[(0x100 + y as isize + self.dyuv_v_to_r[v6 as usize] as isize).clamp(0, 0x2FF) as usize];
                let g1 = self.dyuv_limit_lut[(0x100 + y as isize + self.dyuv_u_to_g[u6 as usize] as isize + self.dyuv_v_to_g[v6 as usize] as isize).clamp(0, 0x2FF) as usize];
                let b1 = self.dyuv_limit_lut[(0x100 + y as isize + self.dyuv_u_to_b[u6 as usize] as isize).clamp(0, 0x2FF) as usize];
                let color1 = 0xFF00_0000 | (r1 << 16) | (g1 << 8) | b1;

                pixels[x] = color0;
                pixels[x + 1] = color0;
                pixels[x + 2] = color1;
                pixels[x + 3] = color1;
                x += 4;
            }

            if use_matte_flag && !tp_always {
                for i in 0..width {
                    transparent[i] = self.matte_flag[matte_flag_index][i] == tp_check_parity;
                }
            } else {
                transparent.fill(tp_always);
            }
        } else {
            let fixed_tp = !use_color_key && !use_matte_flag && !(use_rgb_tp_bit && icm == ICM_RGB555 && PATH == 1);
            let clut_select = (self.image_coding_method & (1 << 22)) != 0;

            let mut x = 0;
            while x < width {
                let mut raw_rgb_tp = false;
                let byte = if PATH == 0 { self.plane_a[(vsr & 0x0007_FFFF) as usize] } else { self.plane_b[(vsr & 0x0007_FFFF) as usize] };
                vsr += 1;

                let (color0, color1) = if icm == ICM_RGB555 && PATH == 1 {
                    let byte1 = self.plane_a[(vsr2 & 0x0007_FFFF) as usize];
                    vsr2 += 1;
                    let blue = ((byte & 0x1F) as u32) << 3;
                    let green = (((byte & 0xE0) as u32) >> 2) + (((byte1 & 0x03) as u32) << 6);
                    let red = (((byte1 & 0x7C) as u32) >> 2) << 3;
                    raw_rgb_tp = use_rgb_tp_bit && (((byte1 >> 7) & 1) != 0);
                    let c = 0xFF00_0000 | (red << 16) | (green << 8) | blue;
                    (c, c)
                } else if icm == ICM_CLUT4 {
                    let mask = if decoding_mode == DDR_FT_RLE { 0x07 } else { 0x0F };
                    let c0 = self.clut[Self::byte_to_clut::<PATH>(icm, mask & (byte >> 4), clut_select) as usize];
                    let c1 = self.clut[Self::byte_to_clut::<PATH>(icm, mask & byte, clut_select) as usize];
                    (0xFF00_0000 | c0, 0xFF00_0000 | c1)
                } else {
                    let c = self.clut[Self::byte_to_clut::<PATH>(icm, byte, clut_select) as usize];
                    (0xFF00_0000 | c, 0xFF00_0000 | c)
                };

                let mut length_m = if mosaic_enable { mosaic_factor * 2 } else { 2 };
                if decoding_mode == DDR_FT_RLE {
                    let length = if (byte & 0x80) != 0 {
                        let l = if PATH == 0 { self.plane_a[(vsr & 0x0007_FFFF) as usize] } else { self.plane_b[(vsr & 0x0007_FFFF) as usize] };
                        vsr += 1;
                        l as usize
                    } else {
                        1
                    };
                    length_m = if length != 0 { length * 2 } else { width };
                }

                let raw_ck0 = (mask_bits & color0) == tp_color_match;
                let raw_ck1 = (mask_bits & color1) == tp_color_match;
                let end = (x + length_m).min(width);

                let mut rl_index = x;
                while rl_index < end {
                    pixels[rl_index] = color0;
                    if rl_index + 1 < width {
                        pixels[rl_index + 1] = color1;
                    }
                    if !fixed_tp {
                        let cond0 = raw_rgb_tp || (use_color_key && raw_ck0) || (use_matte_flag && self.matte_flag[matte_flag_index][rl_index]);
                        transparent[rl_index] = cond0 == tp_check_parity;
                        if rl_index + 1 < width {
                            let cond1 = raw_rgb_tp || (use_color_key && raw_ck1) || (use_matte_flag && self.matte_flag[matte_flag_index][rl_index + 1]);
                            transparent[rl_index + 1] = cond1 == tp_check_parity;
                        }
                    }
                    rl_index += 2;
                }
                x = end;
            }

            if fixed_tp {
                transparent.fill(tp_always);
            }
        }

        self.set_vsr::<PATH>(vsr);
        if PATH == 1 {
            self.set_vsr::<0>(vsr2);
        } else {
            self.set_vsr::<1>(vsr2);
        }
    }

    fn get_backdrop_plane(&self) -> u32 {
        if (self.image_coding_method & ICM_EV) != 0 {
            fixed_color_out(0xFF00_0000) // External Video Background
        } else {
            fixed_color_out(S_4BPP_DISPLAY_COLOR[(self.backdrop_color & 0x0F) as usize])
        }
    }

    fn mix_lines(
        &self,
        plane_a: &[u32; 768],
        transparent_a: &[bool; 768],
        plane_b: &[u32; 768],
        transparent_b: &[bool; 768],
        out: &mut [u32; 768],
    ) {
        let mosaic_a = (self.mosaic_hold[0] & 0x800000) != 0;
        let mosaic_b = (self.mosaic_hold[1] & 0x800000) != 0;
        let mut mosaic_count_a = ((self.mosaic_hold[0] & 0x0000FF) as usize) << 1;
        let mut mosaic_count_b = ((self.mosaic_hold[1] & 0x0000FF) as usize) << 1;
        if mosaic_count_a == 0 { mosaic_count_a = 1; }
        if mosaic_count_b == 0 { mosaic_count_b = 1; }

        let icm_a = self.get_icm::<0>();
        let icm_b = self.get_icm::<1>();
        if icm_a == ICM_CLUT4 {
            mosaic_count_a >>= 1;
            if mosaic_count_a == 0 { mosaic_count_a = 1; }
        }
        if icm_b == ICM_CLUT4 {
            mosaic_count_b >>= 1;
            if mosaic_count_b == 0 { mosaic_count_b = 1; }
        }

        let order_ab = (self.plane_order & 1) == 0;
        let width = self.get_screen_width();
        let border_width = self.get_border_width();
        let studio = studio_levels();

        if border_width > 0 {
            out[..border_width].fill(0xFF00_0000);
            out[border_width + width..].fill(0xFF00_0000);
        }

        for x in 0..width {
            let dest_x = border_width + x;
            if transparent_a[x] && transparent_b[x] {
                out[dest_x] = if (self.image_coding_method & ICM_EV) != 0 {
                    self.external_video_line[dest_x]
                } else {
                    self.get_backdrop_plane()
                };
                continue;
            }

            let mut pa = if mosaic_a { plane_a[x - (x % mosaic_count_a)] } else { plane_a[x] };
            let mut pb = if mosaic_b { plane_b[x - (x % mosaic_count_b)] } else { plane_b[x] };

            if transparent_a[x] {
                pa = self.s_4bpp_color[0];
            } else if order_ab && (self.transparency_control & TCR_DISABLE_MX != 0) {
                pb = self.s_4bpp_color[0];
            }

            if transparent_b[x] {
                pb = self.s_4bpp_color[0];
            } else if !order_ab && (self.transparency_control & TCR_DISABLE_MX != 0) {
                pa = self.s_4bpp_color[0];
            }

            let plane_a_r = ((pa >> 16) & 0xFF) as i32;
            let plane_a_g = ((pa >> 8) & 0xFF) as i32;
            let plane_a_b = (pa & 0xFF) as i32;

            let plane_b_r = ((pb >> 16) & 0xFF) as i32;
            let plane_b_g = ((pb >> 8) & 0xFF) as i32;
            let plane_b_b = (pb & 0xFF) as i32;

            let wa = self.weight_factor[0][x] as i32;
            let wb = self.weight_factor[1][x] as i32;

            let weighted_a_r = (((plane_a_r - 16).clamp(0, 255) * wa) >> 6).clamp(0, 255);
            let weighted_a_g = (((plane_a_g - 16).clamp(0, 255) * wa) >> 6).clamp(0, 255);
            let weighted_a_b = (((plane_a_b - 16).clamp(0, 255) * wa) >> 6).clamp(0, 255);

            let weighted_b_r = (((plane_b_r - 16).clamp(0, 255) * wb) >> 6).clamp(0, 255);
            let weighted_b_g = (((plane_b_g - 16).clamp(0, 255) * wb) >> 6).clamp(0, 255);
            let weighted_b_b = (((plane_b_b - 16).clamp(0, 255) * wb) >> 6).clamp(0, 255);

            let mixed_r = (weighted_a_r + weighted_b_r + 16).clamp(0, 255);
            let mixed_g = (weighted_a_g + weighted_b_g + 16).clamp(0, 255);
            let mixed_b = (weighted_a_b + weighted_b_b + 16).clamp(0, 255);

            let out_r = level_out(mixed_r, studio);
            let out_g = level_out(mixed_g, studio);
            let out_b = level_out(mixed_b, studio);

            out[dest_x] = 0xFF00_0000 | (out_r << 16) | (out_g << 8) | out_b;
        }
    }

    pub fn hardware_cursor_visible(&self) -> bool {
        (self.cursor_control & CURCNT_EN) != 0 && self.cursor_pattern.iter().any(|&row| row != 0)
    }

    fn draw_cursor(&self, scanline: usize, out: &mut [u32; 768]) {
        if (self.cursor_control & CURCNT_EN) == 0 {
            return; // Cursor is Disabled
        }

        let mut color_index = (self.cursor_control & CURCNT_COLOR) as usize;
        if self.blink_active {
            let invert = (self.cursor_control & CURCNT_BLKC) != 0;
            if !invert {
                return; // Normal blink: invisible
            } else {
                color_index ^= 7; // Inverted color blink
            }
        }

        let cursor_x = (self.cursor_position & 0x3FF) as usize;
        let cursor_y = (((self.cursor_position >> 12) & 0x3FF) as i32) + (self.video_mode.vblank_lines() as i32);
        let y = (scanline as i32) - cursor_y;

        if y >= 0 && y < 16 {
            let color = fixed_color_out(S_4BPP_DISPLAY_COLOR[color_index]);
            let resolution = if (self.cursor_control & CURCNT_CUW) != 0 { 1 } else { 2 };
            let pattern = self.cursor_pattern[y as usize];
            for x in 0..16 {
                if (pattern & (1 << (15 - x))) != 0 {
                    for j in 0..resolution {
                        let idx = cursor_x + x * resolution + j;
                        if idx < 768 {
                            out[idx] = color;
                        }
                    }
                }
            }
        }
    }

    pub fn render_scanline(&mut self, active_line: usize) {
        self.csrr[0] |= CSR1R_DA;
        self.evaluate_line_mattes();

        let mut mixed_line = [0u32; 768];
        let mut plane_a = [0u32; 768];
        let mut plane_b = [0u32; 768];
        let mut transparent_a = [false; 768];
        let mut transparent_b = [false; 768];

        self.process_vsr::<0>(&mut plane_a, &mut transparent_a);
        self.process_vsr::<1>(&mut plane_b, &mut transparent_b);
        self.mix_lines(&plane_a, &transparent_a, &plane_b, &transparent_b, &mut mixed_line);
        self.draw_cursor(self.scanline, &mut mixed_line);

        // Execute DCA during horizontal blanking at the end of the active scanline so register
        // and VSR updates apply starting on the next scanline.
        self.process_dca();

        let active_lines = self.effective_active_scanlines();
        let is_interlaced = (self.dcr[0] & DCR_SM_BIT) != 0;
        let is_odd_field = (self.csrr[0] & CSR1R_PA) == 0;

        let y_offset = (SCREEN_HEIGHT.saturating_sub(active_lines * 2)) / 2;

        if is_interlaced {
            let (curr_row, prev_row) = if is_odd_field {
                (y_offset + active_line * 2 + 1, y_offset + active_line * 2)
            } else {
                (y_offset + active_line * 2, y_offset + active_line * 2 + 1)
            };

            if curr_row < SCREEN_HEIGHT {
                let curr_offset = curr_row * SCREEN_WIDTH;
                self.framebuffer[curr_offset..curr_offset + SCREEN_WIDTH].copy_from_slice(&mixed_line);
            }
            if prev_row < SCREEN_HEIGHT && active_line < 312 {
                let prev_offset = prev_row * SCREEN_WIDTH;
                self.framebuffer[prev_offset..prev_offset + SCREEN_WIDTH].copy_from_slice(&self.interlace_field[active_line]);
            }
            if active_line < 312 {
                self.interlace_field[active_line].copy_from_slice(&mixed_line);
            }
        } else {
            // Progressive / single field output: duplicate lines
            let row0 = y_offset + active_line * 2;
            let row1 = y_offset + active_line * 2 + 1;
            if row1 < SCREEN_HEIGHT {
                let offset0 = row0 * SCREEN_WIDTH;
                self.framebuffer[offset0..offset0 + SCREEN_WIDTH].copy_from_slice(&mixed_line);
                let offset1 = row1 * SCREEN_WIDTH;
                self.framebuffer[offset1..offset1 + SCREEN_WIDTH].copy_from_slice(&mixed_line);
            }
        }

        if active_line == 0 && y_offset > 0 {
            // Blank top letterbox bar
            self.framebuffer[..y_offset * SCREEN_WIDTH].fill(0xFF00_0000);
        }
        if active_line == active_lines - 1 && (y_offset + active_lines * 2) < SCREEN_HEIGHT {
            // Blank bottom letterbox bar
            let start = (y_offset + active_lines * 2) * SCREEN_WIDTH;
            self.framebuffer[start..].fill(0xFF00_0000);
        }
    }

    /// Returns `(is_interlaced, rendered_odd_field, y_offset)` describing the frame just
    /// completed in `self.framebuffer`, for presentation-only de-interlacing.
    pub fn display_interlace_info(&self) -> (bool, bool, usize) {
        let interlaced = (self.dcr[0] & DCR_SM_BIT) != 0;
        // CSR1R_PA toggles at the end of the final scanline, so the just-rendered field
        // had the opposite parity.
        let rendered_odd_field = (self.csrr[0] & CSR1R_PA) != 0;
        let y_offset = (SCREEN_HEIGHT.saturating_sub(self.effective_active_scanlines() * 2)) / 2;
        (interlaced, rendered_odd_field, y_offset)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrosshairMode {
    #[default]
    Off,
    Auto,
    Always,
}

impl CrosshairMode {
    pub fn as_ini_str(self) -> &'static str {
        match self {
            CrosshairMode::Off => "Off",
            CrosshairMode::Auto => "Auto",
            CrosshairMode::Always => "Always",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "0" | "false" | "disabled" => Some(CrosshairMode::Off),
            "auto" => Some(CrosshairMode::Auto),
            "always" | "on" | "1" | "true" => Some(CrosshairMode::Always),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CrosshairMode::Off => "Off",
            CrosshairMode::Auto => "Auto (When Game Hides Cursor)",
            CrosshairMode::Always => "Always Visible",
        }
    }

    pub fn next(self) -> Self {
        match self {
            CrosshairMode::Off => CrosshairMode::Auto,
            CrosshairMode::Auto => CrosshairMode::Always,
            CrosshairMode::Always => CrosshairMode::Off,
        }
    }

    pub fn should_draw(self, hw_cursor_visible: bool) -> bool {
        match self {
            CrosshairMode::Off => false,
            CrosshairMode::Auto => !hw_cursor_visible,
            CrosshairMode::Always => true,
        }
    }
}

/// Draws a non-destructive lightgun reticle at `(x, y)` onto an XRGB8888 presentation buffer.
pub fn draw_crosshair(fb: &mut [u32], width: usize, height: usize, x: i32, y: i32) {
    let mut plot = |px: i32, py: i32, color: u32| {
        if px >= 0 && py >= 0 {
            let (ux, uy) = (px as usize, py as usize);
            if ux < width && uy < height {
                let idx = uy * width + ux;
                if idx < fb.len() {
                    fb[idx] = color;
                }
            }
        }
    };

    const OUTLINE: u32 = 0xFF10_1010;
    const ARM: u32 = 0xFFFF_FFFF;
    const DOT: u32 = 0xFF00_E5FF;

    for d in 3i32..=9i32 {
        for &(px, py) in &[(x - d, y), (x + d, y), (x, y - d), (x, y + d)] {
            plot(px - 1, py, OUTLINE);
            plot(px + 1, py, OUTLINE);
            plot(px, py - 1, OUTLINE);
            plot(px, py + 1, OUTLINE);
        }
    }
    for (ox, oy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        plot(x + ox, y + oy, OUTLINE);
    }
    for d in 3i32..=9i32 {
        plot(x - d, y, ARM);
        plot(x + d, y, ARM);
        plot(x, y - d, ARM);
        plot(x, y + d, ARM);
    }
    plot(x, y, DOT);
}

opencdi::impl_state_enum!(VideoMode { Pal = 0, Ntsc = 1 });

opencdi::impl_state!(Mcd212 {
    plane_a, plane_b,
    csrr, csrw, dcr, vsr, ddr, dcp, dca,
    clut, clut_bank, image_coding_method, transparency_control, plane_order, transparent_color, mask_color, dyuv_abs_start,
    cursor_position, cursor_control, cursor_pattern, matte_control, backdrop_color, mosaic_hold, weight_latch, weight_factor, matte_flag,
    blink_time, blink_active,
    framebuffer, interlace_field,
    scanline, scanline_cycles, vblank_irq, video_mode, external_video_line, external_video_present,
} skip {
    s_4bpp_color, delta_y_lut, delta_uv_lut, dyuv_limit_lut, dyuv_u_to_b, dyuv_u_to_g, dyuv_v_to_g, dyuv_v_to_r, trace_dca, dc_log,
});

#[cfg(test)]
mod tests {
    use super::*;

    // Channel 1 ICA at 0x400: INTERRUPT, then reload DCP and stop.
    fn mcd_with_int() -> Mcd212 {
        let mut m = Mcd212::new();
        m.dcr[0] = DCR_ICA_BIT;
        m.plane_a[0x400..0x408].copy_from_slice(&[0x60, 0, 0, 0, 0x30, 0, 0, 0]);
        m
    }

    #[test]
    fn interrupt_disable_bit_gates_request_not_status() {
        let mut m = mcd_with_int();
        m.write_reg(0x10, CSRW_DI | 0x0003);
        m.tick_scanline(0);
        assert_eq!(m.csrr[1] & CSR2R_IT1, CSR2R_IT1);
        assert!(!m.vblank_irq && !m.int_line());
        assert_eq!(m.read_reg(0x00) & 0x07, CSR2R_IT1 as u16);
        assert_eq!(m.csrr[1] & CSR2R_IT1, 0);

        let mut m = mcd_with_int();
        m.write_reg(0x10, 0x0003);
        m.write_reg(0x00, CSRW_DI);
        m.tick_scanline(0);
        assert!(m.vblank_irq && m.int_line());
        m.read_reg(0x00);
        assert!(!m.int_line());
    }

    #[test]
    fn non_interlaced_process_ica_ignores_field_parity() {
        let mut m = Mcd212::new();
        m.dcr[0] = DCR_ICA_BIT; // SM = 0 (non-interlaced)
        m.dcr[1] = DCR_ICA_BIT;
        // Field 1 (0x400) branches to 0x500 (sets DCP=0x001234 / 0x00ABCD);
        // Field 2 (0x404) branches to 0x600 (sets DCP=0x005678 / 0x00EF01).
        m.plane_a[0x400..0x408].copy_from_slice(&[0x40, 0x00, 0x05, 0x00, 0x40, 0x00, 0x06, 0x00]);
        m.plane_b[0x400..0x408].copy_from_slice(&[0x40, 0x00, 0x05, 0x00, 0x40, 0x00, 0x06, 0x00]);
        m.plane_a[0x500..0x504].copy_from_slice(&[0x30, 0x00, 0x12, 0x34]);
        m.plane_a[0x600..0x604].copy_from_slice(&[0x30, 0x00, 0x56, 0x78]);
        m.plane_b[0x500..0x504].copy_from_slice(&[0x30, 0x00, 0xAB, 0xC0]);
        m.plane_b[0x600..0x604].copy_from_slice(&[0x30, 0x00, 0xEF, 0x00]);

        m.csrr[0] = CSR1R_PA;
        m.tick_scanline(0);
        assert_eq!(m.get_dcp::<0>(), 0x001234);
        assert_eq!(m.get_dcp::<1>(), 0x00ABC0);

        // Interlaced (SM = 1) selects 0x202 (0x404) when CSR1R_PA is set.
        m.dcr[0] |= DCR_SM_BIT;
        m.csrr[0] = CSR1R_PA;
        m.tick_scanline(0);
        assert_eq!(m.get_dcp::<0>(), 0x005678);
        assert_eq!(m.get_dcp::<1>(), 0x00EF00);
    }

    #[test]
    fn dca_executes_after_vsr_scanout_for_next_scanline() {
        let mut m = Mcd212::new();
        m.video_mode = VideoMode::Pal;
        m.dcr[0] = DCR_DE_BIT | DCR_ICA_BIT | DCR_DCA_BIT; // CF=0 -> 720px active + 24px borders

        let ica = [
            0xC000_0001u32, // Plane A CLUT8
            0xC100_0008,    // Plane A always opaque
            0xC300_0000,    // CLUT bank 0
            0x8100_FC00,    // CLUT[1] = green
            0x82FC_0000,    // CLUT[2] = red
            0xDB00_003F,    // Weight A = max
            0x2000_1000,    // DCP = 0x1000
            0x5000_2000,    // VSR = 0x2000 and stop
        ];
        for (i, &w) in ica.iter().enumerate() {
            let off = 0x400 + i * 4;
            m.plane_a[off..off + 4].copy_from_slice(&w.to_be_bytes());
        }
        // Line 32 HBLANK DCA at 0x1000 reloads VSR to 0x3000 for line 33.
        m.plane_a[0x1000..0x1004].copy_from_slice(&0x4000_3000u32.to_be_bytes());
        m.plane_a[0x1004..0x1008].copy_from_slice(&0x0000_0000u32.to_be_bytes());
        m.plane_a[0x2000..0x2168].fill(1);
        m.plane_a[0x3000..0x3168].fill(2);

        m.tick_scanline(0);
        assert_eq!(m.get_vsr::<0>(), 0x2000);

        m.tick_scanline(32);
        m.render_line(32);
        let p0 = m.framebuffer[24];
        assert!(((p0 >> 8) & 0xFF) > 200 && ((p0 >> 16) & 0xFF) < 30, "line 32 must scan out 0x2000 (green), got {p0:08X}");
        assert_eq!(m.framebuffer[0] & 0x00FF_FFFF, 0, "left 24px border must be black");
        assert_eq!(m.get_vsr::<0>(), 0x3000, "DCA must reload VSR during line 32 HBLANK");

        m.tick_scanline(33);
        m.render_line(33);
        let p1 = m.framebuffer[2 * SCREEN_WIDTH + 24];
        assert!(((p1 >> 16) & 0xFF) > 200 && ((p1 >> 8) & 0xFF) < 30, "line 33 must scan out 0x3000 (red), got {p1:08X}");
    }
}

#[cfg(test)]
mod matte_and_dc_tests {
    use super::*;

    fn write_be32(mem: &mut [u8], addr: usize, val: u32) {
        mem[addr..addr + 4].copy_from_slice(&val.to_be_bytes());
    }

    #[test]
    fn interleaved_ica_preserves_later_path1_writes_and_path0_same_step_priority() {
        let mut m = Mcd212::new();
        m.dcr[0] = DCR_DE_BIT | DCR_ICA_BIT;
        m.dcr[1] = DCR_DE_BIT | DCR_ICA_BIT;

        // Path 0 sets startup matte control early, writes TCR on step 2, then stops.
        let p0 = [0xD080_0000, 0xD180_0000, 0xC180_0808, 0x0000_0000];
        for (i, &w) in p0.iter().enumerate() {
            write_be32(&mut *m.plane_a, 0x0400 + i * 4, w);
        }

        // Path 1 runs NOPs past Path 0's matte resets, collides on step 2 (Path 0 must win),
        // then writes shared matte control on steps 3..4 (Path 1 must persist).
        let p1 = [
            0x1000_0000,
            0x1000_0000,
            0xC180_0303,
            0xD090_0000,
            0xD180_02FF,
            0x0000_0000,
        ];
        for (i, &w) in p1.iter().enumerate() {
            write_be32(&mut *m.plane_b, 0x0400 + i * 4, w);
        }

        assert!(m.tick_scanline(0));
        assert_eq!(m.transparency_control & 0x00FF_FFFF, 0x800808);
        assert_eq!(m.matte_control[0], 0x90_0000);
        assert_eq!(m.matte_control[1], 0x80_02FF);
    }

    #[test]
    fn interleaved_dca_preserves_later_path1_writes_and_path0_same_step_priority() {
        let mut m = Mcd212::new();
        m.dcr[0] = DCR_DE_BIT | DCR_DCA_BIT | DCR_CF_BIT;
        m.dcr[1] = DCR_DE_BIT | DCR_DCA_BIT | DCR_CF_BIT;
        m.dca[0] = 0x1000;
        m.dca[1] = 0x1000;

        let dca0 = [0xD280_0000, 0xC180_0803, 0x0000_0000];
        for (i, &w) in dca0.iter().enumerate() {
            write_be32(&mut *m.plane_a, 0x1000 + i * 4, w);
        }
        let dca1 = [0x1000_0000, 0xC180_0303, 0xD290_0100, 0x0000_0000];
        for (i, &w) in dca1.iter().enumerate() {
            write_be32(&mut *m.plane_b, 0x1000 + i * 4, w);
        }

        // Active scanline HBLANK runs DCA right after VSR scanout in render_line.
        let active_start = m.video_mode.vblank_lines();
        m.tick_scanline(active_start);
        m.render_line(active_start);
        assert_eq!(m.transparency_control & 0x00FF_FFFF, 0x800803);
        assert_eq!(m.matte_control[2], 0x90_0100);
    }

    #[test]
    fn matte_transparency_keys_outside_dvc_window_with_icm_ev() {
        let mut m = Mcd212::new();
        m.dcr[0] = DCR_DE_BIT | DCR_CF_BIT;
        m.dcr[1] = DCR_DE_BIT | DCR_CF_BIT;
        m.csrr[0] = CSR1R_DA;
        m.set_register::<0>(0xC0, ICM_EV | ((ICM_CLUT7 as u32) << 8) | (ICM_CLUT7 as u32));
        m.set_register::<0>(0xC2, 0); // Plane A in front, Plane B behind

        m.set_vsr::<0>(0x1000);
        m.set_vsr::<1>(0x1000);
        m.plane_a[0x1000..0x1000 + 768].fill(1);
        m.plane_b[0x1000..0x1000 + 768].fill(1);
        m.clut[1] = 0x00FF_1010; // Plane A: red
        m.clut[129] = 0x0010_FF10; // Plane B (CLUT bank 2): green
        m.set_register::<0>(0xDB, 63);
        m.set_register::<1>(0xDC, 63);

        // DVC active window only covers x=32..735; outer strips have external_video_present = false.
        m.external_video_present.fill(false);
        m.external_video_present[32..736].fill(true);
        m.external_video_line.fill(0xFF00_0000);
        m.external_video_line[32..736].fill(0xFF12_3456);

        let y_off = (SCREEN_HEIGHT.saturating_sub(m.effective_active_scanlines() * 2)) / 2;
        let row_start = y_off * SCREEN_WIDTH;

        // Case 1: Horizontal wipe (Plane A = TCR_MF0, Plane B = opaque 0x08).
        // Matte switches MF0 = 1 at x = 0x2C0 (704); x=736..767 must uncover Plane B, not stay Plane A.
        m.set_register::<0>(0xC1, 0x0080_0800 | (TCR_MF0 as u32));
        m.set_register::<0>(0xD0, 0x9002C0);
        m.render_scanline(0);
        let row = &m.framebuffer[row_start..row_start + 768];
        let red_left = (row[700] >> 16) & 0xFF;
        let green_mid = (row[720] >> 8) & 0xFF;
        let green_edge = (row[750] >> 8) & 0xFF;
        assert!(red_left > 150 && ((row[700] >> 8) & 0xFF) < 40, "x=700 should stay Plane A red: {:#010X}", row[700]);
        assert_eq!(row[750], row[720], "x=750 outside DVC window must match Plane B at x=720");
        assert!(green_mid > 150 && green_edge > 150, "x=750 should uncover Plane B green: {:#010X}", row[750]);

        // Case 2: Full-screen matte for FMV (both planes = TCR_MF0, MF0 = 1 across 0..766).
        // Outer DVC border x=0..31 and x=736..766 must show DVC border (black), not Plane A/B graphics.
        m.set_vsr::<0>(0x1000);
        m.set_vsr::<1>(0x1000);
        m.set_register::<0>(0xC1, 0x0080_0000 | ((TCR_MF0 as u32) << 8) | (TCR_MF0 as u32));
        m.set_register::<0>(0xD0, 0x900000);
        m.set_register::<0>(0xD1, 0x8002FF);
        m.render_scanline(0);
        let row = &m.framebuffer[row_start..row_start + 768];
        assert_eq!(row[10], 0xFF00_0000);
        assert_eq!(row[384], 0xFF12_3456);
        assert_eq!(row[750], 0xFF00_0000);
    }

    #[test]
    fn icm_reserved_bits_4_and_12_do_not_enable_external_video() {
        let mut m = Mcd212::new();
        m.dcr[0] = DCR_DE_BIT | DCR_CF_BIT;
        m.dcr[1] = DCR_DE_BIT | DCR_CF_BIT;
        m.csrr[0] = CSR1R_DA;
        m.set_register::<0>(0xC1, 0x0080_0000); // Both planes always transparent (TCR_ALWAYS)
        m.set_register::<0>(0xD8, 0x09); // Bright blue backdrop color
        m.set_vsr::<0>(0x1000);
        m.set_vsr::<1>(0x1000);
        m.external_video_line.fill(0xFF12_3456);

        let y_off = (SCREEN_HEIGHT.saturating_sub(m.effective_active_scanlines() * 2)) / 2;
        let row_start = y_off * SCREEN_WIDTH;
        let expected_backdrop = fixed_color_out(S_4BPP_DISPLAY_COLOR[0x09]);
        assert_ne!(expected_backdrop, 0xFF12_3456);

        // MCD212 0xC0 value 0x001313 (e.g. Lucky Luke / Vision Factory) sets reserved bits 4 & 12
        // with EV bit 18 clear: external video must stay disabled and backdrop must show cleanly.
        m.set_register::<0>(0xC0, 0x0000_1313);
        m.render_scanline(0);
        let row = &m.framebuffer[row_start..row_start + 768];
        assert_eq!(row[10], expected_backdrop);
        assert_eq!(row[384], expected_backdrop);
        assert_eq!(row[750], expected_backdrop);

        // Setting bit 18 (ICM_EV) must route external_video_line through transparent planes.
        m.set_vsr::<0>(0x1000);
        m.set_vsr::<1>(0x1000);
        m.set_register::<0>(0xC0, ICM_EV | 0x0000_1313);
        m.render_scanline(0);
        let row = &m.framebuffer[row_start..row_start + 768];
        assert_eq!(row[10], 0xFF12_3456);
        assert_eq!(row[384], 0xFF12_3456);
        assert_eq!(row[750], 0xFF12_3456);
    }

    #[test]
    fn matte_weight_factor_persists_across_scanlines_while_region_flags_reset() {
        let mut m = Mcd212::new();
        m.dcr[0] = DCR_DE_BIT | DCR_CF_BIT;
        m.dcr[1] = DCR_DE_BIT | DCR_CF_BIT;
        m.set_register::<0>(0xDB, 63);
        m.set_register::<1>(0xDC, 63);
        // Steel Machine style: x=0 resets MF0=0, x=64 sets wf_B=42, x=100 sets MF0=1,
        // x=704 sets wf_B=0, x=706 sets wf_A=0 (consecutive matte slots 0..4 so no STOP op intervenes).
        m.set_register::<0>(0xD0, 0x08 << 20);
        m.set_register::<0>(0xD1, (0x06 << 20) | (42 << 10) | 64);
        m.set_register::<0>(0xD2, (0x09 << 20) | 100);
        m.set_register::<0>(0xD3, (0x06 << 20) | (0 << 10) | 704);
        m.set_register::<0>(0xD4, (0x04 << 20) | (0 << 10) | 706);

        // Line 0 starts with ICA weights [63, 63], ends with [0, 0] at x=704..767.
        m.evaluate_line_mattes();
        assert_eq!(m.weight_factor[0][0], 63);
        assert_eq!(m.weight_factor[1][0], 63);
        assert_eq!(m.weight_factor[1][64], 42);
        assert_eq!(m.weight_factor[1][704], 0);
        assert_eq!(m.weight_factor[0][706], 0);
        assert!(m.matte_flag[0][700]);

        // Line 1 inherits wf=[0, 0] from end of line 0 across x=0..63, while MF0 resets to false.
        m.evaluate_line_mattes();
        assert_eq!(m.weight_factor[0][0..64], [0; 64]);
        assert_eq!(m.weight_factor[1][0..64], [0; 64]);
        assert!(!m.matte_flag[0][99]);
        assert!(m.matte_flag[0][100]);
        assert_eq!(m.weight_factor[1][64], 42);
        assert_eq!(m.weight_factor[1][703], 42);
        assert_eq!(m.weight_factor[1][704], 0);
        assert_eq!(m.weight_factor[0][706], 0);
    }

    #[test]
    fn inverted_combined_tcr_uses_nor_semantics() {
        let mut m = Mcd212::new();
        m.dcr[0] = DCR_DE_BIT | DCR_CF_BIT;
        m.set_register::<0>(0xC0, ICM_CLUT8 as u32);
        // Plane A uses inverted TCR_MF0_KEY1 (0x08 | 0x05 = 0x0D -> transparent iff !(ck || mf0)).
        m.set_register::<0>(0xC1, 0x08 | (TCR_MF0_KEY1 as u32));
        m.set_register::<0>(0xC4, 0x00FC_0000); // Color key = CLUT[2] (red)
        m.set_register::<0>(0xC7, 0x0000_0000); // Full mask (all bits compared)
        m.clut[1] = 0x0000_FC00; // non-key green
        m.clut[2] = 0x00FC_0000; // color-key red

        // 4 regions:
        // x=0..15:   !ck, !mf0  -> transparent = true
        // x=16..31:   ck, !mf0  -> transparent = false
        // x=32..47:   ck,  mf0  -> transparent = false
        // x=48..63:  !ck,  mf0  -> transparent = false
        m.plane_a[0x1000..0x1008].fill(1);
        m.plane_a[0x1008..0x1018].fill(2);
        m.plane_a[0x1018..0x1020].fill(1);
        m.set_vsr::<0>(0x1000);
        m.matte_flag[0][0..32].fill(false);
        m.matte_flag[0][32..64].fill(true);

        let mut pixels = [0u32; 768];
        let mut tp = [false; 768];
        m.process_vsr::<0>(&mut pixels, &mut tp);

        assert!(tp[..16].iter().all(|&t| t), "!ck && !mf0 must be transparent under inverted TCR_MF0_KEY1");
        assert!(tp[16..32].iter().all(|&t| !t), "ck && !mf0 must be opaque under inverted TCR_MF0_KEY1");
        assert!(tp[32..48].iter().all(|&t| !t), "ck && mf0 must be opaque under inverted TCR_MF0_KEY1");
        assert!(tp[48..64].iter().all(|&t| !t), "!ck && mf0 must be opaque under inverted TCR_MF0_KEY1");
    }
}

#[cfg(test)]
mod level_tests {
    use super::*;

    #[test]
    fn full_stretches_black_and_white() {
        assert_eq!(level_out(16, false), 0);
        assert_eq!(level_out(235, false), 255);
    }

    #[test]
    fn studio_passes_values_through() {
        assert_eq!(level_out(16, true), 16);
        assert_eq!(level_out(235, true), 235);
    }

    #[test]
    fn fixed_colors_map_to_studio_range() {
        assert_eq!(full_to_studio(0), 16);
        assert_eq!(full_to_studio(255), 235);
    }

    #[test]
    fn crosshair_modes_and_bounds_clipping() {
        assert!(!CrosshairMode::Off.should_draw(false));
        assert!(CrosshairMode::Auto.should_draw(false));
        assert!(!CrosshairMode::Auto.should_draw(true));
        assert!(CrosshairMode::Always.should_draw(true));
        assert_eq!(CrosshairMode::Off.next(), CrosshairMode::Auto);
        assert_eq!(CrosshairMode::Auto.next(), CrosshairMode::Always);
        assert_eq!(CrosshairMode::Always.next(), CrosshairMode::Off);
        assert_eq!(CrosshairMode::from_str_opt("auto"), Some(CrosshairMode::Auto));
        assert_eq!(CrosshairMode::from_str_opt("always"), Some(CrosshairMode::Always));
        assert_eq!(CrosshairMode::from_str_opt("off"), Some(CrosshairMode::Off));

        let mut m = Mcd212::new();
        assert!(!m.hardware_cursor_visible());
        m.cursor_control = CURCNT_EN;
        assert!(!m.hardware_cursor_visible(), "empty pattern means invisible cursor");
        m.cursor_pattern[0] = 0x8000;
        assert!(m.hardware_cursor_visible());

        let mut fb = vec![0xFF20_2020u32; SCREEN_WIDTH * SCREEN_HEIGHT];
        draw_crosshair(&mut fb, SCREEN_WIDTH, SCREEN_HEIGHT, 0, 0);
        draw_crosshair(&mut fb, SCREEN_WIDTH, SCREEN_HEIGHT, 767, 559);
        draw_crosshair(&mut fb, SCREEN_WIDTH, SCREEN_HEIGHT, 384, 280);
        assert_eq!(fb[0], 0xFF00_E5FF);
        assert_eq!(fb[280 * SCREEN_WIDTH + 384], 0xFF00_E5FF);
        assert_eq!(fb[280 * SCREEN_WIDTH + 387], 0xFFFF_FFFF);
    }
}

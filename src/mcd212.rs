pub const SCREEN_WIDTH: usize = 768;
pub const SCREEN_HEIGHT: usize = 560;
pub const TOTAL_SCANLINES: usize = 312;
pub const CYCLES_PER_SCANLINE: u32 = 993; // 15.5 MHz / 15,625 Hz (PAL default)

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

    #[inline]
    pub fn cycles_per_scanline(self) -> u32 {
        // SCC68070 Master clock: ~15.5 MHz
        // PAL line frequency: 15,625 Hz -> 15,500,000 / 15,625 = 992 cycles (use 993)
        // NTSC line frequency: 15,734 Hz -> 15,500,000 / 15,734 = 985 cycles
        match self {
            VideoMode::Pal => 993,
            VideoMode::Ntsc => 985,
        }
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
    pub display_plane_a: Box<[u8; 512 * 1024]>,
    pub display_plane_b: Box<[u8; 512 * 1024]>,

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
            display_plane_a: vec![0u8; 512 * 1024].into_boxed_slice().try_into().unwrap(),
            display_plane_b: vec![0u8; 512 * 1024].into_boxed_slice().try_into().unwrap(),

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
        self.display_plane_a.fill(0);
        self.display_plane_b.fill(0);
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

        // Stalled waiting for the system slot (1 to 5 clocks at 15.5 MHz)
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
                    if self.image_coding_method != val {
                        println!(">>> [Scanline {}] ICMR changed from 0x{:08X} to 0x{:08X}", self.scanline, self.image_coding_method, val);
                    }
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
                self.update_matte_arrays();
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
                    self.weight_factor[0][0] = val as u8;
                    self.update_matte_arrays();
                }
            }
            0xDC => {
                if PATH == 1 {
                    self.weight_factor[1][0] = val as u8;
                    self.update_matte_arrays();
                }
            }
            _ => {}
        }
    }

    fn update_matte_arrays(&mut self) {
        let width = 768;
        let num_mattes = if (self.image_coding_method & (1 << 19)) != 0 { 2 } else { 1 };

        let mut latched_mf = [false, false];
        let mut latched_wf = [self.weight_factor[0][0], self.weight_factor[1][0]];
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
    }

    #[inline]
    pub fn effective_active_scanlines(&self) -> usize {
        if self.video_mode == VideoMode::Ntsc || (self.dcr[0] & DCR_FD_BIT) != 0 {
            240
        } else {
            280
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

            // Stage completed VRAM frame into display planes (eliminates in-flight blitter race/tearing)
            self.display_plane_a.copy_from_slice(&self.plane_a[..]);
            self.display_plane_b.copy_from_slice(&self.plane_b[..]);

            // Process ICA: Path 1 then Path 0 for correct hardware precedence (Path 0 wins)
            self.process_ica::<1>();
            self.process_ica::<0>();

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
            // Active display lines
            self.csrr[0] |= CSR1R_DA;

            if (self.dcr[1] & DCR_DCA_BIT) != 0 {
                self.process_dca::<1>();
            }
            if (self.dcr[0] & DCR_DCA_BIT) != 0 {
                self.process_dca::<0>();
            }
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

    fn process_ica<const PATH: usize>(&mut self) {
        if (self.dcr[PATH] & DCR_ICA_BIT) == 0 {
            return;
        }

        // LCT depends on current frame parity
        let mut addr = if (self.csrr[0] & CSR1R_PA) != 0 {
            0x202
        } else {
            0x200
        };

        let max_to_process = 32 * 120;
        for _ in 0..max_to_process {
            let byte_addr = (addr as usize) * 2;
            let cmd = if PATH == 0 {
                if byte_addr + 4 > self.plane_a.len() {
                    break;
                }
                u32::from_be_bytes([
                    self.plane_a[byte_addr],
                    self.plane_a[byte_addr + 1],
                    self.plane_a[byte_addr + 2],
                    self.plane_a[byte_addr + 3],
                ])
            } else {
                if byte_addr + 4 > self.plane_b.len() {
                    break;
                }
                u32::from_be_bytes([
                    self.plane_b[byte_addr],
                    self.plane_b[byte_addr + 1],
                    self.plane_b[byte_addr + 2],
                    self.plane_b[byte_addr + 3],
                ])
            };
            addr += 2;

            match (cmd >> 24) as u8 {
                0x00..=0x0F => return, // STOP
                0x10..=0x1F => {}      // NOP
                0x20..=0x2F => {
                    // RELOAD DCP
                    self.set_dcp::<PATH>(cmd & 0x003F_FFFC);
                }
                0x30..=0x3F => {
                    // RELOAD DCP and STOP
                    self.set_dcp::<PATH>(cmd & 0x003F_FFFC);
                    return;
                }
                0x40..=0x4F => {
                    // RELOAD VSR (ICA jump)
                    addr = ((cmd & 0x0007_FFFF) / 2) as u32;
                }
                0x50..=0x5F => {
                    // RELOAD VSR and STOP
                    self.set_vsr::<PATH>(cmd & 0x003F_FFFF);
                    return;
                }
                0x60..=0x6F => {
                    // INTERRUPT
                    self.csrr[1] |= 1 << (2 - PATH);
                    self.vblank_irq = true;
                }
                0x78..=0x7F => {
                    // RELOAD DISPLAY PARAMETERS
                    self.set_display_parameters::<PATH>((cmd & 0x1F) as u8);
                }
                reg => {
                    self.set_register::<PATH>(reg, cmd & 0x00FF_FFFF);
                }
            }
        }
    }

    fn process_dca<const PATH: usize>(&mut self) {
        let trace_this = false;
        let start_addr = ((self.dca[PATH] & 0x0007_FFFC) / 2) as usize;
        let mut addr = start_addr;
        let mut count = 0;
        let max = 64;
        let mut processing = true;

        while processing && count < max {
            let byte_addr = addr * 2;
            let cmd = if PATH == 0 {
                if byte_addr + 4 > self.plane_a.len() {
                    break;
                }
                u32::from_be_bytes([
                    self.plane_a[byte_addr],
                    self.plane_a[byte_addr + 1],
                    self.plane_a[byte_addr + 2],
                    self.plane_a[byte_addr + 3],
                ])
            } else {
                if byte_addr + 4 > self.plane_b.len() {
                    break;
                }
                u32::from_be_bytes([
                    self.plane_b[byte_addr],
                    self.plane_b[byte_addr + 1],
                    self.plane_b[byte_addr + 2],
                    self.plane_b[byte_addr + 3],
                ])
            };
            addr += 2;
            count += 4;

            if trace_this {
                println!("  [Scanline {} DCA{} cmd] 0x{:08X}", self.scanline, PATH, cmd);
            }
            match (cmd >> 24) as u8 {
                0x00..=0x0F => processing = false, // STOP
                0x10..=0x1F => {}                  // NOP
                0x20..=0x2F => {}                  // RELOAD DCP (NOP in DCA)
                0x30..=0x3F => {
                    // RELOAD DCP and STOP
                    self.set_dcp::<PATH>(cmd & 0x003F_FFFC);
                    self.dca[PATH] = cmd & 0x0007_FFFC;
                    return;
                }
                0x40..=0x4F => {
                    // RELOAD VSR
                    self.set_vsr::<PATH>(cmd & 0x003F_FFFF);
                }
                0x50..=0x5F => {
                    // RELOAD VSR and STOP
                    self.set_vsr::<PATH>(cmd & 0x003F_FFFF);
                    processing = false;
                }
                0x60..=0x6F => {
                    // INTERRUPT
                    self.csrr[1] |= 1 << (2 - PATH);
                    self.vblank_irq = true;
                }
                0x78..=0x7F => {
                    // RELOAD DISPLAY PARAMETERS
                    self.set_display_parameters::<PATH>((cmd & 0x1F) as u8);
                }
                reg => {
                    self.set_register::<PATH>(reg, cmd & 0x00FF_FFFF);
                }
            }
        }

        addr += (max - count) >> 1;
        self.dca[PATH] = (addr * 2) as u32;
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

    fn process_vsr<const PATH: usize>(&mut self, pixels: &mut [u32; 768], transparent: &mut [bool; 768]) {
        let icm = self.get_icm::<PATH>();
        let tp_ctrl = self.get_transparency_control::<PATH>();
        let width = 768;

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
        // With external video on, a matte region keys the plane away only inside the DVC window.
        let ext_keys = use_matte_flag && (self.image_coding_method & ICM_EV) != 0;

        if icm == ICM_DYUV {
            let mut x = 0;
            while x < width {
                let byte0 = if PATH == 0 { self.display_plane_a[(vsr & 0x0007_FFFF) as usize] } else { self.display_plane_b[(vsr & 0x0007_FFFF) as usize] };
                vsr += 1;
                let byte1 = if PATH == 0 { self.display_plane_a[(vsr & 0x0007_FFFF) as usize] } else { self.display_plane_b[(vsr & 0x0007_FFFF) as usize] };
                vsr += 1;

                let y2 = y.wrapping_add(self.delta_y_lut[byte0 as usize]);
                y = y2.wrapping_add(self.delta_y_lut[byte1 as usize]);
                u = u.wrapping_add(self.delta_uv_lut[byte0 as usize]);
                v = v.wrapping_add(self.delta_uv_lut[byte1 as usize]);

                let r0 = self.dyuv_limit_lut[(0x100 + y2 as isize + self.dyuv_v_to_r[v as usize] as isize).clamp(0, 0x2FF) as usize];
                let g0 = self.dyuv_limit_lut[(0x100 + y2 as isize + self.dyuv_u_to_g[u as usize] as isize + self.dyuv_v_to_g[v as usize] as isize).clamp(0, 0x2FF) as usize];
                let b0 = self.dyuv_limit_lut[(0x100 + y2 as isize + self.dyuv_u_to_b[u as usize] as isize).clamp(0, 0x2FF) as usize];
                let color0 = 0xFF00_0000 | (r0 << 16) | (g0 << 8) | b0;

                let byte2 = if PATH == 0 { self.display_plane_a[(vsr & 0x0007_FFFF) as usize] } else { self.display_plane_b[(vsr & 0x0007_FFFF) as usize] };
                let byte3 = if PATH == 0 { self.display_plane_a[((vsr + 1) & 0x0007_FFFF) as usize] } else { self.display_plane_b[((vsr + 1) & 0x0007_FFFF) as usize] };
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
                    transparent[i] = self.matte_flag[matte_flag_index][i] == tp_check_parity && (!ext_keys || self.external_video_present[i]);
                }
            } else {
                transparent.fill(tp_always);
            }
        } else {
            let fixed_tp = !use_color_key && !use_matte_flag && !(use_rgb_tp_bit && icm == ICM_RGB555 && PATH == 1);
            let clut_select = (self.image_coding_method & (1 << 22)) != 0;

            let mut x = 0;
            while x < width {
                let mut rgb_tp_bit = false;
                let byte = if PATH == 0 { self.display_plane_a[(vsr & 0x0007_FFFF) as usize] } else { self.display_plane_b[(vsr & 0x0007_FFFF) as usize] };
                vsr += 1;

                let (color0, color1) = if icm == ICM_RGB555 && PATH == 1 {
                    let byte1 = self.display_plane_a[(vsr2 & 0x0007_FFFF) as usize];
                    vsr2 += 1;
                    let blue = ((byte & 0x1F) as u32) << 3;
                    let green = (((byte & 0xE0) as u32) >> 2) + (((byte1 & 0x03) as u32) << 6);
                    let red = (((byte1 & 0x7C) as u32) >> 2) << 3;
                    rgb_tp_bit = use_rgb_tp_bit && (((byte1 >> 7) & 1) != 0) == tp_check_parity;
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
                        let l = if PATH == 0 { self.display_plane_a[(vsr & 0x0007_FFFF) as usize] } else { self.display_plane_b[(vsr & 0x0007_FFFF) as usize] };
                        vsr += 1;
                        l as usize
                    } else {
                        1
                    };
                    length_m = if length != 0 { length * 2 } else { width };
                }

                let color_match0 = ((mask_bits & color0) == tp_color_match) == tp_check_parity;
                let color_match1 = ((mask_bits & color1) == tp_color_match) == tp_check_parity;
                let end = (x + length_m).min(width);

                let mut rl_index = x;
                while rl_index < end {
                    pixels[rl_index] = color0;
                    if rl_index + 1 < width {
                        pixels[rl_index + 1] = color1;
                    }
                    if !fixed_tp {
                        transparent[rl_index] = rgb_tp_bit || (use_color_key && color_match0) || (use_matte_flag && (self.matte_flag[matte_flag_index][rl_index] == tp_check_parity) && (!ext_keys || self.external_video_present[rl_index]));
                        if rl_index + 1 < width {
                            transparent[rl_index + 1] = rgb_tp_bit || (use_color_key && color_match1) || (use_matte_flag && (self.matte_flag[matte_flag_index][rl_index + 1] == tp_check_parity) && (!ext_keys || self.external_video_present[rl_index + 1]));
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
        if (self.image_coding_method & ((1 << 4) | (1 << 12) | (1 << 18))) != 0 {
            0xFF00_0000 // External Video Background
        } else {
            S_4BPP_DISPLAY_COLOR[(self.backdrop_color & 0x0F) as usize]
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
        let width = 768;

        for x in 0..width {
            if transparent_a[x] && transparent_b[x] {
                out[x] = if (self.image_coding_method & ((1 << 4) | (1 << 12) | (1 << 18))) != 0 {
                    self.external_video_line[x]
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

            let out_r = (((mixed_r - 16) * 298 + 128) >> 8).clamp(0, 255) as u32;
            let out_g = (((mixed_g - 16) * 298 + 128) >> 8).clamp(0, 255) as u32;
            let out_b = (((mixed_b - 16) * 298 + 128) >> 8).clamp(0, 255) as u32;

            out[x] = 0xFF00_0000 | (out_r << 16) | (out_g << 8) | out_b;
        }
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
            let color = S_4BPP_DISPLAY_COLOR[color_index];
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
        let mut plane_a = [0u32; 768];
        let mut plane_b = [0u32; 768];
        let mut transparent_a = [false; 768];
        let mut transparent_b = [false; 768];
        let mut mixed_line = [0u32; 768];

        self.process_vsr::<0>(&mut plane_a, &mut transparent_a);
        self.process_vsr::<1>(&mut plane_b, &mut transparent_b);
        self.mix_lines(&plane_a, &transparent_a, &plane_b, &transparent_b, &mut mixed_line);
        self.draw_cursor(self.scanline, &mut mixed_line);


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
}

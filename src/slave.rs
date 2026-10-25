use log::{info, trace};
use crate::mcd212::VideoMode;

#[derive(Clone, Copy)]
pub struct SlaveChannel {
    pub out_buf: [u8; 8],
    pub out_count: usize,
    pub out_index: usize,
    pub out_cmd: u8,

    pub in_buf: [u8; 18],
    pub in_count: usize,
    pub in_index: usize,

    // Pacchetto CH0 in attesa di promozione (arrivato durante una lettura in corso)
    pub pending_buf: [u8; 8],
    pub pending_count: usize,
    pub pending_cmd: u8,
}

impl Default for SlaveChannel {
    fn default() -> Self {
        Self {
            out_buf: [0; 8],
            out_count: 0,
            out_index: 0,
            out_cmd: 0,
            in_buf: [0; 18],
            in_count: 0,
            in_index: 0,
            pending_buf: [0; 8],
            pending_count: 0,
            pending_cmd: 0,
        }
    }
}

pub struct SlaveHle {
    pub channels: [SlaveChannel; 4],

    // Mouse coordinates (0..767, 0..559)
    pub device_mouse_x: i32,
    pub device_mouse_y: i32,
    pub device_mouse_updated: bool,
    pub mouse_btn: u8,

    pub center_x: i32,
    pub center_y: i32,

    // Previous input values to detect changes
    input_mouse_x: i32,
    input_mouse_y: i32,
    input_mouse_btn: u8,

    pub irq_pending: bool,
    pub has_disc: bool,
    pub disc_state: u8,
    pub disc_timer: u32,
    pub xbus_interrupt_enable: bool,
    pub video_mode: VideoMode,
    pub debug_log: bool,
    pub stream_counter: u32,
    // Set by the channel 2 reset command, cleared by the machine when it resets the player
    pub reset_request: bool,
}

impl SlaveHle {
    pub fn new() -> Self {
        Self {
            channels: [SlaveChannel::default(); 4],

            device_mouse_x: 384,
            device_mouse_y: 280,
            center_x: 384,
            center_y: 280,
            device_mouse_updated: false,
            mouse_btn: 0,

            input_mouse_x: -1,
            input_mouse_y: -1,
            input_mouse_btn: 0,

            irq_pending: false,
            has_disc: false,
            disc_state: 0x00,
            disc_timer: 0,
            xbus_interrupt_enable: false,
            video_mode: VideoMode::Pal,
            debug_log: false,
            stream_counter: 0,
            reset_request: false,
        }
    }

    pub fn reset(&mut self) {
        for ch in &mut self.channels {
            *ch = SlaveChannel::default();
        }
        self.device_mouse_x = 384;
        self.device_mouse_y = 280;
        self.center_x = 384;
        self.center_y = 280;
        self.input_mouse_btn = 0;
        self.input_mouse_x = -1;
        self.input_mouse_y = -1;
        self.irq_pending = false;
        self.reset_request = false;
        self.disc_timer = 0;
        self.disc_state = if self.has_disc { 0x41 } else { 0x00 };
    }

    pub fn set_disc_inserted(&mut self, inserted: bool) {
        self.has_disc = inserted;
        self.disc_timer = 0;
        self.disc_state = if inserted { 0x41 } else { 0x00 };
        let disc_byte = self.disc_state;
        info!("Slave: set_disc_inserted({}) -> state 0x{:02X}, pushing to Ch3 with IRQ2", inserted, disc_byte);
        self.prepare_readback(3, &[0xB0, 0x00, disc_byte, 0x15], 0xB0, true);
    }

    pub fn notify_disc_swapped(&mut self) {
        self.has_disc = true;
        self.disc_timer = 0;
        self.disc_state = 0x41;
        info!("Slave: notify_disc_swapped -> sending tray open (0x00) then spinup (0x41 -> 0x44)");
        self.prepare_readback(3, &[0xB0, 0x00, 0x00, 0x15], 0xB0, true);
    }

    pub fn tick(&mut self, cycles: u32) {
        if self.has_disc && self.disc_state == 0x41 {
            self.disc_timer += cycles;
            if self.disc_timer >= 4_500_000 {
                self.disc_state = 0x44;
                info!("Slave: disc TOC ready (0x41 -> 0x44) -> pushing status packet to Ch3 with IRQ2");
                self.prepare_readback(3, &[0xB0, 0x00, 0x44, 0x15], 0xB0, true);
            }
        }
    }

    pub fn read(&mut self, offset: u32) -> u16 {
        let ch_idx = ((offset >> 1) & 3) as usize;
        if self.channels[ch_idx].out_count > 0 {
            let val = self.channels[ch_idx].out_buf[self.channels[ch_idx].out_index];

            if self.channels[ch_idx].out_index == 0 {
                self.irq_pending = false;
            }

            let ch = &mut self.channels[ch_idx];
            ch.out_index += 1;
            ch.out_count -= 1;

            if self.debug_log && ch_idx == 0 {
                println!("[SLAVE R CH0] val=0x{:02X} (left={})", val, ch.out_count);
            }

            if ch.out_count == 0 {
                ch.out_index = 0;
                ch.out_cmd = 0;
                // Fine lettura CH0: promuovi il pacchetto in attesa (se presente)
                if ch_idx == 0 && ch.pending_count > 0 {
                    ch.out_index = 0;
                    ch.out_count = ch.pending_count;
                    ch.out_cmd = ch.pending_cmd;
                    ch.out_buf[..ch.pending_count].copy_from_slice(&ch.pending_buf[..ch.pending_count]);
                    ch.pending_count = 0;
                    ch.pending_cmd = 0;
                    ch.pending_buf = [0; 8];
                    self.irq_pending = true;
                }
            }

            return (val as u16) | ((val as u16) << 8);
        }

        0x00FF
    }

    pub fn write(&mut self, offset: u32, data: u16) {
        let ch_idx = ((offset >> 1) & 3) as usize;
        let byte_val = ((data >> 8) | (data & 0xFF)) as u8;
        if self.debug_log && ch_idx == 0 {
            println!("[SLAVE W CH0] byte=0x{:02X}", byte_val);
        }
        trace!("SLAVE WRITE ch{} (offset 0x{:X}) = 0x{:02X} (raw 0x{:04X})", ch_idx, offset, byte_val, data);

        match ch_idx {
            0 => {
                // Channel 0: Mouse
                let ch = &mut self.channels[0];
                if ch.in_index < ch.in_buf.len() {
                    ch.in_buf[ch.in_index] = byte_val;
                    ch.in_index += 1;
                }
                if ch.in_buf[0] >= 0xC0 {
                    if ch.in_index == 1 {
                        ch.in_count = 3;
                    } else if ch.in_count == 3 && ch.in_index == ch.in_count {
                        // Pacchetto completo (3 byte): decodifica ora, come MAME set_mouse_position()
                        self.device_mouse_x = (((ch.in_buf[1] & 0x70) as i32) << 3) | ((ch.in_buf[2] & 0x7F) as i32);
                        self.device_mouse_y = (((ch.in_buf[1] & 0x0F) as i32) << 6) | ((ch.in_buf[0] & 0x3F) as i32);
                        self.input_mouse_x = -1;
                        self.input_mouse_y = -1;
                        self.device_mouse_updated = true;
                        if self.debug_log {
                            println!(
                                "[SLAVE CH0 WARP] bytes=0x{:02X} 0x{:02X} 0x{:02X} -> device ({}, {})",
                                ch.in_buf[0], ch.in_buf[1], ch.in_buf[2], self.device_mouse_x, self.device_mouse_y
                            );
                        }
                        ch.in_buf = [0; 18];
                        ch.in_index = 0;
                        ch.in_count = 0;
                    }
                } else {
                    ch.in_index = 0;
                    ch.in_count = 0;
                }
            }
            1 => {
                // Channel 1: Front Panel LCD
                let ch = &mut self.channels[1];
                if ch.in_index == 0 && byte_val != 0xF0 {
                    return;
                }
                if ch.in_index < ch.in_buf.len() {
                    ch.in_buf[ch.in_index] = byte_val;
                    ch.in_index += 1;
                }
                if ch.in_index == 1 {
                    if byte_val == 0xF0 {
                        ch.in_count = 17;
                    } else {
                        ch.in_index = 0;
                        ch.in_count = 0;
                    }
                } else if ch.in_index == ch.in_count {
                    ch.in_index = 0;
                    ch.in_count = 0;
                }
            }
            2 => {
                // Channel 2: Audio Volume / Attenuation
                let ch = &mut self.channels[2];
                if ch.in_index < ch.in_buf.len() {
                    ch.in_buf[ch.in_index] = byte_val;
                    ch.in_index += 1;
                }
                if ch.in_index == 1 {
                    match byte_val {
                        0x82 | 0x83 => {
                            ch.in_index = 0;
                            ch.in_count = 0;
                        }
                        0x8A => {
                            // Reset player: the slave pulls the system reset line; the machine acts on it after this access
                            ch.in_index = 0;
                            ch.in_count = 0;
                            self.reset_request = true;
                        }
                        0xC0..=0xCF => {
                            ch.in_count = 5;
                        }
                        0xF0 => {
                            ch.in_count = 17;
                        }
                        _ => {
                            ch.in_index = 0;
                            ch.in_count = 0;
                        }
                    }
                } else if ch.in_index == ch.in_count {
                    ch.in_index = 0;
                    ch.in_count = 0;
                }
            }
            3 => {
                // Channel 3: CD / System
                let ch = &mut self.channels[3];
                if ch.in_index < ch.in_buf.len() {
                    ch.in_buf[ch.in_index] = byte_val;
                    ch.in_index += 1;
                }
                if ch.in_index == 1 {
                    match byte_val {
                        0x80 | 0x81 | 0xB0 | 0xB1 => {
                            ch.in_count = 4;
                        }
                        0xF0 => {
                            info!("Slave CMD 0xF0: Request Slave Revision");
                            self.prepare_readback(2, &[0xF0, 0x32, 0x31], 0xF0, true);
                            self.channels[3].in_index = 0;
                            self.channels[3].in_count = 0;
                        }
                        0xF3 => {
                            info!("Slave CMD 0xF3: Request Pointer Type");
                            self.prepare_readback(2, &[0xF3, 0x01], 0xF3, true);
                            self.channels[3].in_index = 0;
                            self.channels[3].in_count = 0;
                        }
                        0xF4 => {
                            info!("Slave CMD 0xF4: Request Test Plug Status");
                            self.prepare_readback(2, &[0xF4, 0x00], 0xF4, true);
                            self.channels[3].in_index = 0;
                            self.channels[3].in_count = 0;
                        }
                        0xF6 => {
                            // Authentic CD-i Slave 68HC05 response: 0x01 for NTSC (525 lines), 0x02 for PAL (625 lines)
                            let val = match self.video_mode {
                                VideoMode::Pal => 0x02,
                                VideoMode::Ntsc => 0x01,
                            };
                            trace!("Slave CMD 0xF6: Request NTSC/PAL Status -> 0x{:02X} ({:?})", val, self.video_mode);
                            // Polled synchronously by CPU; do NOT assert IRQ2
                            self.prepare_readback(2, &[0xF6, val], 0xF6, false);
                            self.channels[3].in_index = 0;
                            self.channels[3].in_count = 0;
                        }
                        0xF7 => {
                            info!("Slave CMD 0xF7: Arm Developer Mode");
                            self.channels[3].in_index = 0;
                            self.channels[3].in_count = 0;
                        }
                        0xFA => {
                            info!("Slave CMD 0xFA: Enable X-Bus Interrupts");
                            self.xbus_interrupt_enable = true;
                            self.channels[3].in_index = 0;
                            self.channels[3].in_count = 0;
                        }
                        _ => {
                            self.channels[3].in_index = 0;
                            self.channels[3].in_count = 0;
                        }
                    }
                } else if ch.in_index == ch.in_count {
                    match ch.in_buf[0] {
                        0xB0 => {
                            let disc_byte = if self.has_disc { self.disc_state } else { 0x00 };
                            info!("Slave CMD 0xB0 completed: Request Disc Status (has_disc={}) -> byte=0x{:02X}", self.has_disc, disc_byte);
                            self.prepare_readback(3, &[0xB0, 0x00, disc_byte, 0x15], 0xB0, true);
                        }
                        0xB1 => {
                            self.prepare_readback(3, &[0xB1, 0x00, 0x00, 0x00], 0xB1, true);
                        }
                        _ => {}
                    }
                    self.channels[3].in_index = 0;
                    self.channels[3].in_count = 0;
                }
            }
            _ => {}
        }
    }

    pub fn prepare_readback(&mut self, ch_idx: usize, data: &[u8], cmd: u8, assert_irq: bool) {
        let ch = &mut self.channels[ch_idx];
        // CH0: pacchetto arrivato mentre il precedente è in lettura -> stage (last-wins), nessuna promozione ora
        if ch_idx == 0 && ch.out_count > 0 && ch.out_index > 0 {
            ch.pending_count = data.len().min(ch.pending_buf.len());
            ch.pending_cmd = cmd;
            for (i, &b) in data.iter().take(ch.pending_count).enumerate() {
                ch.pending_buf[i] = b;
            }
            return;
        }
        ch.out_index = 0;
        ch.out_count = data.len().min(ch.out_buf.len());
        ch.out_cmd = cmd;
        for (i, &b) in data.iter().take(ch.out_count).enumerate() {
            ch.out_buf[i] = b;
        }
        if assert_irq {
            self.irq_pending = true;
        }
    }

    pub fn update_mouse(&mut self, x: i32, y: i32, btn: u8) {
        if x == self.input_mouse_x && y == self.input_mouse_y && btn == self.input_mouse_btn {
            return;
        }

        self.input_mouse_btn = btn;

        let mut button_bits = 0x01u8;
        if (btn & 1) != 0 { button_bits |= 0x02; }
        if (btn & 2) != 0 { button_bits |= 0x04; }
        if (btn & 4) != 0 { button_bits |= 0x06; }
        let b = button_bits << 3;

        self.device_mouse_x = x.clamp(0, 767);
        self.device_mouse_y = y.clamp(0, 559);

        self.input_mouse_x = x;
        self.input_mouse_y = y;

        let b0 = (((self.device_mouse_x & 0x380) >> 7) as u8) | b;
        let b1 = (self.device_mouse_x & 0x7F) as u8;
        let b2 = ((self.device_mouse_y & 0x380) >> 7) as u8;
        let b3 = (self.device_mouse_y & 0x7F) as u8;

        self.prepare_readback(0, &[b0, b1, b2, b3], 0xF7, true);
    }
}

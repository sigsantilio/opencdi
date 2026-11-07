use log::trace;

#[derive(Default, Clone, Copy)]
pub struct DmaChannel {
    pub channel_status: u8,
    pub channel_error: u8,
    pub device_control: u8,
    pub operation_control: u8,
    pub sequence_control: u8,
    pub channel_control: u8,
    pub transfer_counter: u16,
    pub memory_address_counter: u32,
    pub device_address_counter: u32,
}

pub struct SccDma {
    pub channels: [DmaChannel; 2],
}

impl SccDma {
    pub fn new() -> Self {
        Self {
            channels: [DmaChannel::default(); 2],
        }
    }

    pub fn read_reg(&self, offset: u32) -> u16 {
        let ch_idx = if offset < 0x40 { 0 } else { 1 };
        let ch = &self.channels[ch_idx];
        let ch_offset = offset % 0x40;

        match ch_offset {
            0x00 => ((ch.channel_status as u16) << 8) | (ch.channel_error as u16),
            0x04 => ((ch.device_control as u16) << 8) | (ch.operation_control as u16),
            0x06 => ((ch.sequence_control as u16) << 8) | (ch.channel_control as u16),
            0x0A => ch.transfer_counter,
            0x0C => (ch.memory_address_counter >> 16) as u16,
            0x0E => (ch.memory_address_counter & 0xFFFF) as u16,
            0x14 => (ch.device_address_counter >> 16) as u16,
            0x16 => (ch.device_address_counter & 0xFFFF) as u16,
            _ => 0,
        }
    }

    pub fn write_reg(&mut self, offset: u32, val: u16) {
        let ch_idx = if offset < 0x40 { 0 } else { 1 };
        let ch = &mut self.channels[ch_idx];
        let ch_offset = offset % 0x40;

        trace!("SCC DMA ch{} write offset 0x{:02X}: 0x{:04X}", ch_idx, ch_offset, val);

        match ch_offset {
            0x00 => {
                // Writing to status clears bits
                ch.channel_status &= !((val >> 8) as u8 & 0xB0);
            }
            0x04 => {
                ch.device_control = (val >> 8) as u8;
                ch.operation_control = (val & 0xFF) as u8;
            }
            0x06 => {
                ch.sequence_control = (val >> 8) as u8;
                ch.channel_control = (val & 0x7F) as u8;
                // If software operation bit set, flag complete
                if (val & 0x80) != 0 {
                    ch.channel_status |= 0x80;
                }
            }
            0x0A => {
                ch.transfer_counter = val;
            }
            0x0C => {
                ch.memory_address_counter = (ch.memory_address_counter & 0x0000_FFFF) | ((val as u32) << 16);
            }
            0x0E => {
                ch.memory_address_counter = (ch.memory_address_counter & 0xFFFF_0000) | (val as u32);
            }
            0x14 => {
                ch.device_address_counter = (ch.device_address_counter & 0x0000_FFFF) | ((val as u32) << 16);
            }
            0x16 => {
                ch.device_address_counter = (ch.device_address_counter & 0xFFFF_0000) | (val as u32);
            }
            _ => {}
        }
    }
}

opencdi::impl_state!(DmaChannel {
    channel_status, channel_error, device_control, operation_control, sequence_control, channel_control,
    transfer_counter, memory_address_counter, device_address_counter,
} skip {});

opencdi::impl_state!(SccDma { channels } skip {});

// MSB-first bit reader for MPEG streams.
// Ported to Rust from PL_MPEG by Dominic Szablewski (MIT licensed).

/// A bit reader over an owned byte buffer.
#[derive(Clone, Default)]
pub struct Buffer {
    pub bytes: Vec<u8>,
    /// Current read position, in bits.
    pub bit_index: usize,
}

impl Buffer {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            bit_index: 0,
        }
    }

    pub fn append(&mut self, data: &[u8]) {
        self.bytes.extend_from_slice(data);
    }

    /// Total length of the buffer in bits.
    pub fn len_bits(&self) -> usize {
        self.bytes.len() << 3
    }

    /// Whether the start code `00 00 01 <code>` appears at or after byte
    /// offset `from_byte`. Does not move the read position.
    pub fn contains_start_code_from(&self, from_byte: usize, code: u8) -> bool {
        let b = &self.bytes;
        let mut i = from_byte;
        while i + 4 <= b.len() {
            if b[i] == 0x00 && b[i + 1] == 0x00 && b[i + 2] == 0x01 && b[i + 3] == code {
                return true;
            }
            i += 1;
        }
        false
    }

    /// Total length of the buffer in bytes.
    pub fn len_bytes(&self) -> usize {
        self.bytes.len()
    }

    /// Whether at least `count` more bits are available.
    pub fn has(&self, count: usize) -> bool {
        self.len_bits().saturating_sub(self.bit_index) >= count
    }

    /// Whether all bits have been consumed.
    pub fn has_ended(&self) -> bool {
        self.bit_index >= self.len_bits()
    }

    /// Read `count` bits (0..=32) MSB-first as an unsigned integer.
    /// Returns 0 if not enough bits remain.
    pub fn read(&mut self, mut count: usize) -> u32 {
        if !self.has(count) {
            return 0;
        }
        let mut value: u32 = 0;
        while count > 0 {
            let current_byte = self.bytes[self.bit_index >> 3] as u32;
            let remaining = 8 - (self.bit_index & 7); // remaining bits in this byte
            let read = remaining.min(count); // bits read in this run
            let shift = remaining - read;
            let mask = 0xffu32 >> (8 - read);
            value = (value << read) | ((current_byte & (mask << shift)) >> shift);
            self.bit_index += read;
            count -= read;
        }
        value
    }

    /// Align the read position to the next byte boundary.
    pub fn align(&mut self) {
        self.bit_index = ((self.bit_index + 7) >> 3) << 3;
    }

    /// Skip `count` bits if available.
    pub fn skip(&mut self, count: usize) {
        if self.has(count) {
            self.bit_index += count;
        }
    }

    /// Align, then skip consecutive bytes equal to `v`. Returns the number
    /// of bytes skipped.
    pub fn skip_bytes(&mut self, v: u8) -> usize {
        self.align();
        let mut skipped = 0;
        while self.has(8) && self.bytes[self.bit_index >> 3] == v {
            self.bit_index += 8;
            skipped += 1;
        }
        skipped
    }

    /// Align, then scan forward for the next `00 00 01 xx` start code.
    /// Consumes up to and including the 4 marker bytes and returns `xx`,
    /// or -1 if none is found.
    pub fn next_start_code(&mut self) -> i32 {
        self.align();
        while self.has(5 << 3) {
            let byte_index = self.bit_index >> 3;
            if self.bytes[byte_index] == 0x00
                && self.bytes[byte_index + 1] == 0x00
                && self.bytes[byte_index + 2] == 0x01
            {
                self.bit_index = (byte_index + 4) << 3;
                return self.bytes[byte_index + 3] as i32;
            }
            self.bit_index += 8;
        }
        -1
    }

    /// Scan forward until the start code `code` (or end of buffer).
    pub fn find_start_code(&mut self, code: i32) -> i32 {
        loop {
            let current = self.next_start_code();
            if current == code || current == -1 {
                return current;
            }
        }
    }

    /// Peek whether start code `code` appears ahead, without consuming.
    pub fn has_start_code(&mut self, code: i32) -> i32 {
        let previous = self.bit_index;
        let current = self.find_start_code(code);
        self.bit_index = previous;
        current
    }

    /// Current read position in bytes (rounded down).
    pub fn tell(&self) -> usize {
        self.bit_index >> 3
    }

    /// Seek to a byte position.
    pub fn seek(&mut self, byte_pos: usize) {
        self.bit_index = byte_pos << 3;
    }

    /// Borrow the bytes of the next `count` bytes from the current
    /// (byte-aligned) position, without advancing. Returns None if out of range.
    pub fn peek_bytes(&self, count: usize) -> Option<&[u8]> {
        let start = self.bit_index >> 3;
        self.bytes.get(start..start + count)
    }

    /// Decode a value using a binary VLC table (PL_MPEG layout). Each entry is
    /// `(index, value)`; a positive `index` points to the next node (`index + bit`),
    /// `index <= 0` terminates with `value`.
    pub fn read_vlc(&mut self, table: &[(i16, i16)]) -> i16 {
        let mut index: i16 = 0;
        loop {
            let bit = self.read(1) as i16;
            let (next, value) = table[(index + bit) as usize];
            if next <= 0 {
                return value;
            }
            index = next;
        }
    }

    /// Decode a value using an unsigned binary VLC table.
    pub fn read_vlc_uint(&mut self, table: &[(i16, u16)]) -> u16 {
        let mut index: i16 = 0;
        loop {
            let bit = self.read(1) as i16;
            let (next, value) = table[(index + bit) as usize];
            if next <= 0 {
                return value;
            }
            index = next;
        }
    }

    /// Peek `bit_count` bits without consuming them; return whether non-zero.
    pub fn peek_non_zero(&mut self, bit_count: usize) -> bool {
        if !self.has(bit_count) {
            return false;
        }
        let saved = self.bit_index;
        let val = self.read(bit_count);
        self.bit_index = saved;
        val != 0
    }

    /// Discard bytes that have already been read to keep buffer compact.
    pub fn discard_read_bytes(&mut self) {
        let byte_pos = self.bit_index >> 3;
        if byte_pos > 32 * 1024 {
            self.bytes.drain(0..byte_pos);
            self.bit_index &= 7;
        }
    }

    pub fn find_frame_sync(&mut self) -> bool {
        let mut i = self.bit_index >> 3;
        let last = self.bytes.len().saturating_sub(1);
        while i < last {
            if self.bytes[i] == 0xFF && (self.bytes[i + 1] & 0xFE) == 0xFC {
                self.bit_index = ((i + 1) << 3) + 3;
                return true;
            }
            i += 1;
        }
        self.bit_index = (i + 1) << 3;
        false
    }
}

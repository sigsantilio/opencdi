// System ROM checks on byte buffers (no files): identify by hash, undo word swaps, flag truncated or foreign images; see doc/handover/bios-detection.md.
use std::fmt;
use std::io::Read;

/// Size of every CD-i system ROM we know (512 KB).
pub const SYSTEM_ROM_SIZE: usize = 512 * 1024;
// A zero or 0xFF tail at least this long marks an unknown image as probably truncated.
const TRUNCATED_TAIL: usize = 64 * 1024;
// Upper bound for one extracted zip entry, so a broken or hostile archive cannot exhaust memory.
const ZIP_ENTRY_MAX: usize = 16 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    Mono1,
    Mono2,
    MiniMmc,
    Mono4,
    Other,
}

impl Board {
    /// Boards OpenCDI emulates today.
    pub fn emulated(self) -> bool {
        self == Board::Mono1
    }

    pub fn name(self) -> &'static str {
        match self {
            Board::Mono1 => "Mono-I",
            Board::Mono2 => "Mono-II",
            Board::MiniMmc => "Mini-MMC",
            Board::Mono4 => "Mono-IV",
            Board::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    System,
    Dvc,
    Mcu,
}

/// Byte order of the image whose hash a table row lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Cpu,
    WordSwapped,
}

#[derive(Debug)]
pub struct KnownRom {
    pub file: &'static str,
    pub model: &'static str,
    pub board: Board,
    pub kind: Kind,
    pub size: usize,
    pub crc32: u32,
    pub sha1: &'static str,
    pub layout: Layout,
    pub good: bool,
    pub note: &'static str,
}

const fn rom(file: &'static str, model: &'static str, board: Board, kind: Kind, size: usize, crc32: u32, sha1: &'static str, layout: Layout, good: bool, note: &'static str) -> KnownRom {
    KnownRom { file, model, board, kind, size, crc32, sha1, layout, good, note }
}

use Board::*;
use Kind::*;
use Layout::*;

/// Known images, in order of preference.
pub static KNOWN: &[KnownRom] = &[
    rom("cdi220b.rom", "Philips CD-i 220 F2", Mono1, System, SYSTEM_ROM_SIZE, 0x279683ca, "53360a1f21ddac952e95306ced64186a3fc0b93e", Cpu, true, ""),
    rom("cdi200.rom", "Magnavox CD-i 200", Mono1, System, SYSTEM_ROM_SIZE, 0x40c4e6b9, "d961de803c89b3d1902d656ceb9ce7c02dccb40a", Cpu, true, ""),
    rom("cdi220.rom", "Philips CD-i 220 (alternative)", Mono1, System, SYSTEM_ROM_SIZE, 0x584c0af8, "5d757ab46b8c8fc36361555d978d7af768342d47", Cpu, false, "truncated dump: every byte from offset 0x5F88E is zero and the player shell is missing; use cdi220b.rom"),
    rom("cdi490a.rom", "Philips CD-i 490", Mono4, System, SYSTEM_ROM_SIZE, 0xe2f200f6, "c9bf3c4c7e4fe5cbec3fe3fc993c77a4522ca547", WordSwapped, true, ""),
    rom("philips__cdi-220_ph3_r1.2__mb834200b-15__02f_aa__9402_z04.tc574200-le._1.7211", "Philips CD-i 220 F3", Mono2, System, SYSTEM_ROM_SIZE, 0x17d723e7, "6c317a82e35d60ca5e7a74fc99f665055693169d", WordSwapped, true, ""),
    rom("philips__cd-i_4.1_r1.1__mb834200b-15__10e_aa__9336_z01.7211", "Philips CD-i 210 F2", Mono2, System, SYSTEM_ROM_SIZE, 0x8453553f, "5ee4dc3e7eb4c3867ac9d04f1614908906af19fb", WordSwapped, true, ""),
    rom("philips__cd-i_2.1__mb834200b-15__26b_aa__9224_z01.tc574200.7211", "Philips CD-i 910-17P Mini-MMC", MiniMmc, System, SYSTEM_ROM_SIZE, 0x4ae3bee3, "9729b4ee3ce0c17172d062339c47b1ab822b222b", WordSwapped, true, ""),
    rom("cdi910.rom", "Philips CD-i 910 (alternative)", MiniMmc, System, SYSTEM_ROM_SIZE, 0x2f3048d2, "11c4c3e602060518b52e77156345fa01f619e793", WordSwapped, true, ""),
    rom("gpi-1200k-1313.bin", "Goldstar GPi 1200", Other, System, SYSTEM_ROM_SIZE, 0xdbd41615, "83929617a5c01551ee961aeb685295fcc0810f54", WordSwapped, true, ""),
    rom("vmpega.rom", "Philips VMPEG Digital Video Cartridge", Mono1, Dvc, 256 * 1024, 0xdb264e8b, "be407fbc102f1731a0862554855e963e5a47c17b", Cpu, true, ""),
    rom("impega.rom", "Philips IMPEG Digital Video Cartridge", Mono4, Dvc, 256 * 1024, 0x84d6f6aa, "02526482a0851ea2a7b582d8afaa8ef14a8bd914", Cpu, true, ""),
    rom("zx405037p__cdi_servo_2.1__b43t__llek9215.mc68hc705c8a_withtestrom.7201", "CD-i servo MCU 2.1", Mono1, Mcu, 8 * 1024, 0x7a3af407, "fdf8d78d6a0df4a56b5b963d72eabd39fcec163f", Cpu, true, ""),
    rom("zx405042p__cdi_slave_2.0__b43t__zzmk9213.mc68hc705c8a_withtestrom.7206", "CD-i slave MCU 2.0", Mono1, Mcu, 8 * 1024, 0x688cda63, "56d0acd7caad51c7de703247cd6d842b36173079", Cpu, true, ""),
    rom("zc405351p__servo_cdi_4.1__0d67p__lluk9404.mc68hc705c8a.7490", "CD-i servo MCU 4.1", Mono2, Mcu, 8 * 1024, 0x2bc8e4e9, "8cd052b532fc052d6b0077261c12f800e8655bb1", Cpu, true, ""),
    rom("zc405352p__slave_cdi_4.1__0d67p__lltr9403.mc68hc705c8a.7206", "CD-i slave MCU 4.1", Mono2, Mcu, 8 * 1024, 0x5b19da07, "cf02d84977050c71e87a38f1249e83c43a93949b", Cpu, true, ""),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Ok,
    Fixed,
    Warning,
    Error,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Status::Ok => "ok",
            Status::Fixed => "fixed",
            Status::Warning => "warning",
            Status::Error => "error",
        })
    }
}

#[derive(Debug, Clone)]
pub struct Report {
    pub status: Status,
    pub known: Option<&'static KnownRom>,
    pub board: Option<Board>,
    pub word_swapped: bool,
    pub sha1: String,
    pub message: String,
}

impl Report {
    pub fn model(&self) -> Option<&'static str> {
        self.known.map(|k| k.model)
    }

    /// False for errors and for boards OpenCDI does not emulate; unknown boards are assumed Mono-I.
    pub fn usable(&self) -> bool {
        self.status != Status::Error && self.board.is_none_or(Board::emulated)
    }

    /// 3: good and emulated, 2: usable with a warning, 1: valid but board not emulated, 0: error.
    pub fn rank(&self) -> u8 {
        match (self.status, self.usable()) {
            (Status::Error, _) => 0,
            (_, false) => 1,
            (Status::Warning, true) => 2,
            _ => 3,
        }
    }

    fn preference(&self) -> usize {
        self.known.and_then(|k| KNOWN.iter().position(|r| std::ptr::eq(r, k))).unwrap_or(KNOWN.len())
    }
}

/// A checked image; `data` is in CPU (big-endian word) order unless the status is `Error`.
#[derive(Debug, Clone)]
pub struct BiosImage {
    pub data: Vec<u8>,
    pub report: Report,
}

pub fn crc32(data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut t = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                k += 1;
            }
            t[i] = c;
            i += 1;
        }
        t
    };
    !data.iter().fold(!0u32, |c, &b| TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8))
}

pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let full = data.len() / 64 * 64;
    let mut tail = data[full..].to_vec();
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&(data.len() as u64).wrapping_mul(8).to_be_bytes());
    for block in data[..full].chunks_exact(64).chain(tail.chunks_exact(64)) {
        let mut w = [0u32; 80];
        for (i, c) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(wi);
            (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (o, x) in out.chunks_exact_mut(4).zip(h) {
        o.copy_from_slice(&x.to_be_bytes());
    }
    out
}

pub fn sha1_hex(data: &[u8]) -> String {
    sha1(data).iter().map(|b| format!("{:02x}", b)).collect()
}

/// Exchanges the two bytes of every 16-bit word (and back).
pub fn swap_words(data: &mut [u8]) {
    for w in data.chunks_exact_mut(2) {
        w.swap(0, 1);
    }
}

fn swapped(data: &[u8]) -> Vec<u8> {
    let mut v = data.to_vec();
    swap_words(&mut v);
    v
}

/// Finds the table row for `data`; the flag says whether the bytes must be swapped to reach CPU order.
pub fn identify(data: &[u8]) -> Option<(&'static KnownRom, bool)> {
    identify_in(KNOWN.iter(), data)
}

fn identify_in<'a>(rows: impl Iterator<Item = &'a KnownRom>, data: &[u8]) -> Option<(&'a KnownRom, bool)> {
    let rows: Vec<&KnownRom> = rows.filter(|k| k.size == data.len()).collect();
    if rows.is_empty() {
        return None;
    }
    let raw_crc = crc32(data);
    let swapped_data = if data.len() % 2 == 0 { Some(swapped(data)) } else { None };
    let swapped_crc = swapped_data.as_deref().map(crc32);
    let (mut raw_sha, mut swp_sha) = (None, None);
    for k in rows {
        if k.crc32 == raw_crc && raw_sha.get_or_insert_with(|| sha1_hex(data)) == k.sha1 {
            return Some((k, k.layout == WordSwapped && k.kind == System));
        }
        if let (Some(s), Some(c)) = (swapped_data.as_deref(), swapped_crc) {
            if k.crc32 == c && k.kind == System && swp_sha.get_or_insert_with(|| sha1_hex(s)) == k.sha1 {
                return Some((k, k.layout == Cpu));
            }
        }
    }
    None
}

fn be32(d: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
}

// Reset vector of a CD-i system ROM: supervisor stack in RAM, entry point inside the 512 KB ROM window at 0x400000.
fn plausible_vector(d: &[u8]) -> bool {
    let (ssp, pc) = (be32(d, 0), be32(d, 4));
    ssp != 0 && ssp & 1 == 0 && ssp < 0x0010_0000 && pc & 1 == 0 && (0x0040_0000..0x0048_0000).contains(&pc)
}

fn foreign_format(d: &[u8]) -> Option<&'static str> {
    let starts = |m: &[u8]| d.starts_with(m);
    if starts(b"PK\x03\x04") {
        Some("a zip archive; extract the .rom file or name the zip cdimono1.zip")
    } else if starts(b"7z\xBC\xAF\x27\x1C") {
        Some("a 7-Zip archive; extract the .rom file first")
    } else if starts(b"Rar!") {
        Some("a RAR archive; extract the .rom file first")
    } else if starts(b"\x1F\x8B") {
        Some("a gzip file; extract the .rom file first")
    } else if starts(b"MComprHD") {
        Some("a CHD disc image, not a system ROM")
    } else if starts(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]) {
        Some("a raw CD sector image (disc), not a system ROM")
    } else if starts(&[0x4A, 0xFC]) && d.len() < SYSTEM_ROM_SIZE {
        Some("a single OS-9 module (a cartridge driver or data file), not a system ROM")
    } else if d.len() < 64 * 1024 && d.iter().take(512).all(|&b| b == b'\t' || b == b'\n' || b == b'\r' || (0x20..0x7F).contains(&b)) {
        Some("a text file (cue sheet or notes?), not a system ROM")
    } else {
        None
    }
}

fn tail_fill(d: &[u8]) -> (usize, u8) {
    let fill = *d.last().unwrap_or(&0);
    if fill != 0x00 && fill != 0xFF {
        return (0, fill);
    }
    (d.iter().rev().take_while(|&&b| b == fill).count(), fill)
}

/// Whether the last word holds the 16-bit sum of all other bytes (true for the Mono-I, Mono-II and Mono-IV dumps we hold).
pub fn checksum_ok(d: &[u8]) -> bool {
    let n = d.len();
    n >= 2 && d[..n - 2].iter().fold(0u16, |s, &b| s.wrapping_add(b as u16)) == u16::from_be_bytes([d[n - 2], d[n - 1]])
}

fn report(status: Status, known: Option<&'static KnownRom>, board: Option<Board>, word_swapped: bool, sha1: String, message: String) -> Report {
    Report { status, known, board, word_swapped, sha1, message }
}

/// Checks a would-be system ROM and returns it in CPU order; word-swapped dumps are fixed in place.
pub fn inspect(mut data: Vec<u8>) -> BiosImage {
    let err = |data: Vec<u8>, known: Option<&'static KnownRom>, msg: String| {
        let sha = sha1_hex(&data);
        BiosImage { report: report(Status::Error, known, None, false, sha, msg), data }
    };
    if data.is_empty() {
        return err(data, None, "file is empty".into());
    }
    if let Some((k, swap)) = identify(&data) {
        match k.kind {
            Dvc => return err(data, Some(k), format!("this is the {} ROM ({}), a cartridge ROM, not a system ROM", k.model, k.file)),
            Mcu => return err(data, Some(k), format!("this is the {} ROM, not a system ROM", k.model)),
            System => {}
        }
        if swap {
            swap_words(&mut data);
        }
        let sha = sha1_hex(&data);
        let mut msg = format!("{} ({} board), {}", k.model, k.board.name(), k.file);
        let status = if !k.good {
            msg += &format!(": {}", k.note);
            Status::Warning
        } else if swap {
            msg += ": word-swapped dump, byte order fixed";
            Status::Fixed
        } else {
            msg += ": known good dump";
            Status::Ok
        };
        if !k.board.emulated() {
            msg += &format!("; the {} board is not emulated yet, so it will not boot", k.board.name());
        }
        return BiosImage { report: report(status, Some(k), Some(k.board), swap, sha, msg), data };
    }
    if let Some(what) = foreign_format(&data) {
        return err(data, None, format!("not a CD-i system ROM: this is {}", what));
    }
    if data.len() != SYSTEM_ROM_SIZE {
        let hint = if data.len() < SYSTEM_ROM_SIZE { ", too short (truncated download or partial dump?)" } else { "" };
        let msg = format!("size is {} bytes, a CD-i system ROM is {} bytes{}", data.len(), SYSTEM_ROM_SIZE, hint);
        return err(data, None, msg);
    }
    if data.iter().all(|&b| b == data[0]) {
        let msg = format!("image is blank: every byte is 0x{:02X}", data[0]);
        return err(data, None, msg);
    }
    let swap = if plausible_vector(&data) {
        false
    } else if plausible_vector(&[data[1], data[0], data[3], data[2], data[5], data[4], data[7], data[6]]) {
        true
    } else {
        let msg = format!(
            "not a CD-i system ROM: reset vector reads SSP {:08X}, PC {:08X} (expected a stack in RAM such as 00001500 and an entry point in 00400000-0047FFFF)",
            be32(&data, 0), be32(&data, 4)
        );
        return err(data, None, msg);
    };
    if swap {
        swap_words(&mut data);
    }
    let sha = sha1_hex(&data);
    let (tail, fill) = tail_fill(&data);
    let mut msg = format!("unknown image (SHA-1 {} not in the table)", sha);
    if swap {
        msg += ", word-swapped, byte order fixed";
    }
    if tail >= TRUNCATED_TAIL {
        msg += &format!("; the last {} bytes are all 0x{:02X}, so the dump is probably truncated", tail, fill);
    } else if checksum_ok(&data) {
        msg += "; its built-in checksum is valid, but it may not boot";
    } else {
        msg += "; its built-in checksum does not match (corrupt, patched or a board without one?), it may not boot";
    }
    BiosImage { report: report(Status::Warning, None, None, swap, sha, msg), data }
}

/// Size of the Digital Video Cartridge ROM (256 KB).
pub const DVC_ROM_SIZE: usize = 256 * 1024;

/// Checks a would-be DVC cartridge ROM; only the known VMPEG dump comes back `Ok`, since only VMPEG is emulated.
pub fn inspect_dvc(data: Vec<u8>) -> BiosImage {
    let sha = sha1_hex(&data);
    let done = |data: Vec<u8>, status: Status, known: Option<&'static KnownRom>, msg: String| {
        let board = if status == Status::Ok { known.map(|k| k.board) } else { None };
        BiosImage { report: report(status, known, board, false, sha.clone(), msg), data }
    };
    if data.is_empty() {
        return done(data, Status::Error, None, "file is empty".into());
    }
    if let Some((k, _)) = identify(&data) {
        return match (k.kind, k.board) {
            (Dvc, Mono1) => done(data, Status::Ok, Some(k), format!("{}, {}: known good dump", k.model, k.file)),
            (Dvc, _) => done(data, Status::Error, Some(k), format!("{} ({}, {} board): only the VMPEG cartridge is emulated, not fitted", k.model, k.file, k.board.name())),
            _ => done(data, Status::Error, Some(k), format!("this is the {} ROM, not a DVC cartridge ROM", k.model)),
        };
    }
    if let Some(what) = foreign_format(&data) {
        return done(data, Status::Error, None, format!("not a DVC cartridge ROM: this is {}", what));
    }
    if data.len() != DVC_ROM_SIZE {
        let msg = format!("size is {} bytes, the VMPEG cartridge ROM is {} bytes", data.len(), DVC_ROM_SIZE);
        return done(data, Status::Error, None, msg);
    }
    let msg = format!("unknown cartridge image (SHA-1 {} not in the table), may not work", sha);
    done(data, Status::Warning, None, msg)
}

/// Checks every 256 KB entry of a zip (`cdimono1.zip` carries `vmpega.rom`) and returns the best one with its entry name.
pub fn best_dvc_in_zip(zip: &[u8]) -> Result<(String, BiosImage), String> {
    let mut found = Vec::new();
    for e in zip_entries(zip)?.into_iter().filter(|e| e.size == DVC_ROM_SIZE) {
        found.push((e.name.clone(), inspect_dvc(e.extract()?)));
    }
    best(found).ok_or_else(|| "zip holds no 256 KB cartridge ROM".into())
}

/// Picks the best image: highest rank, then table preference, then input order.
pub fn best<T>(images: impl IntoIterator<Item = (T, BiosImage)>) -> Option<(T, BiosImage)> {
    let mut best: Option<(T, BiosImage)> = None;
    for (t, img) in images {
        let key = |r: &Report| (r.rank(), std::cmp::Reverse(r.preference()));
        if best.as_ref().is_none_or(|(_, b)| key(&img.report) > key(&b.report)) {
            best = Some((t, img));
        }
    }
    best
}

pub fn is_zip(data: &[u8]) -> bool {
    data.starts_with(b"PK\x03\x04") || data.starts_with(b"PK\x05\x06")
}

/// One file inside a zip archive (stored or deflated, no encryption, no zip64).
pub struct ZipEntry<'a> {
    pub name: String,
    pub size: usize,
    method: u16,
    crc: u32,
    flags: u16,
    raw: &'a [u8],
}

fn le16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn le32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

impl ZipEntry<'_> {
    /// File name without any folder part, lower case.
    pub fn base_name(&self) -> String {
        self.name.rsplit(['/', '\\']).next().unwrap_or(&self.name).to_ascii_lowercase()
    }

    pub fn extract(&self) -> Result<Vec<u8>, String> {
        if self.flags & 1 != 0 {
            return Err(format!("{}: encrypted zip entries are not supported", self.name));
        }
        if self.size > ZIP_ENTRY_MAX {
            return Err(format!("{}: entry of {} bytes is too large", self.name, self.size));
        }
        let out = match self.method {
            0 => self.raw.to_vec(),
            8 => {
                let mut out = Vec::with_capacity(self.size);
                flate2::read::DeflateDecoder::new(self.raw)
                    .take(ZIP_ENTRY_MAX as u64 + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| format!("{}: inflate failed: {}", self.name, e))?;
                out
            }
            m => return Err(format!("{}: zip compression method {} is not supported (use deflate or store)", self.name, m)),
        };
        if out.len() != self.size || crc32(&out) != self.crc {
            return Err(format!("{}: zip entry is damaged (size or CRC mismatch)", self.name));
        }
        Ok(out)
    }
}

/// Lists the entries of a zip archive held in memory.
pub fn zip_entries(zip: &[u8]) -> Result<Vec<ZipEntry<'_>>, String> {
    let bad = || "zip archive is damaged or uses an unsupported format (zip64?)".to_string();
    let lo = zip.len().saturating_sub(22 + 0xFFFF);
    let eocd = (lo..=zip.len().saturating_sub(22)).rev().find(|&i| le32(zip, i) == Some(0x0605_4B50)).ok_or_else(bad)?;
    let count = le16(zip, eocd + 10).ok_or_else(bad)? as usize;
    let mut at = le32(zip, eocd + 16).ok_or_else(bad)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if le32(zip, at) != Some(0x0201_4B50) {
            return Err(bad());
        }
        let flags = le16(zip, at + 8).ok_or_else(bad)?;
        let method = le16(zip, at + 10).ok_or_else(bad)?;
        let crc = le32(zip, at + 16).ok_or_else(bad)?;
        let csize = le32(zip, at + 20).ok_or_else(bad)? as usize;
        let size = le32(zip, at + 24).ok_or_else(bad)? as usize;
        let (n, x, c) = (le16(zip, at + 28).ok_or_else(bad)? as usize, le16(zip, at + 30).ok_or_else(bad)? as usize, le16(zip, at + 32).ok_or_else(bad)? as usize);
        let local = le32(zip, at + 42).ok_or_else(bad)? as usize;
        let name = String::from_utf8_lossy(zip.get(at + 46..at + 46 + n).ok_or_else(bad)?).into_owned();
        if le32(zip, local) != Some(0x0403_4B50) {
            return Err(bad());
        }
        let start = local + 30 + le16(zip, local + 26).ok_or_else(bad)? as usize + le16(zip, local + 28).ok_or_else(bad)? as usize;
        let raw = zip.get(start..start.checked_add(csize).ok_or_else(bad)?).ok_or_else(bad)?;
        out.push(ZipEntry { name, size, method, crc, flags, raw });
        at += 46 + n + x + c;
    }
    Ok(out)
}

/// Checks every 512 KB entry of a zip (`cdimono1.zip` style) and returns the best one with its entry name.
pub fn best_in_zip(zip: &[u8]) -> Result<(String, BiosImage), String> {
    let mut found = Vec::new();
    let mut problems = Vec::new();
    for e in zip_entries(zip)?.into_iter().filter(|e| e.size == SYSTEM_ROM_SIZE) {
        match e.extract() {
            Ok(d) => found.push((e.name.clone(), inspect(d))),
            Err(m) => problems.push(m),
        }
    }
    best(found).ok_or_else(|| {
        if problems.is_empty() { "zip holds no 512 KB system ROM".into() } else { problems.join("; ") }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic Mono-I style image: reset vector, filler that never repeats a long run, no real ROM bytes.
    fn synthetic() -> Vec<u8> {
        let mut d: Vec<u8> = (0..SYSTEM_ROM_SIZE as u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8 | 1).collect();
        d[..8].copy_from_slice(&[0x00, 0x00, 0x15, 0x00, 0x00, 0x40, 0x04, 0xB8]);
        d
    }

    #[test]
    fn hashes_match_reference_vectors() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(sha1_hex(&[b'a'; 1000]), "291e9a6c66994949b57ba5e650361e98fc36b1ba");
    }

    #[test]
    fn table_rows_are_well_formed() {
        for k in KNOWN {
            assert_eq!(k.sha1.len(), 40, "{}", k.file);
            assert!(k.sha1.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{}", k.file);
            assert!(k.good || !k.note.is_empty(), "{}", k.file);
        }
    }

    #[test]
    fn unknown_cpu_order_image_loads_with_warning() {
        let img = inspect(synthetic());
        assert_eq!(img.report.status, Status::Warning);
        assert!(!img.report.word_swapped);
        assert!(img.report.usable());
        assert_eq!(img.data, synthetic());
    }

    #[test]
    fn checksum_is_reported_for_unknown_images() {
        let mut d = synthetic();
        assert!(!checksum_ok(&d));
        assert!(inspect(d.clone()).report.message.contains("does not match"));
        let n = d.len();
        let sum = d[..n - 2].iter().fold(0u16, |s, &b| s.wrapping_add(b as u16));
        d[n - 2..].copy_from_slice(&sum.to_be_bytes());
        assert!(checksum_ok(&d));
        assert!(inspect(d.clone()).report.message.contains("checksum is valid"));
        swap_words(&mut d);
        let img = inspect(d);
        assert!(img.report.word_swapped && img.report.message.contains("checksum is valid"));
    }

    #[test]
    fn unknown_swapped_image_is_fixed() {
        let mut d = synthetic();
        swap_words(&mut d);
        let img = inspect(d);
        assert_eq!(img.report.status, Status::Warning);
        assert!(img.report.word_swapped);
        assert_eq!(img.data, synthetic());
    }

    #[test]
    fn zero_tail_is_reported_as_truncated() {
        let mut d = synthetic();
        d[0x5F88E..].fill(0);
        let img = inspect(d);
        assert_eq!(img.report.status, Status::Warning);
        assert!(img.report.message.contains("truncated"), "{}", img.report.message);
    }

    #[test]
    fn short_and_blank_and_foreign_images_are_rejected() {
        let mut short = synthetic();
        short.truncate(300_000);
        assert!(inspect(short).report.message.contains("too short"));
        assert!(inspect(vec![0xFF; SYSTEM_ROM_SIZE]).report.message.contains("blank"));
        assert_eq!(inspect(Vec::new()).report.status, Status::Error);
        let mut chd = synthetic();
        chd[..8].copy_from_slice(b"MComprHD");
        assert!(inspect(chd).report.message.contains("CHD"));
        assert!(inspect([0x4A, 0xFC, 0, 1, 0, 0, 0, 8].to_vec()).report.message.contains("OS-9 module"));
        assert!(inspect(b"FILE \"x.bin\" BINARY\n".to_vec()).report.message.contains("text"));
        let mut junk = synthetic();
        junk[..8].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0x12, 0x34, 0x56, 0x78]);
        let r = inspect(junk).report;
        assert_eq!(r.status, Status::Error);
        assert!(r.message.contains("reset vector"), "{}", r.message);
        assert_eq!(r.rank(), 0);
    }

    // Puts a synthetic image's hashes into a table row so identify() can be tested without real ROM bytes.
    fn leak_row(data: &[u8], layout: Layout, board: Board, good: bool) -> &'static KnownRom {
        let sha: &'static str = Box::leak(sha1_hex(data).into_boxed_str());
        Box::leak(Box::new(rom("test.rom", "Test", board, System, data.len(), crc32(data), sha, layout, good, "test note")))
    }

    #[test]
    fn swap_flag_follows_row_layout() {
        let cpu = synthetic();
        let mut wswap = synthetic();
        swap_words(&mut wswap);
        let cpu_row = leak_row(&cpu, Cpu, Mono1, true);
        let swapped_row = leak_row(&wswap, WordSwapped, Mono4, true);
        let find = |row: &'static KnownRom, d: &[u8]| identify_in([row].into_iter(), d).map(|r| r.1);
        assert_eq!(find(cpu_row, &cpu), Some(false));
        assert_eq!(find(cpu_row, &wswap), Some(true));
        assert_eq!(find(swapped_row, &wswap), Some(true));
        assert_eq!(find(swapped_row, &cpu), Some(false));
        assert_eq!(find(cpu_row, &vec![0u8; SYSTEM_ROM_SIZE]), None);
    }

    #[test]
    fn best_prefers_good_emulated_images() {
        let warn = inspect(synthetic());
        let mut bad = synthetic();
        bad[0] = 0xFF;
        let bad = inspect(bad);
        assert_eq!(bad.report.status, Status::Error);
        let (name, _) = best([("bad", bad.clone()), ("warn", warn.clone())]).unwrap();
        assert_eq!(name, "warn");
        let mut ok = warn.clone();
        ok.report.status = Status::Fixed;
        let (name, _) = best([("warn", warn), ("bad", bad), ("ok", ok)]).unwrap();
        assert_eq!(name, "ok");
        let mut mono4 = inspect(synthetic());
        mono4.report.board = Some(Mono4);
        assert_eq!(mono4.report.rank(), 1);
    }

    fn zip_of(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        use flate2::write::DeflateEncoder;
        use std::io::Write;
        let (mut out, mut cd) = (Vec::new(), Vec::new());
        for &(name, data, deflate) in entries {
            let body = if deflate {
                let mut e = DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
                e.write_all(data).unwrap();
                e.finish().unwrap()
            } else {
                data.to_vec()
            };
            let method: u16 = if deflate { 8 } else { 0 };
            let off = out.len() as u32;
            let mut common = Vec::new();
            common.extend_from_slice(&20u16.to_le_bytes());
            common.extend_from_slice(&0u16.to_le_bytes());
            common.extend_from_slice(&method.to_le_bytes());
            common.extend_from_slice(&[0; 4]);
            common.extend_from_slice(&crc32(data).to_le_bytes());
            common.extend_from_slice(&(body.len() as u32).to_le_bytes());
            common.extend_from_slice(&(data.len() as u32).to_le_bytes());
            common.extend_from_slice(&(name.len() as u16).to_le_bytes());
            common.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
            out.extend_from_slice(&common);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&body);
            cd.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
            cd.extend_from_slice(&20u16.to_le_bytes());
            cd.extend_from_slice(&common);
            cd.extend_from_slice(&[0; 10]);
            cd.extend_from_slice(&off.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn zip_entries_extract_and_pick_the_rom() {
        let mut swapped_rom = synthetic();
        swap_words(&mut swapped_rom);
        let z = zip_of(&[("readme.txt", b"hello", false), ("set/cdi490a.rom", &swapped_rom, true)]);
        assert!(is_zip(&z));
        let entries = zip_entries(&z).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].base_name(), "cdi490a.rom");
        assert_eq!(entries[0].extract().unwrap(), b"hello");
        let (name, img) = best_in_zip(&z).unwrap();
        assert_eq!(name, "set/cdi490a.rom");
        assert!(img.report.word_swapped);
        assert_eq!(img.data, synthetic());
    }

    #[test]
    fn dvc_check_refuses_system_and_unknown_images() {
        assert_eq!(inspect_dvc(Vec::new()).report.status, Status::Error);
        let sys = inspect_dvc(synthetic());
        assert_eq!(sys.report.status, Status::Error);
        assert!(sys.report.message.contains("bytes"), "{}", sys.report.message);
        let unknown = inspect_dvc(synthetic()[..DVC_ROM_SIZE].to_vec());
        assert_eq!(unknown.report.status, Status::Warning);
        assert!(unknown.report.message.contains("unknown cartridge"));
        let z = zip_of(&[("cdi220b.rom", &synthetic(), false), ("vmpega.rom", &synthetic()[..DVC_ROM_SIZE], true)]);
        let (name, img) = best_dvc_in_zip(&z).unwrap();
        assert_eq!((name.as_str(), img.report.status), ("vmpega.rom", Status::Warning));
        assert!(best_dvc_in_zip(&zip_of(&[("cdi220b.rom", &synthetic(), false)])).is_err());
    }

    #[test]
    fn damaged_zip_is_rejected() {
        let mut z = zip_of(&[("cdi220b.rom", &synthetic(), true)]);
        assert!(zip_entries(&z[..z.len() - 10]).is_err());
        let mid = z.len() / 3;
        z[mid] ^= 0xFF;
        assert!(best_in_zip(&z).is_err());
        assert!(inspect(z).report.message.contains("zip"));
    }
}

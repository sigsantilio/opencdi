use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use chd::Chd;
use log::{info, error};

pub const CD_SECTOR_RAW_SIZE: usize = 2352;
pub const CD_SUBCHANNEL_SIZE: usize = 96;
pub const CD_FRAME_SIZE: usize = CD_SECTOR_RAW_SIZE + CD_SUBCHANNEL_SIZE; // 2448
pub const CD_SCRAMBLE_SIZE: usize = 2340;
pub const CD_TRACK_PADDING: u32 = 4;
pub const CD_SYNC_HEADER: [u8; 12] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

static CD_SCRAMBLE_TABLE: std::sync::LazyLock<[u8; CD_SCRAMBLE_SIZE]> = std::sync::LazyLock::new(|| {
    let mut table = [0u8; CD_SCRAMBLE_SIZE];
    let mut reg = 0x0001u16;
    for i in 0..CD_SCRAMBLE_SIZE {
        let mut b = 0u8;
        for bit in 0..8 {
            b |= ((reg & 1) as u8) << bit;
            let new_bit = (reg & 1) ^ ((reg >> 1) & 1);
            reg = (reg >> 1) | (new_bit << 14);
        }
        table[i] = b;
    }
    table
});

/// Descrambles standard Yellow/Green Book Mode 1 / Mode 2 sectors if they were ripped raw
/// (e.g. CD-i Ready discs where Track 1 is dumped as an audio track without hardware descrambling).
pub fn maybe_descramble_sector(sector: &mut [u8; CD_SECTOR_RAW_SIZE]) {
    if sector[0..12] != CD_SYNC_HEADER {
        return;
    }
    let mode_byte = sector[15];
    if (mode_byte == 0x62 || mode_byte == 0x61 || mode_byte == 0x60) && (sector[13] & 0x80) != 0 {
        let table = &*CD_SCRAMBLE_TABLE;
        for i in 0..CD_SCRAMBLE_SIZE {
            sector[12 + i] ^= table[i];
        }
    }
}

#[derive(Clone, Debug)]
pub struct CdTrack {
    pub number: u8,
    pub control: u8,            // 0x41 for Data, 0x01 for Audio
    pub pregap_start_lba: u32,  // Disc LBA where INDEX 00 starts (== start_lba if no pregap)
    pub start_lba: u32,         // Disc LBA where INDEX 01 starts (reported in TOC)
    pub length_sectors: u32,    // Sectors from INDEX 01 to end of track
    pub source_start_lba: u32,  // Disc LBA where stored sectors begin (> pregap_start_lba for virtual zero PREGAP)
    pub stored_sectors: u32,    // Number of sectors physically present in backing image/CHD
    pub total_sectors: u32,     // Total disc LBA span covered by this track (pregap + stored + postgap)
    pub sector_size: usize,
    pub file_index: usize,
    pub file_offset: u64,
    pub chd_unit_start: u32,    // Physical CHD unit index of stored sector 0 (tracks padded to CD_TRACK_PADDING)
    pub chd_audio_swap: bool,   // CHD TYPE:AUDIO / VAUDIO frames are stored in big-endian 16-bit order
}

impl CdTrack {
    pub fn contains_lba(&self, lba: u32) -> bool {
        lba >= self.pregap_start_lba && lba < self.pregap_start_lba + self.total_sectors
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
pub struct Win32CdHandle(pub *mut std::ffi::c_void);

#[cfg(target_os = "windows")]
impl Drop for Win32CdHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != (-1isize as *mut std::ffi::c_void) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
#[cfg(target_os = "windows")]
unsafe impl Send for Win32CdHandle {}
#[cfg(target_os = "windows")]
unsafe impl Sync for Win32CdHandle {}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        lpFileName: *const u16,
        dwDesiredAccess: u32,
        dwShareMode: u32,
        lpSecurityAttributes: *mut std::ffi::c_void,
        dwCreationDisposition: u32,
        dwFlagsAndAttributes: u32,
        hTemplateFile: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;

    fn DeviceIoControl(
        hDevice: *mut std::ffi::c_void,
        dwIoControlCode: u32,
        lpInBuffer: *const std::ffi::c_void,
        nInBufferSize: u32,
        lpOutBuffer: *mut std::ffi::c_void,
        nOutBufferSize: u32,
        lpBytesReturned: *mut u32,
        lpOverlapped: *mut std::ffi::c_void,
    ) -> i32;

    fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;

    fn GetLastError() -> u32;

    fn SetFilePointerEx(
        hFile: *mut std::ffi::c_void,
        liDistanceToMove: i64,
        lpNewFilePointer: *mut i64,
        dwMoveMethod: u32,
    ) -> i32;

    fn ReadFile(
        hFile: *mut std::ffi::c_void,
        lpBuffer: *mut std::ffi::c_void,
        nNumberOfBytesToRead: u32,
        lpNumberOfBytesRead: *mut u32,
        lpOverlapped: *mut std::ffi::c_void,
    ) -> i32;
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TrackData {
    pub reserved: u8,
    pub control_adr: u8,
    pub track_number: u8,
    pub reserved1: u8,
    pub address: [u8; 4],
}

#[cfg(target_os = "windows")]
#[repr(C)]
pub struct CdromToc {
    pub length: [u8; 2],
    pub first_track: u8,
    pub last_track: u8,
    pub track_data: [TrackData; 100],
}

#[cfg(target_os = "windows")]
#[repr(C)]
pub struct RawReadInfo {
    pub disk_offset: i64,
    pub sector_count: u32,
    pub track_mode: u32,
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

pub fn parse_drive_letter<P: AsRef<Path>>(path: P) -> Option<char> {
    let s = path.as_ref().to_str()?;
    let s_clean = s.trim();
    if s_clean.starts_with(r"\\.\") && s_clean.len() >= 6 {
        let ch = s_clean.chars().nth(4)?;
        if ch.is_ascii_alphabetic() && s_clean.chars().nth(5) == Some(':') {
            return Some(ch.to_ascii_uppercase());
        }
    }
    let chars: Vec<char> = s_clean.chars().collect();
    if (chars.len() == 2 || chars.len() == 3) && chars[0].is_ascii_alphabetic() && chars[1] == ':' {
        if chars.len() == 2 || chars[2] == '\\' || chars[2] == '/' {
            return Some(chars[0].to_ascii_uppercase());
        }
    }
    None
}

pub enum CdSource {
    Chd {
        chd: Chd<File>,
        hunk_buf: Vec<u8>,
        comp_buf: Vec<u8>,
        current_hunk: Option<u32>,
        hunk_size: usize,
        sectors_per_hunk: u32,
    },
    RawFiles {
        files: Vec<File>,
    },
    #[cfg(target_os = "windows")]
    PhysicalDrive {
        handle: Win32CdHandle,
        drive_letter: char,
    },
}

pub struct CdRom {
    source: CdSource,
    pub total_sectors: u32,
    pub tracks: Vec<CdTrack>,
    pub disc_type: u8, // 0x10 = CD-i, 0x20 = CD-ROM XA / CD-i Bridge / Video CD, 0x00 = CD-DA
    pub volume_id: String,
    pub volume_set: String,
    pub publisher: String,
    pub preparer: String,
    pub disc_ntsc_hint: Option<bool>,
}

const CD_SWAPPED_SYNC_HEADER: [u8; 12] = [0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0x00, 0xFF];

fn read_probe(file: &mut File, offset: u64, buf: &mut [u8]) -> bool {
    file.seek(SeekFrom::Start(offset)).is_ok() && file.read_exact(buf).is_ok()
}

fn is_sync_header(hdr: &[u8]) -> bool {
    hdr == CD_SYNC_HEADER || hdr == CD_SWAPPED_SYNC_HEADER
}

fn has_pvd_signature(sig: &[u8]) -> bool {
    sig == b"\x01CD-I " || sig == b"\x01CD001"
}

/// Auto-detects the sector stride (2352, 2448, 2336, or 2048 bytes) of single-file `.iso`/`.bin`/`.img` dumps.
pub(crate) fn detect_raw_sector_size(file: &mut File, file_size: u64, ext: &str) -> usize {
    let mut hdr0 = [0u8; 12];
    let mut hdr16 = [0u8; 12];
    for &stride in &[CD_SECTOR_RAW_SIZE, CD_FRAME_SIZE] {
        let s = stride as u64;
        if read_probe(file, 0, &mut hdr0) && is_sync_header(&hdr0) {
            if file_size < 17 * s || (read_probe(file, 16 * s, &mut hdr16) && is_sync_header(&hdr16)) {
                let _ = file.seek(SeekFrom::Start(0));
                return stride;
            }
        }
    }

    let mut sig = [0u8; 6];
    for &(stride, data_off) in &[
        (CD_SECTOR_RAW_SIZE, 24u64),
        (CD_SECTOR_RAW_SIZE, 16u64),
        (CD_FRAME_SIZE, 24u64),
        (CD_FRAME_SIZE, 16u64),
        (2336usize, 8u64),
        (2048usize, 0u64),
    ] {
        if read_probe(file, 16 * (stride as u64) + data_off, &mut sig) && has_pvd_signature(&sig) {
            let _ = file.seek(SeekFrom::Start(0));
            return stride;
        }
    }

    let _ = file.seek(SeekFrom::Start(0));
    if ext == "iso" {
        if file_size > 0 && file_size % (CD_SECTOR_RAW_SIZE as u64) == 0 && file_size % 2048 != 0 {
            CD_SECTOR_RAW_SIZE
        } else if file_size > 0 && file_size % 2336 == 0 && file_size % 2048 != 0 {
            2336
        } else if file_size % 2048 == 0 {
            2048
        } else {
            CD_SECTOR_RAW_SIZE
        }
    } else if file_size > 0 && file_size % 2048 == 0 && file_size % (CD_SECTOR_RAW_SIZE as u64) != 0 {
        2048
    } else {
        CD_SECTOR_RAW_SIZE
    }
}

fn synthesize_mode2_sync_and_msf(out_sector: &mut [u8; CD_SECTOR_RAW_SIZE], lba: u32) {
    out_sector[0..12].copy_from_slice(&CD_SYNC_HEADER);
    let total_f = lba + 150;
    let m = total_f / 75 / 60;
    let s = (total_f / 75) % 60;
    let f = total_f % 75;
    out_sector[12] = (((m / 10) << 4) | (m % 10)) as u8;
    out_sector[13] = (((s / 10) << 4) | (s % 10)) as u8;
    out_sector[14] = (((f / 10) << 4) | (f % 10)) as u8;
    out_sector[15] = 2; // Mode 2
}

fn synthesize_cooked_mode2_form1_header(out_sector: &mut [u8; CD_SECTOR_RAW_SIZE], lba: u32) {
    synthesize_mode2_sync_and_msf(out_sector, lba);
    // CDIC::is_mode2_sector_selected reads out_sector[20..24] (second subheader copy) and requires
    // (submode & (0x40 | 0x20 | 0x08 | 0x04 | 0x02)) != 0. Mode 2 Form 1 Data|EOR = 0x09 (0x89 with EOF on VD terminator).
    let is_vd_term = out_sector[24] == 0xFF
        && (&out_sector[25..30] == b"CD-I " || &out_sector[25..30] == b"CD001");
    let submode = if is_vd_term { 0x89 } else { 0x09 };
    let subhdr = [0x00, 0x00, submode, 0x00];
    out_sector[16..20].copy_from_slice(&subhdr);
    out_sector[20..24].copy_from_slice(&subhdr);
}

fn parse_msf(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let m: u32 = parts[0].trim().parse().ok()?;
    let s: u32 = parts[1].trim().parse().ok()?;
    let f: u32 = parts[2].trim().parse().ok()?;
    Some(m * 60 * 75 + s * 75 + f)
}

#[derive(Clone, Debug)]
struct CueTrackDef {
    num: u8,
    control: u8,
    sector_size: usize,
    file_index: usize,
    pregap_sectors: u32,
    postgap_sectors: u32,
    idx00: Option<u32>,
    idx01: u32,
    has_idx01: bool,
}

fn parse_cue_sheet(content: &str) -> (Vec<String>, Vec<CueTrackDef>) {
    let mut file_names: Vec<String> = Vec::new();
    let mut parsed_tracks: Vec<CueTrackDef> = Vec::new();
    let mut current_file_idx: Option<usize> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("REM") {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        if parts[0].eq_ignore_ascii_case("FILE") {
            let fname = if let (Some(start), Some(end)) = (trimmed.find('"'), trimmed.rfind('"')) {
                if end > start { Some(&trimmed[start + 1..end]) } else { None }
            } else {
                parts.get(1).copied()
            };
            if let Some(name) = fname {
                file_names.push(name.to_string());
                current_file_idx = Some(file_names.len() - 1);
            }
        } else if parts[0].eq_ignore_ascii_case("TRACK") {
            if parts.len() >= 3 {
                let tnum: u8 = parts[1].parse().unwrap_or(1);
                let mode = parts[2].to_uppercase();
                let control = if mode.starts_with("AUDIO") { 0x01 } else { 0x41 };
                let sector_size = if mode.contains("2048") {
                    2048
                } else if mode.contains("2336") {
                    2336
                } else if mode.contains("2448") {
                    2448
                } else {
                    CD_SECTOR_RAW_SIZE
                };
                let fidx = current_file_idx.unwrap_or(0);
                parsed_tracks.push(CueTrackDef {
                    num: tnum,
                    control,
                    sector_size,
                    file_index: fidx,
                    pregap_sectors: 0,
                    postgap_sectors: 0,
                    idx00: None,
                    idx01: 0,
                    has_idx01: false,
                });
            }
        } else if parts[0].eq_ignore_ascii_case("PREGAP") {
            if let (Some(t), Some(sectors)) = (parsed_tracks.last_mut(), parts.get(1).and_then(|s| parse_msf(s))) {
                t.pregap_sectors = sectors;
            }
        } else if parts[0].eq_ignore_ascii_case("POSTGAP") {
            if let (Some(t), Some(sectors)) = (parsed_tracks.last_mut(), parts.get(1).and_then(|s| parse_msf(s))) {
                t.postgap_sectors = sectors;
            }
        } else if parts[0].eq_ignore_ascii_case("INDEX") {
            if parts.len() >= 3 {
                if let (Some(t), Some(sectors)) = (parsed_tracks.last_mut(), parse_msf(parts[2])) {
                    if parts[1] == "00" || parts[1] == "0" {
                        t.idx00 = Some(sectors);
                    } else if parts[1] == "01" || parts[1] == "1" {
                        t.idx01 = sectors;
                        t.has_idx01 = true;
                    }
                }
            }
        }
    }

    for t in &mut parsed_tracks {
        if !t.has_idx01 {
            if let Some(i00) = t.idx00 {
                t.idx01 = i00;
            }
        }
    }

    (file_names, parsed_tracks)
}

fn build_cue_tracks(parsed_tracks: &[CueTrackDef], file_sizes: &[u64]) -> (Vec<CdTrack>, u32) {
    let mut tracks: Vec<CdTrack> = Vec::with_capacity(parsed_tracks.len());
    let mut current_disc_lba = 0u32;
    let mut current_file_idx: Option<usize> = None;
    let mut file_base_disc_lba = 0u32;
    let mut file_synth_gap = 0u32;

    for (i, pt) in parsed_tracks.iter().enumerate() {
        let fsize = file_sizes.get(pt.file_index).copied().unwrap_or(0);
        let file_sectors = (fsize / pt.sector_size as u64) as u32;
        let start_in_file = pt.idx00.unwrap_or(pt.idx01).min(file_sectors);

        if current_file_idx != Some(pt.file_index) {
            current_file_idx = Some(pt.file_index);
            file_base_disc_lba = current_disc_lba;
            file_synth_gap = 0;
        }

        let end_in_file = if i + 1 < parsed_tracks.len() && parsed_tracks[i + 1].file_index == pt.file_index {
            let next = &parsed_tracks[i + 1];
            next.idx00.unwrap_or(next.idx01).min(file_sectors)
        } else {
            file_sectors
        };

        let stored_sectors = end_in_file.saturating_sub(start_in_file);
        let pregap_start_lba = current_disc_lba;
        file_synth_gap += pt.pregap_sectors;
        let source_start_lba = pregap_start_lba + pt.pregap_sectors;
        let start_lba = file_base_disc_lba + file_synth_gap + pt.idx01;
        let track_end_lba = source_start_lba + stored_sectors + pt.postgap_sectors;
        let total_sectors = track_end_lba.saturating_sub(pregap_start_lba);
        let length_sectors = track_end_lba.saturating_sub(start_lba);

        tracks.push(CdTrack {
            number: pt.num,
            control: pt.control,
            pregap_start_lba,
            start_lba,
            length_sectors,
            source_start_lba,
            stored_sectors,
            total_sectors,
            sector_size: pt.sector_size,
            file_index: pt.file_index,
            file_offset: (start_in_file as u64) * (pt.sector_size as u64),
            chd_unit_start: 0,
            chd_audio_swap: false,
        });

        file_synth_gap += pt.postgap_sectors;
        current_disc_lba = track_end_lba;
    }

    (tracks, current_disc_lba)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChdTrackMeta {
    num: u8,
    track_type: String,
    frames: u32,
    pregap: u32,
    pregap_in_source: bool,
    postgap: u32,
}

pub(crate) fn parse_chd_track_entry(s: &str) -> Option<ChdTrackMeta> {
    let clean = s.trim_matches(|c: char| c == '\0' || c.is_whitespace() || c == '.');
    let mut num: Option<u8> = None;
    let mut track_type = String::new();
    let mut frames: u32 = 0;
    let mut pregap: u32 = 0;
    let mut pgtype = String::new();
    let mut postgap: u32 = 0;

    for token in clean.split_whitespace() {
        if let Some((key, val)) = token.split_once(':') {
            match key {
                "TRACK" => num = val.parse().ok(),
                "TYPE" => track_type = val.to_uppercase(),
                "FRAMES" => frames = val.parse().unwrap_or(0),
                "PREGAP" => pregap = val.parse().unwrap_or(0),
                "PGTYPE" => pgtype = val.to_uppercase(),
                "POSTGAP" => postgap = val.parse().unwrap_or(0),
                _ => {}
            }
        }
    }

    let num = num?;
    Some(ChdTrackMeta {
        num,
        track_type,
        frames,
        pregap,
        pregap_in_source: pgtype.starts_with('V'),
        postgap,
    })
}

pub(crate) fn build_chd_tracks(entries: &[ChdTrackMeta]) -> (Vec<CdTrack>, u32) {
    let mut tracks = Vec::with_capacity(entries.len());
    let mut current_disc_lba = 0u32;
    let mut current_chd_unit = 0u32;

    for m in entries {
        let is_audio = m.track_type == "AUDIO";
        let control = if is_audio { 0x01 } else { 0x41 };
        let pregap_start_lba = current_disc_lba;
        let source_start_lba = if m.pregap_in_source {
            current_disc_lba
        } else {
            current_disc_lba + m.pregap
        };
        let start_lba = current_disc_lba + m.pregap;
        let stored_sectors = m.frames;
        let track_end_lba = source_start_lba + stored_sectors + m.postgap;
        let total_sectors = track_end_lba.saturating_sub(pregap_start_lba);
        let length_sectors = track_end_lba.saturating_sub(start_lba);

        tracks.push(CdTrack {
            number: m.num,
            control,
            pregap_start_lba,
            start_lba,
            length_sectors,
            source_start_lba,
            stored_sectors,
            total_sectors,
            sector_size: CD_SECTOR_RAW_SIZE,
            file_index: 0,
            file_offset: 0,
            chd_unit_start: current_chd_unit,
            chd_audio_swap: is_audio,
        });

        current_disc_lba = track_end_lba;
        let padded_frames = (m.frames + (CD_TRACK_PADDING - 1)) & !(CD_TRACK_PADDING - 1);
        current_chd_unit += padded_frames;
    }

    (tracks, current_disc_lba)
}

impl CdRom {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let path_ref = path.as_ref();

        if let Some(drive_letter) = parse_drive_letter(path_ref) {
            #[cfg(target_os = "windows")]
            {
                return Self::open_physical_drive(drive_letter);
            }
            #[cfg(not(target_os = "windows"))]
            {
                return Err(format!("Unità CD-ROM fisiche non supportate su questo sistema operativo ({}:)", drive_letter).into());
            }
        }

        let ext = path_ref.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

        if ext == "cue" {
            let content = std::fs::read_to_string(path_ref)?;
            let (file_names, parsed_tracks) = parse_cue_sheet(&content);

            let mut files: Vec<File> = Vec::with_capacity(file_names.len());
            let mut file_sizes: Vec<u64> = Vec::with_capacity(file_names.len());
            for fname in &file_names {
                let fpath = path_ref.parent().map(|p| p.join(fname)).unwrap_or_else(|| PathBuf::from(fname));
                let file = File::open(&fpath)?;
                let fsize = file.metadata()?.len();
                files.push(file);
                file_sizes.push(fsize);
            }

            if files.is_empty() {
                return Err(format!("No files found in CUE sheet {:?}", path_ref).into());
            }

            let (mut tracks, mut total_sectors) = build_cue_tracks(&parsed_tracks, &file_sizes);

            if tracks.is_empty() {
                let sec_size = CD_SECTOR_RAW_SIZE;
                total_sectors = (file_sizes[0] / sec_size as u64) as u32;
                tracks.push(CdTrack {
                    number: 1,
                    control: 0x41,
                    pregap_start_lba: 0,
                    start_lba: 0,
                    length_sectors: total_sectors,
                    source_start_lba: 0,
                    stored_sectors: total_sectors,
                    total_sectors,
                    sector_size: sec_size,
                    file_index: 0,
                    file_offset: 0,
                    chd_unit_start: 0,
                    chd_audio_swap: false,
                });
            }

            info!(
                "Opened CUE {:?}: {} file(s), {} track(s), total_sectors={}",
                path_ref,
                files.len(),
                tracks.len(),
                total_sectors
            );
            for t in &tracks {
                info!(
                    "  Track {:02}: span {}..{} (idx01={}, len={}) ctrl=0x{:02X} file_idx={}",
                    t.number,
                    t.pregap_start_lba,
                    t.pregap_start_lba + t.total_sectors.saturating_sub(1),
                    t.start_lba,
                    t.length_sectors,
                    t.control,
                    t.file_index
                );
            }

            let mut cdrom = Self {
                source: CdSource::RawFiles { files },
                total_sectors,
                tracks,
                disc_type: 0x10,
                volume_id: String::new(),
                volume_set: String::new(),
                publisher: String::new(),
                preparer: String::new(),
                disc_ntsc_hint: None,
            };
            cdrom.detect_disc_type();
            return Ok(cdrom);
        }

        if ext == "chd" {
            let file = File::open(path_ref)?;
            let mut chd = Chd::open(file, None)?;
            let header = chd.header();

            let hunk_size = header.hunk_size() as usize;
            let sectors_per_hunk = if hunk_size % CD_FRAME_SIZE == 0 {
                (hunk_size / CD_FRAME_SIZE) as u32
            } else if hunk_size % CD_SECTOR_RAW_SIZE == 0 {
                (hunk_size / CD_SECTOR_RAW_SIZE) as u32
            } else {
                8
            };

            let raw_total_units = header.hunk_count() * sectors_per_hunk;
            let hunk_buf = chd.get_hunksized_buffer();

            let mut chd_metas: Vec<ChdTrackMeta> = Vec::new();
            if let Ok(metadatas) = Vec::<chd::metadata::Metadata>::try_from(chd.metadata_refs()) {
                for meta in metadatas {
                    let tag = meta.metatag;
                    if tag == chd::metadata::KnownMetadata::CdRomTrack2 as u32
                        || tag == chd::metadata::KnownMetadata::CdRomTrack as u32
                        || tag == chd::metadata::KnownMetadata::GdRomTrack as u32
                    {
                        if let Ok(s) = std::str::from_utf8(&meta.value) {
                            if let Some(parsed) = parse_chd_track_entry(s) {
                                chd_metas.push(parsed);
                            }
                        }
                    }
                }
            }

            let (tracks, total_sectors) = if !chd_metas.is_empty() {
                build_chd_tracks(&chd_metas)
            } else {
                let tracks = vec![CdTrack {
                    number: 1,
                    control: 0x41,
                    pregap_start_lba: 0,
                    start_lba: 0,
                    length_sectors: raw_total_units,
                    source_start_lba: 0,
                    stored_sectors: raw_total_units,
                    total_sectors: raw_total_units,
                    sector_size: CD_SECTOR_RAW_SIZE,
                    file_index: 0,
                    file_offset: 0,
                    chd_unit_start: 0,
                    chd_audio_swap: false,
                }];
                (tracks, raw_total_units)
            };

            info!(
                "Opened CHD: {:?}, hunk_size={}, sectors_per_hunk={}, tracks={}, total_sectors={}",
                path_ref, hunk_size, sectors_per_hunk, tracks.len(), total_sectors
            );
            for t in &tracks {
                info!(
                    "  CHD Track {:02}: span {}..{} (idx01={}, len={}) unit_start={} ctrl=0x{:02X} swap={}",
                    t.number,
                    t.pregap_start_lba,
                    t.pregap_start_lba + t.total_sectors.saturating_sub(1),
                    t.start_lba,
                    t.length_sectors,
                    t.chd_unit_start,
                    t.control,
                    t.chd_audio_swap
                );
            }

            let mut cdrom = Self {
                source: CdSource::Chd {
                    chd,
                    hunk_buf,
                    comp_buf: Vec::new(),
                    current_hunk: None,
                    hunk_size,
                    sectors_per_hunk,
                },
                total_sectors,
                tracks,
                disc_type: 0x10,
                volume_id: String::new(),
                volume_set: String::new(),
                publisher: String::new(),
                preparer: String::new(),
                disc_ntsc_hint: None,
            };
            cdrom.detect_disc_type();
            Ok(cdrom)
        } else {
            let mut file = File::open(path_ref)?;
            let file_size = file.metadata()?.len();
            let sector_size = detect_raw_sector_size(&mut file, file_size, &ext);
            let total_sectors = (file_size / sector_size as u64) as u32;

            info!(
                "Opened raw disc image: {:?}, sector_size={}, total_sectors={}",
                path_ref, sector_size, total_sectors
            );

            let tracks = vec![CdTrack {
                number: 1,
                control: 0x41,
                pregap_start_lba: 0,
                start_lba: 0,
                length_sectors: total_sectors,
                source_start_lba: 0,
                stored_sectors: total_sectors,
                total_sectors,
                sector_size,
                file_index: 0,
                file_offset: 0,
                chd_unit_start: 0,
                chd_audio_swap: false,
            }];

            let mut cdrom = Self {
                source: CdSource::RawFiles { files: vec![file] },
                total_sectors,
                tracks,
                disc_type: 0x10,
                volume_id: String::new(),
                volume_set: String::new(),
                publisher: String::new(),
                preparer: String::new(),
                disc_ntsc_hint: None,
            };
            cdrom.detect_disc_type();
            Ok(cdrom)
        }
    }

    #[cfg(target_os = "windows")]
    pub fn open_physical_drive(drive_letter: char) -> Result<Self, Box<dyn std::error::Error>> {
        let device_path = format!(r"\\.\{}:", drive_letter);
        let wide_path = to_wide(&device_path);

        unsafe {
            let handle = CreateFileW(
                wide_path.as_ptr(),
                0x8000_0000, // GENERIC_READ
                0x0000_0001 | 0x0000_0002, // FILE_SHARE_READ | FILE_SHARE_WRITE
                std::ptr::null_mut(),
                3, // OPEN_EXISTING
                0,
                std::ptr::null_mut(),
            );

            if handle.is_null() || handle == (-1isize as *mut std::ffi::c_void) {
                let err = GetLastError();
                return Err(format!(
                    "Impossibile accedere all'unità ottica fisica '{}' (Errore Win32 {})",
                    device_path, err
                ).into());
            }

            let cd_handle = Win32CdHandle(handle);

            // Read TOC via IOCTL_CDROM_READ_TOC
            let mut toc: CdromToc = std::mem::zeroed();
            let mut bytes_ret = 0u32;
            let ok = DeviceIoControl(
                cd_handle.0,
                0x0002_4000, // IOCTL_CDROM_READ_TOC
                std::ptr::null(),
                0,
                &mut toc as *mut _ as *mut std::ffi::c_void,
                std::mem::size_of::<CdromToc>() as u32,
                &mut bytes_ret,
                std::ptr::null_mut(),
            );

            if ok == 0 {
                let err = GetLastError();
                if err == 21 { // ERROR_NOT_READY
                    return Err(format!(
                        "Nessun disco presente nell'unità {}:. Inserire un disco CD-i, Video CD o CD Audio e riprovare.",
                        drive_letter
                    ).into());
                }
                return Err(format!(
                    "Errore nella lettura del disco nell'unità {}: (IOCTL_CDROM_READ_TOC fallito, errore {})",
                    drive_letter, err
                ).into());
            }

            let first_track = toc.first_track;
            let last_track = toc.last_track;
            info!(
                "CD-ROM Fisico (Unità {}:): TOC letta con successo. Tracce: {}..{}",
                drive_letter, first_track, last_track
            );

            let mut tracks: Vec<CdTrack> = Vec::new();
            let num_entries = if last_track >= first_track {
                (last_track - first_track + 1) as usize
            } else {
                1
            };

            // Find lead-out track (TrackNumber 0xAA) to determine total disc sectors
            let mut total_sectors = 0u32;
            for i in 0..100 {
                let td = &toc.track_data[i];
                if td.track_number == 0xAA {
                    let m = td.address[1] as u32;
                    let s = td.address[2] as u32;
                    let f = td.address[3] as u32;
                    let leadout_lba = (m * 60 + s) * 75 + f;
                    total_sectors = leadout_lba.saturating_sub(150);
                    break;
                }
            }

            for i in 0..num_entries.min(100) {
                let td = &toc.track_data[i];
                let next_td = &toc.track_data[i + 1];

                let start_m = td.address[1] as u32;
                let start_s = td.address[2] as u32;
                let start_f = td.address[3] as u32;
                let start_lba = ((start_m * 60 + start_s) * 75 + start_f).saturating_sub(150);

                let next_m = next_td.address[1] as u32;
                let next_s = next_td.address[2] as u32;
                let next_f = next_td.address[3] as u32;
                let next_lba = ((next_m * 60 + next_s) * 75 + next_f).saturating_sub(150);

                let length_sectors = if next_lba > start_lba {
                    next_lba - start_lba
                } else if total_sectors > start_lba {
                    total_sectors - start_lba
                } else {
                    75 * 60 // fallback: 1 minute
                };

                let is_data = (td.control_adr & 0x04) != 0 || (td.control_adr & 0x40) != 0;
                let control = if is_data { 0x41 } else { 0x01 };
                let pregap_start_lba = if i == 0 { 0 } else { start_lba };
                let track_total = (start_lba + length_sectors).saturating_sub(pregap_start_lba);

                tracks.push(CdTrack {
                    number: td.track_number,
                    control,
                    pregap_start_lba,
                    start_lba,
                    length_sectors,
                    source_start_lba: pregap_start_lba,
                    stored_sectors: track_total,
                    total_sectors: track_total,
                    sector_size: CD_SECTOR_RAW_SIZE,
                    file_index: 0,
                    file_offset: 0,
                    chd_unit_start: 0,
                    chd_audio_swap: false,
                });

                info!(
                    "  Traccia {:02}: LBA {}..{} ({} settori) ctrl=0x{:02X} ({})",
                    td.track_number,
                    start_lba,
                    start_lba + length_sectors.saturating_sub(1),
                    length_sectors,
                    control,
                    if is_data { "DATI" } else { "AUDIO CD-DA" }
                );
            }

            if total_sectors == 0 {
                total_sectors = tracks.iter().map(|t| t.start_lba + t.length_sectors).max().unwrap_or(0);
            }

            let mut cdrom = Self {
                source: CdSource::PhysicalDrive {
                    handle: cd_handle,
                    drive_letter,
                },
                total_sectors,
                tracks,
                disc_type: 0x10,
                volume_id: String::new(),
                volume_set: String::new(),
                publisher: String::new(),
                preparer: String::new(),
                disc_ntsc_hint: None,
            };

            cdrom.detect_disc_type();
            Ok(cdrom)
        }
    }

    pub fn track_at_lba(&self, lba: u32) -> Option<&CdTrack> {
        self.tracks.iter().find(|t| t.contains_lba(lba))
    }

    pub fn is_cdi_ready_layout(tracks: &[CdTrack]) -> bool {
        tracks
            .first()
            .is_some_and(|t| (t.control & 0x40) == 0 && t.start_lba > t.pregap_start_lba)
    }

    #[cfg(test)]
    pub(crate) fn from_tracks_for_test(tracks: Vec<CdTrack>, total_sectors: u32, disc_type: u8) -> Self {
        Self {
            source: CdSource::RawFiles { files: Vec::new() },
            total_sectors,
            tracks,
            disc_type,
            volume_id: String::new(),
            volume_set: String::new(),
            publisher: String::new(),
            preparer: String::new(),
            disc_ntsc_hint: None,
        }
    }

    fn detect_disc_type(&mut self) {
        let is_cdi_ready = Self::is_cdi_ready_layout(&self.tracks);

        let mut s16 = [0u8; CD_SECTOR_RAW_SIZE];
        if self.read_sector(16, &mut s16) {
            if s16[0] == 0xFF && s16[1] == 0x00 && s16[2] == 0xFF && s16[10] == 0x00 && s16[11] == 0xFF {
                for pair in s16.chunks_exact_mut(2) {
                    pair.swap(0, 1);
                }
                maybe_descramble_sector(&mut s16);
            }
            let trim_pvd = |s: &[u8]| {
                String::from_utf8_lossy(s)
                    .trim_matches(|c: char| c == '\0' || c.is_whitespace())
                    .to_string()
            };
            let sys_id = &s16[24 + 8..24 + 40];
            self.volume_id = trim_pvd(&s16[24 + 40..24 + 72]);
            self.volume_set = trim_pvd(&s16[24 + 190..24 + 318]);
            self.publisher = trim_pvd(&s16[24 + 318..24 + 446]);
            self.preparer = trim_pvd(&s16[24 + 446..24 + 574]);
            if sys_id.starts_with(b"CD-RTOS CD-BRIDGE") {
                info!("CD-ROM: Riconosciuto disco CD-RTOS CD-BRIDGE (Video CD / White Book), disc_type = 0x20");
                self.disc_type = 0x20;
            } else if is_cdi_ready {
                // Green Book Ch. VIII (CD-i Ready): Track 1 is Red Book Audio with CD-i data in INDEX 00 pregap,
                // so TOC packet A0 PSEC must be 0x00 (CD-DA) rather than 0x10.
                info!("CD-ROM: Riconosciuto disco CD-i Ready (Red Book + pregap CD-RTOS), disc_type = 0x00, volume = {:?}", self.volume_id);
                self.disc_type = 0x00;
            } else if sys_id.starts_with(b"CD-RTOS") || sys_id.starts_with(b"CD-I") {
                info!("CD-ROM: Riconosciuto disco CD-i standard, disc_type = 0x10, volume = {:?}", self.volume_id);
                self.disc_type = 0x10;
            } else {
                info!("CD-ROM: Disc system ID: {:?}, disc_type = 0x10, volume = {:?}", String::from_utf8_lossy(sys_id).trim(), self.volume_id);
                self.disc_type = 0x10;
            }
            self.disc_ntsc_hint = self.probe_mpeg_ntsc().or_else(|| self.probe_pvd_ntsc());
        } else {
            let only_audio = self.tracks.iter().all(|t| (t.control & 0x40) == 0);
            if only_audio && !self.tracks.is_empty() {
                info!("CD-ROM: Riconosciuto disco CD-DA Audio Red Book, disc_type = 0x00");
                self.disc_type = 0x00;
            }
            self.disc_ntsc_hint = None;
        }
    }

    fn probe_mpeg_ntsc(&mut self) -> Option<bool> {
        let mut ranges: [(u32, u32); 2] = [(0, 0), (18, 320.min(self.total_sectors))];
        if self.disc_type == 0x20 && self.tracks.len() >= 2 {
            let s = self.tracks[1].start_lba;
            ranges[0] = (s, s.saturating_add(90).min(self.total_sectors));
        }
        let mut sec = [0u8; CD_SECTOR_RAW_SIZE];
        for (start, end) in ranges {
            if start >= end {
                continue;
            }
            for lba in start..end {
                let is_audio_track = self.track_at_lba(lba).is_some_and(|t| (t.control & 0x40) == 0);
                if is_audio_track && self.disc_type != 0x00 {
                    continue;
                }
                if !self.read_sector(lba, &mut sec) {
                    continue;
                }
                let submode = sec[18] | sec[22];
                if self.disc_type != 0x20 && (submode & 0x62) == 0 {
                    continue;
                }
                for w in sec[24..2348].windows(8) {
                    if w[0] == 0x00 && w[1] == 0x00 && w[2] == 0x01 && w[3] == 0xB3 {
                        let width = ((w[4] as u16) << 4) | ((w[5] as u16) >> 4);
                        let height = (((w[5] & 0x0F) as u16) << 8) | (w[6] as u16);
                        let par = w[7] >> 4;
                        let frc = w[7] & 0x0F;
                        if (160..=768).contains(&width)
                            && (120..=576).contains(&height)
                            && (1..=14).contains(&par)
                            && (1..=8).contains(&frc)
                        {
                            if frc == 3 || height == 288 || height == 576 {
                                return Some(false);
                            }
                            if matches!(frc, 1 | 4 | 5) || height == 240 || height == 480 {
                                return Some(true);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn probe_pvd_ntsc(&self) -> Option<bool> {
        let meta = format!("{} {}", self.publisher.to_uppercase(), self.preparer.to_uppercase());
        if ["PHILIPS POV", "PIMA", "INTERACTIVE MEDIA OF AMERICA", "AMERICAN INTERACTIVE", "LOS ANGELES"]
            .iter()
            .any(|tag| meta.contains(tag))
        {
            return Some(true);
        }
        if ["PIMI", "PIME", "PIMF", "INTERACTIVE MEDIA INTERNATIONAL", "US GOLD LTD", "INFOGRAMES"]
            .iter()
            .any(|tag| meta.contains(tag))
        {
            return Some(false);
        }
        None
    }

    /// Evaluates whether this disc should run in NTSC (60 Hz) mode based on filename and on-disc metadata.
    pub fn is_ntsc_hint(&self, disc_name: &str) -> Option<bool> {
        let name_upper = disc_name.to_uppercase();
        if self.disc_type == 0x20 {
            if let Some(ntsc) = self.disc_ntsc_hint {
                return Some(ntsc);
            }
            if name_upper.contains("NTSC") {
                return Some(true);
            }
            if name_upper.contains("(PAL)") {
                return Some(false);
            }
            return None;
        }
        if name_upper.contains("(USA)")
            || name_upper.contains("[USA]")
            || name_upper.contains("(JAPAN)")
            || name_upper.contains("(JPN)")
            || name_upper.contains("(US)")
            || name_upper.contains("NTSC")
        {
            return Some(true);
        }
        if name_upper.contains("(EUROPE)")
            || name_upper.contains("(EU)")
            || name_upper.contains("[EU]")
            || name_upper.contains("(EUR)")
            || name_upper.contains("(PAL)")
            || name_upper.contains("(GERMANY)")
            || name_upper.contains("(FRANCE)")
            || name_upper.contains("(UK)")
            || name_upper.contains("(ITALY)")
            || name_upper.contains("(SPAIN)")
            || name_upper.contains("(NETHERLANDS)")
        {
            return Some(false);
        }
        let vol_upper = self.volume_id.to_uppercase();
        if vol_upper.contains("EARTH COMMAND") {
            return Some(true);
        }
        self.disc_ntsc_hint
    }

    pub fn read_sector(&mut self, lba: u32, out_sector: &mut [u8; CD_SECTOR_RAW_SIZE]) -> bool {
        if lba >= self.total_sectors {
            error!("CD-ROM: Sector {} out of bounds (max {})", lba, self.total_sectors);
            return false;
        }

        let matched_track = self.tracks.iter().find(|t| t.contains_lba(lba)).cloned();

        let success = match &mut self.source {
            CdSource::Chd { chd, hunk_buf, comp_buf, current_hunk, hunk_size, sectors_per_hunk } => {
                let (chd_unit, is_virtual_silence, is_chd_audio) = if let Some(ref tr) = matched_track {
                    if lba < tr.source_start_lba || lba >= tr.source_start_lba + tr.stored_sectors {
                        (0, true, false)
                    } else {
                        (tr.chd_unit_start + (lba - tr.source_start_lba), false, tr.chd_audio_swap)
                    }
                } else {
                    (lba, false, false)
                };

                if is_virtual_silence {
                    out_sector.fill(0);
                    return true;
                }

                let hunk_idx = chd_unit / *sectors_per_hunk;
                let sector_in_hunk = (chd_unit % *sectors_per_hunk) as usize;

                if *current_hunk != Some(hunk_idx) {
                    match chd.hunk(hunk_idx) {
                        Ok(mut hunk) => {
                            if let Err(e) = hunk.read_hunk_in(comp_buf, hunk_buf) {
                                error!("CD-ROM: Error reading hunk {}: {:?}", hunk_idx, e);
                                return false;
                            }
                            *current_hunk = Some(hunk_idx);
                        }
                        Err(e) => {
                            error!("CD-ROM: Failed to get hunk {}: {:?}", hunk_idx, e);
                            return false;
                        }
                    }
                }

                let frame_stride = if *hunk_size % CD_FRAME_SIZE == 0 {
                    CD_FRAME_SIZE
                } else {
                    CD_SECTOR_RAW_SIZE
                };

                let start = sector_in_hunk * frame_stride;
                if start + CD_SECTOR_RAW_SIZE <= hunk_buf.len() {
                    out_sector.copy_from_slice(&hunk_buf[start..start + CD_SECTOR_RAW_SIZE]);
                    // MAME CHD stores CD audio tracks in big-endian 16-bit order (and CdFlacCodec always outputs BE).
                    // Skip swapping only if a data sector inside an audio pregap was already stored raw unswapped.
                    if is_chd_audio && out_sector[0..12] != CD_SYNC_HEADER {
                        for pair in out_sector.chunks_exact_mut(2) {
                            pair.swap(0, 1);
                        }
                    }
                    true
                } else {
                    error!("CD-ROM: Sector offset out of hunk bounds");
                    false
                }
            }
            CdSource::RawFiles { files } => {
                if let Some(ref track) = matched_track {
                    if lba < track.source_start_lba || lba >= track.source_start_lba + track.stored_sectors {
                        out_sector.fill(0);
                        return true;
                    }
                    if track.file_index >= files.len() {
                        return false;
                    }
                    let file = &mut files[track.file_index];
                    let rel_lba = lba - track.source_start_lba;
                    let offset = track.file_offset + (rel_lba as u64) * (track.sector_size as u64);

                    if file.seek(SeekFrom::Start(offset)).is_err() {
                        return false;
                    }
                    if track.sector_size == CD_SECTOR_RAW_SIZE || track.sector_size == CD_FRAME_SIZE {
                        if file.read_exact(out_sector).is_err() {
                            return false;
                        }
                        if out_sector[0..12] == CD_SWAPPED_SYNC_HEADER {
                            for pair in out_sector.chunks_exact_mut(2) {
                                pair.swap(0, 1);
                            }
                        }
                        true
                    } else if track.sector_size == 2336 {
                        out_sector.fill(0);
                        synthesize_mode2_sync_and_msf(out_sector, lba);
                        file.read_exact(&mut out_sector[16..CD_SECTOR_RAW_SIZE]).is_ok()
                    } else {
                        out_sector.fill(0);
                        let mut data = [0u8; 2048];
                        if file.read_exact(&mut data).is_err() {
                            return false;
                        }
                        out_sector[24..24 + 2048].copy_from_slice(&data);
                        synthesize_cooked_mode2_form1_header(out_sector, lba);
                        true
                    }
                } else {
                    error!("CD-ROM: No track matches LBA {}", lba);
                    false
                }
            }
            #[cfg(target_os = "windows")]
            CdSource::PhysicalDrive { handle, drive_letter: _ } => {
                let is_audio = matched_track.as_ref().map_or(false, |t| (t.control & 0x40) == 0);

                // 1. Try IOCTL_CDROM_RAW_READ
                let modes_to_try: &[u32] = if is_audio {
                    &[2, 1, 0] // CDDA, XAForm2, YellowMode2
                } else {
                    &[1, 0, 2] // XAForm2, YellowMode2, CDDA
                };

                let mut raw_read_success = false;
                for &mode in modes_to_try {
                    let mut raw_info = RawReadInfo {
                        disk_offset: (lba as i64) * 2048,
                        sector_count: 1,
                        track_mode: mode,
                    };
                    let mut bytes_ret = 0u32;
                    let ok = unsafe {
                        DeviceIoControl(
                            handle.0,
                            0x0002_403E, // IOCTL_CDROM_RAW_READ
                            &mut raw_info as *mut _ as *const std::ffi::c_void,
                            std::mem::size_of::<RawReadInfo>() as u32,
                            out_sector.as_mut_ptr() as *mut std::ffi::c_void,
                            CD_SECTOR_RAW_SIZE as u32,
                            &mut bytes_ret,
                            std::ptr::null_mut(),
                        )
                    };
                    if ok != 0 && bytes_ret == CD_SECTOR_RAW_SIZE as u32 {
                        raw_read_success = true;
                        break;
                    }
                }

                if raw_read_success {
                    true
                } else {
                    // 2. Fallback: Cooked 2048-byte read via SetFilePointerEx + ReadFile
                    let byte_offset = (lba as i64) * 2048;
                    let mut new_pos = 0i64;
                    let seek_ok = unsafe {
                        SetFilePointerEx(handle.0, byte_offset, &mut new_pos, 0)
                    };
                    if seek_ok != 0 {
                        out_sector.fill(0);
                        let mut bytes_read = 0u32;
                        let read_ok = unsafe {
                            ReadFile(
                                handle.0,
                                out_sector[24..24 + 2048].as_mut_ptr() as *mut std::ffi::c_void,
                                2048,
                                &mut bytes_read,
                                std::ptr::null_mut(),
                            )
                        };
                        if read_ok != 0 && bytes_read == 2048 {
                            synthesize_cooked_mode2_form1_header(out_sector, lba);
                            true
                        } else {
                            error!("CD-ROM Fisico: Fallita lettura settore LBA {}", lba);
                            false
                        }
                    } else {
                        error!("CD-ROM Fisico: Fallita lettura settore LBA {}", lba);
                        false
                    }
                }
            }
        };

        if success {
            maybe_descramble_sector(out_sector);
        }
        success
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_drive_letter() {
        assert_eq!(parse_drive_letter("E:"), Some('E'));
        assert_eq!(parse_drive_letter("e:"), Some('E'));
        assert_eq!(parse_drive_letter("E:\\"), Some('E'));
        assert_eq!(parse_drive_letter("e:/"), Some('E'));
        assert_eq!(parse_drive_letter(r"\\.\E:"), Some('E'));
        assert_eq!(parse_drive_letter(r"\\.\e:"), Some('E'));
        assert_eq!(parse_drive_letter("game.chd"), None);
        assert_eq!(parse_drive_letter("C:\\games\\mario.iso"), None);
    }

    #[test]
    fn test_cue_single_file_cdi_ready_pregap() {
        let cue = "\
FILE \"pav.bin\" BINARY
  TRACK 01 AUDIO
    INDEX 00 00:00:00
    INDEX 01 28:34:00
  TRACK 02 AUDIO
    INDEX 00 31:53:67
    INDEX 01 31:55:67
";
        let (files, parsed) = parse_cue_sheet(cue);
        assert_eq!(files, vec!["pav.bin"]);
        let total_bin_sectors: u64 = 143_542 + 14_188;
        let (tracks, total_sectors) = build_cue_tracks(&parsed, &[total_bin_sectors * CD_SECTOR_RAW_SIZE as u64]);

        assert_eq!(tracks.len(), 2);
        assert_eq!(total_sectors, total_bin_sectors as u32);

        // Track 1: pregap 0..128550 contains CD-i data, INDEX 01 is at 128550
        assert_eq!(tracks[0].number, 1);
        assert_eq!(tracks[0].control, 0x01);
        assert_eq!(tracks[0].pregap_start_lba, 0);
        assert_eq!(tracks[0].source_start_lba, 0);
        assert_eq!(tracks[0].start_lba, 128_550);
        assert_eq!(tracks[0].length_sectors, 14_992);
        assert_eq!(tracks[0].file_offset, 0);
        assert!(tracks[0].contains_lba(16));

        // Track 2: starts with INDEX 00 pause at 143542, INDEX 01 at 143692
        assert_eq!(tracks[1].number, 2);
        assert_eq!(tracks[1].pregap_start_lba, 143_542);
        assert_eq!(tracks[1].source_start_lba, 143_542);
        assert_eq!(tracks[1].start_lba, 143_692);
        assert_eq!(tracks[1].length_sectors, 14_038);
        assert_eq!(tracks[1].file_offset, 143_542 * CD_SECTOR_RAW_SIZE as u64);
        assert!(tracks[1].contains_lba(143_600));
        assert!(tracks[1].contains_lba(143_692));
    }

    #[test]
    fn test_cue_multi_file_and_virtual_pregap_read_sector() {
        let dir = std::env::temp_dir().join("opencdi_test_cdi_ready_cue");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let t1_pregap = 20u32;
        let t1_audio = 4u32;
        let mut t1_bin = vec![0u8; ((t1_pregap + t1_audio) as usize) * CD_SECTOR_RAW_SIZE];
        // Populate LBA 16 as unscrambled Mode 2 CD-RTOS PVD
        let pvd = &mut t1_bin[16 * CD_SECTOR_RAW_SIZE..(16 + 1) * CD_SECTOR_RAW_SIZE];
        pvd[0..12].copy_from_slice(&CD_SYNC_HEADER);
        pvd[15] = 2;
        pvd[24..24 + 14].copy_from_slice(b"\x01CD-I  CD-RTOS");
        // Marker in Track 1 INDEX 01 audio
        t1_bin[(t1_pregap as usize) * CD_SECTOR_RAW_SIZE] = 0xAA;

        let t2_pregap = 2u32;
        let t2_audio = 3u32;
        let mut t2_bin = vec![0u8; ((t2_pregap + t2_audio) as usize) * CD_SECTOR_RAW_SIZE];
        t2_bin[(t2_pregap as usize) * CD_SECTOR_RAW_SIZE] = 0xBB;

        let t3_audio = 2u32;
        let t3_bin = vec![0xCCu8; (t3_audio as usize) * CD_SECTOR_RAW_SIZE];

        std::fs::write(dir.join("t1.bin"), &t1_bin).unwrap();
        std::fs::write(dir.join("t2.bin"), &t2_bin).unwrap();
        std::fs::write(dir.join("t3.bin"), &t3_bin).unwrap();
        std::fs::write(
            dir.join("disc.cue"),
            "\
FILE \"t1.bin\" BINARY
  TRACK 01 AUDIO
    INDEX 00 00:00:00
    INDEX 01 00:00:20
FILE \"t2.bin\" BINARY
  TRACK 02 AUDIO
    INDEX 00 00:00:00
    INDEX 01 00:00:02
FILE \"t3.bin\" BINARY
  TRACK 03 AUDIO
    PREGAP 00:00:02
    INDEX 01 00:00:00
",
        )
        .unwrap();

        let mut cd = CdRom::open(dir.join("disc.cue")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(cd.tracks.len(), 3);
        assert_eq!(
            cd.disc_type, 0x00,
            "CD-i Ready disc (Track 1 AUDIO with CD-RTOS pregap) must report TOC A0 PSEC = 0x00"
        );
        assert_eq!(cd.tracks[0].start_lba, 20);
        assert_eq!(cd.tracks[1].pregap_start_lba, 24);
        assert_eq!(cd.tracks[1].start_lba, 26);
        assert_eq!(cd.tracks[2].pregap_start_lba, 29);
        assert_eq!(cd.tracks[2].source_start_lba, 31);
        assert_eq!(cd.tracks[2].start_lba, 31);
        assert_eq!(cd.total_sectors, 33);

        let mut sec = [0u8; CD_SECTOR_RAW_SIZE];
        assert!(cd.read_sector(16, &mut sec));
        assert_eq!(&sec[24..24 + 14], b"\x01CD-I  CD-RTOS");
        assert!(cd.read_sector(20, &mut sec));
        assert_eq!(sec[0], 0xAA);
        assert!(cd.read_sector(26, &mut sec));
        assert_eq!(sec[0], 0xBB);
        // Track 3 virtual PREGAP at LBA 29..31 returns synthesized silence
        assert!(cd.read_sector(29, &mut sec));
        assert!(sec.iter().all(|&b| b == 0));
        // Track 3 stored data starts at LBA 31
        assert!(cd.read_sector(31, &mut sec));
        assert_eq!(sec[0], 0xCC);
    }

    #[test]
    fn test_chd_metadata_tracks_and_4_unit_padding() {
        let raw_entries = [
            "TRACK:1 TYPE:AUDIO SUBTYPE:NONE FRAMES:143542 PREGAP:128550 PGTYPE:VAUDIO PGSUB:NONE POSTGAP:0\0",
            "TRACK:2 TYPE:AUDIO SUBTYPE:NONE FRAMES:14188 PREGAP:150 PGTYPE:VAUDIO PGSUB:NONE POSTGAP:0\0",
            "TRACK:3 TYPE:AUDIO SUBTYPE:NONE FRAMES:501 PREGAP:150 PGTYPE:AUDIO PGSUB:NONE POSTGAP:75\0",
            "TRACK:4 TYPE:MODE2_RAW SUBTYPE:NONE FRAMES:200\0",
        ];
        let parsed: Vec<ChdTrackMeta> = raw_entries
            .iter()
            .filter_map(|s| parse_chd_track_entry(s))
            .collect();
        assert_eq!(parsed.len(), 4);

        let (tracks, total_sectors) = build_chd_tracks(&parsed);
        assert_eq!(tracks.len(), 4);

        // Track 1: 143542 frames, VAUDIO pregap inside stream; padded unit end = 143544 (CD_TRACK_PADDING=4)
        assert_eq!(tracks[0].pregap_start_lba, 0);
        assert_eq!(tracks[0].start_lba, 128_550);
        assert_eq!(tracks[0].length_sectors, 14_992);
        assert_eq!(tracks[0].chd_unit_start, 0);
        assert!(tracks[0].chd_audio_swap);

        // Track 2: disc LBA starts at 143542, CHD physical unit starts at padded 143544
        assert_eq!(tracks[1].pregap_start_lba, 143_542);
        assert_eq!(tracks[1].source_start_lba, 143_542);
        assert_eq!(tracks[1].start_lba, 143_692);
        assert_eq!(tracks[1].chd_unit_start, 143_544);

        // Track 3: non-V PGTYPE:AUDIO -> 150 virtual pregap before source_start_lba, plus 75 postgap
        assert_eq!(tracks[2].pregap_start_lba, 157_730);
        assert_eq!(tracks[2].source_start_lba, 157_880);
        assert_eq!(tracks[2].start_lba, 157_880);
        assert_eq!(tracks[2].chd_unit_start, 143_544 + 14_188);
        assert_eq!(tracks[2].total_sectors, 150 + 501 + 75);

        // Track 4 (CHTR format without PREGAP fields): Track 3 frames=501 pads to 504 units
        assert_eq!(tracks[3].control, 0x41);
        assert!(!tracks[3].chd_audio_swap);
        assert_eq!(tracks[3].chd_unit_start, 143_544 + 14_188 + 504);
        assert_eq!(total_sectors, 143_542 + 14_188 + 150 + 501 + 75 + 200);
    }

    #[test]
    fn test_chd_cdi_ready_apprentice_layout() {
        let entries = [
            parse_chd_track_entry("TRACK:1 TYPE:AUDIO SUBTYPE:NONE FRAMES:81650 PREGAP:69150 PGTYPE:VAUDIO PGSUB:NONE POSTGAP:0\0").unwrap(),
            parse_chd_track_entry("TRACK:2 TYPE:AUDIO SUBTYPE:NONE FRAMES:12000 PREGAP:150 PGTYPE:VAUDIO PGSUB:NONE POSTGAP:0\0").unwrap(),
        ];
        let (tracks, total_sectors) = build_chd_tracks(&entries);
        assert_eq!(total_sectors, 81_650 + 12_000);
        assert_eq!(tracks[0].control, 0x01);
        assert_eq!(tracks[0].pregap_start_lba, 0);
        assert_eq!(tracks[0].source_start_lba, 0);
        assert_eq!(tracks[0].start_lba, 69_150);
        assert!(tracks[0].chd_audio_swap);
        assert!(tracks[0].contains_lba(16));
        assert!(tracks[0].contains_lba(69_150));
        assert!(CdRom::is_cdi_ready_layout(&tracks));
    }

    #[test]
    fn test_iso_sector_geometry_autodetect_and_cdic_selection() {
        let dir = std::env::temp_dir().join(format!("opencdi-iso-geom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let make_pvd = |pub_tag: &[u8], prep_tag: &[u8]| {
            let mut pvd = vec![0u8; 2048];
            pvd[0] = 0x01;
            pvd[1..6].copy_from_slice(b"CD-I ");
            pvd[8..15].copy_from_slice(b"CD-RTOS");
            pvd[40..51].copy_from_slice(b"HOTEL_MARIO");
            pvd[190..197].copy_from_slice(b"VOL_SET");
            pvd[318..318 + pub_tag.len()].copy_from_slice(pub_tag);
            pvd[446..446 + prep_tag.len()].copy_from_slice(prep_tag);
            pvd
        };
        let make_vd_term = || {
            let mut term = vec![0u8; 2048];
            term[0] = 0xFF;
            term[1..6].copy_from_slice(b"CD-I ");
            term
        };

        // 1. Cooked 2048-byte .iso (e.g. IsoBuster / mounted virtual drive extraction)
        let iso2048 = dir.join("mario_2048.iso");
        let mut buf2048 = vec![0u8; 20 * 2048];
        buf2048[16 * 2048..17 * 2048].copy_from_slice(&make_pvd(b"PIMA", b"PHILIPS POV"));
        buf2048[17 * 2048..18 * 2048].copy_from_slice(&make_vd_term());
        buf2048[18 * 2048..18 * 2048 + 4].copy_from_slice(b"DIR!");
        std::fs::write(&iso2048, &buf2048).unwrap();
        let mut cd2048 = CdRom::open(&iso2048).unwrap();
        assert_eq!(cd2048.tracks[0].sector_size, 2048);
        assert_eq!(cd2048.volume_id, "HOTEL_MARIO");
        assert_eq!(cd2048.volume_set, "VOL_SET");
        assert_eq!(cd2048.publisher, "PIMA");
        assert_eq!(cd2048.preparer, "PHILIPS POV");
        assert_eq!(cd2048.is_ntsc_hint("mario_2048"), Some(true));

        let mut cdic = crate::cdic::Cdic::new();
        cdic.channel = 0xFFFF_FFFF;
        cdic.audio_channel = 0xFFFF;
        let mut sec = [0u8; CD_SECTOR_RAW_SIZE];
        assert!(cd2048.read_sector(16, &mut sec));
        assert_eq!(sec[18], 0x09);
        assert_eq!(sec[22], 0x09);
        assert!(cdic.is_mode2_sector_selected(&sec));
        assert!(cd2048.read_sector(17, &mut sec));
        assert_eq!(sec[22], 0x89);
        assert!(cdic.is_mode2_sector_selected(&sec));
        assert!(cd2048.read_sector(18, &mut sec));
        assert_eq!(&sec[24..28], b"DIR!");
        assert!(cdic.is_mode2_sector_selected(&sec));

        // 2. Raw 2352-byte .iso (TOSEC / CD-i Emu)
        let iso2352 = dir.join("hotel_mario.iso");
        let mut buf2352 = vec![0u8; 20 * 2352];
        for lba in 0..20usize {
            let base = lba * 2352;
            buf2352[base..base + 12].copy_from_slice(&CD_SYNC_HEADER);
            buf2352[base + 15] = 2;
            buf2352[base + 18] = 0x08;
            buf2352[base + 22] = 0x08;
        }
        buf2352[16 * 2352 + 24..16 * 2352 + 2072].copy_from_slice(&make_pvd(b"PIMI", b""));
        // Inject PAL 25 Hz MPEG-1 sequence header on Form 2 video sector LBA 19
        buf2352[19 * 2352 + 18] = 0x62;
        buf2352[19 * 2352 + 22] = 0x62;
        // 352x288 @ 25 Hz (frc=3), PAR=8
        buf2352[19 * 2352 + 32..19 * 2352 + 40].copy_from_slice(&[0x00, 0x00, 0x01, 0xB3, 0x16, 0x01, 0x20, 0x83]);
        std::fs::write(&iso2352, &buf2352).unwrap();
        let cd2352 = CdRom::open(&iso2352).unwrap();
        assert_eq!(cd2352.tracks[0].sector_size, 2352);
        assert_eq!(cd2352.is_ntsc_hint("hotel_mario"), Some(false));

        // 3. 2336-byte Mode 2 .iso (subheader + 2328 payload)
        let iso2336 = dir.join("mario_2336.iso");
        let mut buf2336 = vec![0u8; 20 * 2336];
        buf2336[16 * 2336 + 2] = 0x09;
        buf2336[16 * 2336 + 6] = 0x09;
        buf2336[16 * 2336 + 8..16 * 2336 + 2056].copy_from_slice(&make_pvd(b"PIMA", b""));
        std::fs::write(&iso2336, &buf2336).unwrap();
        let mut cd2336 = CdRom::open(&iso2336).unwrap();
        assert_eq!(cd2336.tracks[0].sector_size, 2336);
        assert!(cd2336.read_sector(16, &mut sec));
        assert_eq!(sec[0..12], CD_SYNC_HEADER);
        assert_eq!(sec[22], 0x09);
        assert!(cdic.is_mode2_sector_selected(&sec));

        // 4. 2448-byte raw+subchannel .iso
        let iso2448 = dir.join("mario_2448.iso");
        let mut buf2448 = vec![0u8; 20 * 2448];
        for lba in 0..20usize {
            let base = lba * 2448;
            buf2448[base..base + 12].copy_from_slice(&CD_SYNC_HEADER);
            buf2448[base + 15] = 2;
        }
        buf2448[16 * 2448 + 24..16 * 2448 + 2072].copy_from_slice(&make_pvd(b"PIMI", b""));
        std::fs::write(&iso2448, &buf2448).unwrap();
        let cd2448 = CdRom::open(&iso2448).unwrap();
        assert_eq!(cd2448.tracks[0].sector_size, 2448);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

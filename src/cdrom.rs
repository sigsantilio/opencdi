use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use chd::Chd;
use log::{info, error};

pub const CD_SECTOR_RAW_SIZE: usize = 2352;
pub const CD_SUBCHANNEL_SIZE: usize = 96;
pub const CD_FRAME_SIZE: usize = CD_SECTOR_RAW_SIZE + CD_SUBCHANNEL_SIZE; // 2448

#[derive(Clone, Debug)]
pub struct CdTrack {
    pub number: u8,
    pub control: u8,      // 0x41 for Data, 0x01 for Audio
    pub start_lba: u32,
    pub length_sectors: u32,
    pub sector_size: usize,
    pub file_index: usize,
    pub file_offset: u64,
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
            let mut files: Vec<File> = Vec::new();
            let mut file_sizes: Vec<u64> = Vec::new();

            struct ParsedTrack {
                num: u8,
                control: u8,
                sector_size: usize,
                file_index: usize,
                idx01: u32,
            }
            let mut parsed_tracks: Vec<ParsedTrack> = Vec::new();
            let mut current_file_idx: Option<usize> = None;

            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }

                if parts[0].eq_ignore_ascii_case("FILE") {
                    if let Some(start) = trimmed.find('"') {
                        if let Some(end) = trimmed.rfind('"') {
                            if end > start {
                                let fname = &trimmed[start + 1..end];
                                let fpath = path_ref.parent().map(|p| p.join(fname)).unwrap_or_else(|| PathBuf::from(fname));
                                let file = File::open(&fpath)?;
                                let fsize = file.metadata()?.len();
                                files.push(file);
                                file_sizes.push(fsize);
                                current_file_idx = Some(files.len() - 1);
                            }
                        }
                    }
                } else if parts[0].eq_ignore_ascii_case("TRACK") {
                    if parts.len() >= 3 {
                        let tnum: u8 = parts[1].parse().unwrap_or(1);
                        let mode = parts[2].to_uppercase();
                        let control = if mode.starts_with("AUDIO") { 0x01 } else { 0x41 };
                        let sector_size = if mode.contains("2048") { 2048 } else { CD_SECTOR_RAW_SIZE };
                        let fidx = current_file_idx.unwrap_or(0);
                        parsed_tracks.push(ParsedTrack {
                            num: tnum,
                            control,
                            sector_size,
                            file_index: fidx,
                            idx01: 0,
                        });
                    }
                } else if parts[0].eq_ignore_ascii_case("INDEX") {
                    if parts.len() >= 3 && (parts[1] == "01" || parts[1] == "1") {
                        if let Some(t) = parsed_tracks.last_mut() {
                            if let Some(sectors) = parse_msf(parts[2]) {
                                t.idx01 = sectors;
                            }
                        }
                    }
                }
            }

            if files.is_empty() {
                return Err(format!("No files found in CUE sheet {:?}", path_ref).into());
            }

            let mut tracks: Vec<CdTrack> = Vec::new();
            let total_sectors: u32;

            if files.len() == 1 {
                let fsize = file_sizes[0];
                let sec_size = parsed_tracks.first().map(|t| t.sector_size).unwrap_or(CD_SECTOR_RAW_SIZE);
                let file_sectors = (fsize / sec_size as u64) as u32;

                for i in 0..parsed_tracks.len() {
                    let pt = &parsed_tracks[i];
                    let start_lba = pt.idx01;
                    let file_offset = (start_lba as u64) * (pt.sector_size as u64);
                    let length_sectors = if i + 1 < parsed_tracks.len() {
                        parsed_tracks[i + 1].idx01.saturating_sub(start_lba)
                    } else {
                        file_sectors.saturating_sub(start_lba)
                    };

                    tracks.push(CdTrack {
                        number: pt.num,
                        control: pt.control,
                        start_lba,
                        length_sectors,
                        sector_size: pt.sector_size,
                        file_index: 0,
                        file_offset,
                    });
                }
                total_sectors = file_sectors;
            } else {
                let mut current_disc_lba = 0u32;
                for pt in parsed_tracks {
                    let fsize = file_sizes.get(pt.file_index).copied().unwrap_or(0);
                    let length_sectors = (fsize / pt.sector_size as u64) as u32;
                    tracks.push(CdTrack {
                        number: pt.num,
                        control: pt.control,
                        start_lba: current_disc_lba,
                        length_sectors,
                        sector_size: pt.sector_size,
                        file_index: pt.file_index,
                        file_offset: 0,
                    });
                    current_disc_lba += length_sectors;
                }
                total_sectors = current_disc_lba;
            }

            if tracks.is_empty() {
                tracks.push(CdTrack {
                    number: 1,
                    control: 0x41,
                    start_lba: 0,
                    length_sectors: total_sectors,
                    sector_size: CD_SECTOR_RAW_SIZE,
                    file_index: 0,
                    file_offset: 0,
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
                info!("  Track {:02}: LBA {}..{} ({} sectors) ctrl=0x{:02X} file_idx={}", t.number, t.start_lba, t.start_lba + t.length_sectors.saturating_sub(1), t.length_sectors, t.control, t.file_index);
            }

            let mut cdrom = Self {
                source: CdSource::RawFiles { files },
                total_sectors,
                tracks,
                disc_type: 0x10,
                volume_id: String::new(),
                volume_set: String::new(),
                publisher: String::new(),
            };
            cdrom.detect_disc_type();
            return Ok(cdrom);
        }

        if ext == "chd" {
            let file = File::open(path_ref)?;
            let chd = Chd::open(file, None)?;
            let header = chd.header();

            let hunk_size = header.hunk_size() as usize;
            let sectors_per_hunk = if hunk_size % CD_FRAME_SIZE == 0 {
                (hunk_size / CD_FRAME_SIZE) as u32
            } else if hunk_size % CD_SECTOR_RAW_SIZE == 0 {
                (hunk_size / CD_SECTOR_RAW_SIZE) as u32
            } else {
                8
            };

            let total_sectors = header.hunk_count() * sectors_per_hunk;
            let hunk_buf = chd.get_hunksized_buffer();

            info!(
                "Opened CHD: {:?}, hunk_size={}, sectors_per_hunk={}, total_sectors={}",
                path_ref, hunk_size, sectors_per_hunk, total_sectors
            );

            let tracks = vec![CdTrack {
                number: 1,
                control: 0x41,
                start_lba: 0,
                length_sectors: total_sectors,
                sector_size: CD_SECTOR_RAW_SIZE,
                file_index: 0,
                file_offset: 0,
            }];

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
            };
            cdrom.detect_disc_type();
            Ok(cdrom)
        } else {
            let file = File::open(path_ref)?;
            let file_size = file.metadata()?.len();
            let sector_size = if ext == "iso" { 2048 } else { CD_SECTOR_RAW_SIZE };
            let total_sectors = (file_size / sector_size as u64) as u32;

            info!(
                "Opened raw disc image: {:?}, sector_size={}, total_sectors={}",
                path_ref, sector_size, total_sectors
            );

            let tracks = vec![CdTrack {
                number: 1,
                control: 0x41,
                start_lba: 0,
                length_sectors: total_sectors,
                sector_size,
                file_index: 0,
                file_offset: 0,
            }];

            let mut cdrom = Self {
                source: CdSource::RawFiles { files: vec![file] },
                total_sectors,
                tracks,
                disc_type: 0x10,
                volume_id: String::new(),
                volume_set: String::new(),
                publisher: String::new(),
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

                tracks.push(CdTrack {
                    number: td.track_number,
                    control,
                    start_lba,
                    length_sectors,
                    sector_size: CD_SECTOR_RAW_SIZE,
                    file_index: 0,
                    file_offset: 0,
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
            };

            cdrom.detect_disc_type();
            Ok(cdrom)
        }
    }

    fn detect_disc_type(&mut self) {
        let mut s16 = [0u8; CD_SECTOR_RAW_SIZE];
        if self.read_sector(16, &mut s16) {
            let sys_id = &s16[24 + 8..24 + 40];
            self.volume_id = String::from_utf8_lossy(&s16[24 + 40..24 + 72]).trim().to_string();
            self.volume_set = String::from_utf8_lossy(&s16[24 + 318..24 + 446]).trim().to_string();
            self.publisher = String::from_utf8_lossy(&s16[24 + 446..24 + 574]).trim().to_string();
            if sys_id.starts_with(b"CD-RTOS CD-BRIDGE") {
                info!("CD-ROM: Riconosciuto disco CD-RTOS CD-BRIDGE (Video CD / White Book), disc_type = 0x20");
                self.disc_type = 0x20;
            } else if sys_id.starts_with(b"CD-RTOS") || sys_id.starts_with(b"CD-I") {
                info!("CD-ROM: Riconosciuto disco CD-i standard, disc_type = 0x10, volume = {:?}", self.volume_id);
                self.disc_type = 0x10;
            } else {
                info!("CD-ROM: Disc system ID: {:?}, disc_type = 0x10, volume = {:?}", String::from_utf8_lossy(sys_id).trim(), self.volume_id);
                self.disc_type = 0x10;
            }
        } else {
            let only_audio = self.tracks.iter().all(|t| (t.control & 0x40) == 0);
            if only_audio && !self.tracks.is_empty() {
                info!("CD-ROM: Riconosciuto disco CD-DA Audio Red Book, disc_type = 0x00");
                self.disc_type = 0x00;
            }
        }
    }

    /// Evaluates whether this disc should run in NTSC (60 Hz) mode based on filename and Sector 16 metadata.
    pub fn is_ntsc_hint(&self, disc_name: &str) -> Option<bool> {
        let name_upper = disc_name.to_uppercase();
        if name_upper.contains("(USA)") || name_upper.contains("(JAPAN)") || name_upper.contains("(US)") || name_upper.contains("NTSC") {
            return Some(true);
        }
        if name_upper.contains("(EUROPE)") || name_upper.contains("(PAL)") || name_upper.contains("(GERMANY)")
            || name_upper.contains("(FRANCE)") || name_upper.contains("(UK)") || name_upper.contains("(ITALY)")
            || name_upper.contains("(SPAIN)") || name_upper.contains("(NETHERLANDS)") {
            return Some(false);
        }
        let vol_upper = self.volume_id.to_uppercase();
        if vol_upper.contains("EARTH COMMAND") {
            return Some(true);
        }
        None
    }

    pub fn read_sector(&mut self, lba: u32, out_sector: &mut [u8; CD_SECTOR_RAW_SIZE]) -> bool {
        if lba >= self.total_sectors {
            error!("CD-ROM: Sector {} out of bounds (max {})", lba, self.total_sectors);
            return false;
        }

        match &mut self.source {
            CdSource::Chd { chd, hunk_buf, comp_buf, current_hunk, hunk_size, sectors_per_hunk } => {
                let hunk_idx = lba / *sectors_per_hunk;
                let sector_in_hunk = (lba % *sectors_per_hunk) as usize;

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
                    true
                } else {
                    error!("CD-ROM: Sector offset out of hunk bounds");
                    false
                }
            }
            CdSource::RawFiles { files } => {
                if let Some(track) = self.tracks.iter().find(|t| lba >= t.start_lba && lba < t.start_lba + t.length_sectors) {
                    if track.file_index >= files.len() {
                        return false;
                    }
                    let file = &mut files[track.file_index];
                    let rel_lba = lba - track.start_lba;
                    let offset = track.file_offset + (rel_lba as u64) * (track.sector_size as u64);

                    if file.seek(SeekFrom::Start(offset)).is_err() {
                        return false;
                    }
                    if track.sector_size == CD_SECTOR_RAW_SIZE {
                        file.read_exact(out_sector).is_ok()
                    } else {
                        out_sector.fill(0);
                        let mut data = [0u8; 2048];
                        if file.read_exact(&mut data).is_err() {
                            return false;
                        }
                        out_sector[24..24 + 2048].copy_from_slice(&data);
                        true
                    }
                } else {
                    error!("CD-ROM: No track matches LBA {}", lba);
                    false
                }
            }
            #[cfg(target_os = "windows")]
            CdSource::PhysicalDrive { handle, drive_letter: _ } => {
                let track = self.tracks.iter().find(|t| lba >= t.start_lba && lba < t.start_lba + t.length_sectors);
                let is_audio = track.map_or(false, |t| (t.control & 0x40) == 0);

                // 1. Try IOCTL_CDROM_RAW_READ
                let modes_to_try: &[u32] = if is_audio {
                    &[2, 1, 0] // CDDA, XAForm2, YellowMode2
                } else {
                    &[1, 0, 2] // XAForm2, YellowMode2, CDDA
                };

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
                        return true;
                    }
                }

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
                        // Synthesize standard Mode 2 sync header
                        out_sector[0] = 0x00;
                        out_sector[1..11].fill(0xFF);
                        out_sector[11] = 0x00;
                        let m = (lba + 150) / 75 / 60;
                        let s = ((lba + 150) / 75) % 60;
                        let f = (lba + 150) % 75;
                        out_sector[12] = ((m / 10) << 4 | (m % 10)) as u8;
                        out_sector[13] = ((s / 10) << 4 | (s % 10)) as u8;
                        out_sector[14] = ((f / 10) << 4 | (f % 10)) as u8;
                        out_sector[15] = 2; // Mode 2
                        return true;
                    }
                }

                error!("CD-ROM Fisico: Fallita lettura settore LBA {}", lba);
                false
            }
        }
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
}

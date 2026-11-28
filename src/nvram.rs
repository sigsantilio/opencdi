use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use log::{info, warn};

/// Local calendar date and time used to initialize the CD-i real-time clock on host/libretro boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtcDateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    /// 1 = Sunday .. 7 = Saturday (MK48T08 / DS1216 convention)
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

#[inline]
pub const fn to_bcd(val: u8) -> u8 {
    ((val / 10) << 4) | (val % 10)
}

/// Converts Unix epoch seconds (UTC) into `RtcDateTime` using Howard Hinnant's civil-from-days algorithm.
pub fn civil_from_unix_secs(secs: u64) -> RtcDateTime {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = (rem / 3_600) as u8;
    let minute = ((rem % 3_600) / 60) as u8;
    let second = (rem % 60) as u8;
    // 1970-01-01 was Thursday (index 4 in 0=Sun..6=Sat -> 1..=7 is +1)
    let dow0 = (days + 4).rem_euclid(7) as u8;
    let weekday = dow0 + 1;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u8;
    let year = (y + if month <= 2 { 1 } else { 0 }) as u16;

    RtcDateTime {
        year,
        month,
        day,
        weekday,
        hour,
        minute,
        second,
    }
}

/// Reads the host OS local wall-clock date/time, falling back to UTC if OS APIs are unavailable.
pub fn host_local_datetime() -> RtcDateTime {
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct SystemTimeWin {
            w_year: u16,
            w_month: u16,
            w_day_of_week: u16,
            w_day: u16,
            w_hour: u16,
            w_minute: u16,
            w_second: u16,
            w_milliseconds: u16,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetLocalTime(lpSystemTime: *mut SystemTimeWin);
        }
        let mut st = std::mem::MaybeUninit::<SystemTimeWin>::uninit();
        unsafe { GetLocalTime(st.as_mut_ptr()) };
        let st = unsafe { st.assume_init() };
        return RtcDateTime {
            year: st.w_year,
            month: (st.w_month as u8).clamp(1, 12),
            day: (st.w_day as u8).clamp(1, 31),
            weekday: ((st.w_day_of_week as u8) % 7) + 1,
            hour: (st.w_hour as u8).min(23),
            minute: (st.w_minute as u8).min(59),
            second: (st.w_second as u8).min(59),
        };
    }
    #[cfg(all(unix, not(target_os = "windows")))]
    {
        #[repr(C)]
        struct Tm {
            tm_sec: i32,
            tm_min: i32,
            tm_hour: i32,
            tm_mday: i32,
            tm_mon: i32,
            tm_year: i32,
            tm_wday: i32,
            tm_yday: i32,
            tm_isdst: i32,
            tm_gmtoff: std::ffi::c_long,
            tm_zone: *const std::ffi::c_char,
        }
        unsafe extern "C" {
            fn time(tloc: *mut i64) -> i64;
            fn localtime_r(timep: *const i64, result: *mut Tm) -> *mut Tm;
        }
        let mut now: i64 = 0;
        unsafe { time(&mut now) };
        let mut tm = std::mem::MaybeUninit::<Tm>::zeroed();
        let res = unsafe { localtime_r(&now, tm.as_mut_ptr()) };
        if !res.is_null() {
            let tm = unsafe { tm.assume_init() };
            return RtcDateTime {
                year: (1900 + tm.tm_year).max(1970) as u16,
                month: (tm.tm_mon + 1).clamp(1, 12) as u8,
                day: tm.tm_mday.clamp(1, 31) as u8,
                weekday: (tm.tm_wday.rem_euclid(7) + 1) as u8,
                hour: tm.tm_hour.clamp(0, 23) as u8,
                minute: tm.tm_min.clamp(0, 59) as u8,
                second: tm.tm_sec.clamp(0, 59) as u8,
            };
        }
    }
    #[allow(unreachable_code)]
    {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        civil_from_unix_secs(secs)
    }
}

pub struct Timekeeper {
    pub ram: [u8; 8192],
    pub dirty: bool,
    pub dirty_frames: u32,
    // Empty: no persistence (harness, libretro); the GUI host sets the file.
    pub save_path: PathBuf,
    pub persist: bool, // false when the frontend keeps the RAM (libretro save RAM), so no file is written
}

impl Timekeeper {
    pub fn new() -> Self {
        let mut tk = Self {
            ram: [0; 8192],
            dirty: false,
            dirty_frames: 0,
            save_path: PathBuf::new(),
            persist: true,
        };
        tk.init_default();
        tk
    }

    pub fn load_from_file<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<()> {
        let p = path.as_ref();
        self.save_path = p.to_path_buf();
        let data = fs::read(p)?;
        let len = data.len().min(8192);
        self.ram[..len].copy_from_slice(&data[..len]);
        self.dirty = false;
        self.dirty_frames = 0;
        Ok(())
    }

    pub fn save_to_file(&mut self) -> std::io::Result<()> {
        if let Some(parent) = self.save_path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = fs::create_dir_all(parent);
            }
        }
        fs::write(&self.save_path, &self.ram[..])?;
        self.dirty = false;
        self.dirty_frames = 0;
        info!("NVRAM salvata su disco in {:?}", self.save_path);
        Ok(())
    }

    pub fn flush_if_dirty(&mut self) {
        if !self.persist {
            self.dirty = false;
            self.dirty_frames = 0;
        } else if self.dirty && !self.save_path.as_os_str().is_empty() {
            if let Err(e) = self.save_to_file() {
                warn!("Impossibile salvare NVRAM in {:?}: {}", self.save_path, e);
            }
        }
    }

    pub fn clear(&mut self) -> std::io::Result<()> {
        self.ram.fill(0);
        self.init_default();
        self.dirty = true;
        self.flush_if_dirty();
        Ok(())
    }

    fn init_default(&mut self) {
        // MK48T08 timekeeper registers are at the top (8184..8191)
        // Set standard flags: Calibration = 0, Seconds = 0 (oscillator running, bit 7 = 0)
        self.ram[8184] = 0x00; // Control
        self.ram[8185] = 0x00; // Seconds (bit 7 = 0 enables oscillator)
        self.ram[8186] = 0x00; // Minutes
        self.ram[8187] = 0x12; // Hours (12:00)
        self.ram[8188] = 0x01; // Day of week
        self.ram[8189] = 0x01; // Date
        self.ram[8190] = 0x01; // Month
        self.ram[8191] = 0x95; // Year (1995)
    }

    pub fn read(&self, offset: u32) -> u8 {
        let idx = (offset as usize) % 8192;
        self.ram[idx]
    }

    pub fn write(&mut self, offset: u32, val: u8) {
        let idx = (offset as usize) % 8192;
        if self.ram[idx] != val {
            self.ram[idx] = val;
            // Mark dirty for NVRAM user storage area (0..8184)
            if idx < 8184 {
                self.dirty = true;
                self.dirty_frames = 0;
            }
        }
    }

    /// Programs the MK48T08 BCD clock registers (`ram[8184..8192]`) without marking user storage dirty.
    pub fn set_clock(&mut self, dt: RtcDateTime) {
        self.ram[8184] = 0x00; // Control: normal run, calibration 0
        self.ram[8185] = to_bcd(dt.second.min(59)) & 0x7F; // Bit 7 = 0 -> oscillator running
        self.ram[8186] = to_bcd(dt.minute.min(59));
        self.ram[8187] = to_bcd(dt.hour.min(23)) & 0x3F;
        self.ram[8188] = dt.weekday.clamp(1, 7);
        self.ram[8189] = to_bcd(dt.day.clamp(1, 31));
        self.ram[8190] = to_bcd(dt.month.clamp(1, 12));
        self.ram[8191] = to_bcd((dt.year % 100) as u8);
    }

    /// Seeds the MK48T08 clock from the host OS local wall clock.
    pub fn seed_clock_from_host(&mut self) {
        self.set_clock(host_local_datetime());
    }
}

opencdi::impl_state!(Timekeeper { ram } skip { dirty, dirty_frames, save_path, persist });

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_unix_handles_epoch_and_leap_dates() {
        assert_eq!(
            civil_from_unix_secs(0),
            RtcDateTime { year: 1970, month: 1, day: 1, weekday: 5, hour: 0, minute: 0, second: 0 }
        );
        // 2000-02-29 23:59:58 UTC (Tuesday = 3)
        assert_eq!(
            civil_from_unix_secs(951_868_798),
            RtcDateTime { year: 2000, month: 2, day: 29, weekday: 3, hour: 23, minute: 59, second: 58 }
        );
        // 2024-02-29 14:35:09 UTC (Thursday = 5)
        assert_eq!(
            civil_from_unix_secs(1_709_217_309),
            RtcDateTime { year: 2024, month: 2, day: 29, weekday: 5, hour: 14, minute: 35, second: 9 }
        );
    }

    #[test]
    fn timekeeper_set_clock_writes_bcd_and_preserves_user_ram() {
        let mut tk = Timekeeper::new();
        tk.ram[0] = 0x5A;
        tk.dirty = false;
        tk.set_clock(RtcDateTime {
            year: 2026,
            month: 11,
            day: 28,
            weekday: 7,
            hour: 9,
            minute: 42,
            second: 7,
        });
        assert!(!tk.dirty, "clock update must not mark user NVRAM dirty");
        assert_eq!(tk.ram[0], 0x5A);
        assert_eq!(&tk.ram[8184..8192], &[0x00, 0x07, 0x42, 0x09, 0x07, 0x28, 0x11, 0x26]);
    }
}

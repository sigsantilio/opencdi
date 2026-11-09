use opencdi::audio::to_i16;
use opencdi::input_script::{self, Event, Player};
use opencdi::bus::BoardKind;
use opencdi::machine::Machine;
use opencdi::mcd212::{VideoMode, SCREEN_HEIGHT, SCREEN_WIDTH};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

fn video_hash(h: u64, fb: &[u32]) -> u64 {
    let mut h = h;
    for &p in fb {
        h = fnv(h, &p.to_le_bytes());
    }
    h
}

// Hashes the frames the last run_frame made; counts them and those that are not silent.
fn drain_audio(m: &mut Machine, buf: &mut Vec<(f32, f32)>, h: &mut u64, samples: &mut u64, nonzero: &mut u64) {
    buf.clear();
    m.drain_audio(buf);
    for &(l, r) in buf.iter() {
        *h = fnv(*h, &l.to_bits().to_le_bytes());
        *h = fnv(*h, &r.to_bits().to_le_bytes());
        *samples += 1;
        *nonzero += (l != 0.0 || r != 0.0) as u64;
    }
}

fn dump_ppm(fb: &[u32], path: &str) {
    let mut v = format!("P6\n{} {}\n255\n", SCREEN_WIDTH, SCREEN_HEIGHT).into_bytes();
    for &p in fb {
        v.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    std::fs::write(path, v).expect("write ppm");
}

fn usage() -> ! {
    eprintln!("usage: harness <bios.rom> <disc|none> <frames> [--dvc <rom>] [--video-levels full|studio] [--ppm <out.ppm>] [--ntsc] [--autoplay [--print-autoplay-script]] [--input <script|@file>] [--sample <n> [--sample-audio]] [--dump-every <n> <prefix>] [--roundtrip <frame>] [--counters] [--idle-pc <hex>] [--board mono1|mono4] [--audio-out <file.s16>] [--nvram-out <file>]");
    std::process::exit(2);
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        usage();
    }
    let (bios, disc) = (&a[1], &a[2]);
    let frames: u64 = a[3].parse().unwrap_or_else(|_| usage());
    let mut dvc: Option<&String> = None;
    let mut ppm: Option<&String> = None;
    let mut ntsc = false;
    let mut autoplay = false;
    let mut print_script = false;
    let mut input: Option<String> = None;
    let mut sample = 0u64;
    let mut sample_audio = false;
    let mut dump: Option<(u64, String)> = None;
    let mut roundtrip: Option<u64> = None;
    let mut counters = false;
    let mut board: Option<BoardKind> = None;
    let mut idle_pc: Option<u32> = None;
    let mut audio_out: Option<(String, Vec<u8>)> = None;
    let mut nvram_out: Option<&String> = None;
    let mut i = 4;
    while i < a.len() {
        match a[i].as_str() {
            "--dvc" => { i += 1; dvc = a.get(i); }
            "--video-levels" => {
                i += 1;
                match a.get(i).map(|s| s.as_str()) {
                    Some("full") => opencdi::mcd212::set_studio_levels(false),
                    Some("studio") => opencdi::mcd212::set_studio_levels(true),
                    _ => usage(),
                }
            }
            "--ppm" => { i += 1; ppm = a.get(i); }
            "--audio-out" => { i += 1; audio_out = Some((a.get(i).cloned().unwrap_or_else(|| usage()), Vec::new())); }
            "--nvram-out" => { i += 1; nvram_out = Some(a.get(i).unwrap_or_else(|| usage())); }
            "--ntsc" => ntsc = true,
            "--counters" => counters = true,
            "--board" => {
                i += 1;
                board = Some(a.get(i).and_then(|s| BoardKind::parse(s)).unwrap_or_else(|| usage()));
            }
            "--idle-pc" => { i += 1; idle_pc = Some(a.get(i).and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()).unwrap_or_else(|| usage())); }
            "--autoplay" => autoplay = true,
            "--print-autoplay-script" => print_script = true,
            "--input" => { i += 1; input = Some(a.get(i).cloned().unwrap_or_else(|| usage())); }
            "--sample" => { i += 1; sample = a.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| usage()); }
            "--sample-audio" => sample_audio = true,
            "--roundtrip" => { i += 1; roundtrip = Some(a.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| usage())); }
            "--dump-every" => {
                let n = a.get(i + 1).and_then(|s| s.parse().ok()).filter(|&n: &u64| n > 0).unwrap_or_else(|| usage());
                dump = Some((n, a.get(i + 2).cloned().unwrap_or_else(|| usage())));
                i += 2;
            }
            _ => usage(),
        }
        i += 1;
    }
    if (print_script && !autoplay) || (autoplay && input.is_some()) || (sample_audio && sample == 0) || roundtrip.is_some_and(|n| n >= frames) {
        usage();
    }
    let mut player = input.map(|s| {
        let text = match s.strip_prefix('@') {
            Some(f) => std::fs::read_to_string(f).expect("read input script"),
            None => s,
        };
        Player::new(input_script::parse(&text).unwrap_or_else(|e| panic!("input script: {}", e)))
    });
    let mut recorded: Vec<Event> = Vec::new();

    let mut m = Machine::new();
    let report = m.load_bios(bios).unwrap_or_else(|e| panic!("load bios {}: {}", bios, e));
    eprintln!("bios {}: {}", report.status, report.message);
    if let Some(b) = board {
        m.bus.set_board(b);
    }
    eprintln!("board {:?}", m.bus.board);
    if let Some(p) = dvc {
        let report = m.load_dvc(p).unwrap_or_else(|e| panic!("load dvc {}: {}", p, e));
        eprintln!("dvc {}: {}", report.status, report.message);
    }
    if disc != "none" {
        m.load_disc(disc).expect("load disc");
    }
    m.set_video_mode(if ntsc { VideoMode::Ntsc } else { VideoMode::Pal });
    m.reset();

    let mut video_all = FNV_OFFSET;
    let mut audio_all = FNV_OFFSET;
    let mut audio_samples = 0u64;
    let mut audio_nonzero = 0u64;
    let mut disc_launched = false;
    let mut shell_ready_count = 0u32;
    let mut audio_buf: Vec<(f32, f32)> = Vec::new();
    let (mut win_samples, mut win_nonzero) = (0u64, 0u64);
    let mut idle_first: Option<u64> = None;

    // Round trip: save at `at`, run to the end (A), load, rerun the same frames with the same input (B).
    let mut saved: Option<(Vec<u8>, bool, u32, Option<Player>)> = None;
    let mut pass_a: Option<(u64, u64, u64, u64)> = None;
    let mut start = 0;
    'passes: loop {
        for frame in start..frames {
            if roundtrip == Some(frame) && saved.is_none() {
                let mut buf = Vec::new();
                m.save_state(&[], &mut buf);
                saved = Some((buf, disc_launched, shell_ready_count, player.clone()));
                (video_all, audio_all, audio_samples, audio_nonzero) = (FNV_OFFSET, FNV_OFFSET, 0, 0);
            }
            if autoplay && m.bus.cdrom.is_some() && !disc_launched {
                let (bus, cpu) = (&mut m.bus, &m.cpu);
                let shell_state = if bus.dvc.is_fitted() {
                    let s = bus.peek_long(0x00D08090u32.wrapping_add(0xFFFF9172)).unwrap_or(0);
                    if s >= 2 { s } else { bus.peek_long(0x0000C000u32.wrapping_add(0xFFFF9172)).unwrap_or(0) }
                } else {
                    bus.peek_long(0x0000C000u32.wrapping_add(0xFFFF9172)).unwrap_or(0)
                };
                if shell_state >= 2 {
                    shell_ready_count += 1;
                    let btn = match shell_ready_count {
                        1 | 50 | 150 | 200 => Some(0),
                        30 | 180 => Some(1),
                        _ => None,
                    };
                    if let Some(b) = btn {
                        bus.slave.update_mouse(575, 335, b);
                        recorded.push(Event::Pointer { frame, x: 575, y: 335, buttons: b });
                    }
                }
                if ((cpu.pc >= 0x0001_0000 && cpu.pc < 0x0030_0000) || (cpu.pc >= 0x00D0_0000 && cpu.pc < 0x00E0_0000)) && frame > 250 {
                    disc_launched = true;
                    bus.slave.update_mouse(384, 280, 0);
                    recorded.push(Event::Pointer { frame, x: 384, y: 280, buttons: 0 });
                }
            }
            if let Some(p) = player.as_mut() {
                let slave = &mut m.bus.slave;
                if slave.device_mouse_updated {
                    slave.device_mouse_updated = false;
                    p.warp(slave.device_mouse_x, slave.device_mouse_y);
                }
                if let Some((x, y, b)) = p.step(frame) {
                    match m.bus.mono4.as_deref_mut() {
                        Some(m4) => m4.ikat.update_pointer(x, y, b),
                        None => slave.update_mouse(x, y, b),
                    }
                }
            }

            m.run_frame();
            if idle_first.is_none() && idle_pc == Some(m.cpu.pc) {
                idle_first = Some(frame + 1);
            }

            video_all = video_hash(video_all, m.framebuffer());
            drain_audio(&mut m, &mut audio_buf, &mut audio_all, &mut audio_samples, &mut audio_nonzero);
            if let Some((_, bytes)) = audio_out.as_mut() {
                bytes.extend(audio_buf.iter().flat_map(|&(l, r)| [to_i16(l), to_i16(r)]).flat_map(i16::to_le_bytes));
            }
            if sample_audio {
                win_samples += audio_buf.len() as u64;
                win_nonzero += audio_buf.iter().filter(|&&(l, r)| l != 0.0 || r != 0.0).count() as u64;
            }
            if sample > 0 && (frame + 1) % sample == 0 {
                let nb = nonblack_count(m.framebuffer());
                eprintln!("SAMPLE frame={} nonblack={}", frame + 1, nb);
                if sample_audio {
                    eprintln!("AUDIO frame={} samples={} nonzero={}", frame + 1, win_samples, win_nonzero);
                    (win_samples, win_nonzero) = (0, 0);
                }
            }
            if let Some((n, prefix)) = &dump {
                if (frame + 1) % n == 0 {
                    dump_ppm(m.framebuffer(), &format!("{}-{:05}.ppm", prefix, frame + 1));
                }
            }
        }
        if let (Some(at), Some((buf, dl, src, pl))) = (roundtrip, saved.as_ref()) {
            let now = (video_all, video_hash(FNV_OFFSET, m.framebuffer()), audio_all, audio_samples);
            match pass_a {
                None => {
                    pass_a = Some(now);
                    m.load_state(buf).expect("load_state");
                    let mut again = Vec::new();
                    m.save_state(&[], &mut again);
                    eprintln!("ROUNDTRIP-RESAVE {} size={}", if &again == buf { "same" } else { "DIFF" }, buf.len());
                    (disc_launched, shell_ready_count, player) = (*dl, *src, pl.clone());
                    (video_all, audio_all, audio_samples, audio_nonzero) = (FNV_OFFSET, FNV_OFFSET, 0, 0);
                    start = at;
                    continue 'passes;
                }
                Some(a) => {
                    eprintln!("ROUNDTRIP-DETAIL a={:016x?} b={:016x?}", a, now);
                    println!(
                        "ROUNDTRIP at={} frames={} video={} audio={}",
                        at, frames - at,
                        if (a.0, a.1) == (now.0, now.1) { "match" } else { "DIFFER" },
                        if (a.2, a.3) == (now.2, now.3) { "match" } else { "DIFFER" }
                    );
                    return;
                }
            }
        }
        break;
    }

    let video_last = video_hash(FNV_OFFSET, m.framebuffer());
    let nonblack = nonblack_count(m.framebuffer());
    if let Some(p) = ppm {
        dump_ppm(m.framebuffer(), p);
    }
    if let Some((p, bytes)) = &audio_out {
        if let Err(e) = std::fs::write(p, bytes) {
            eprintln!("cannot write audio {}: {}", p, e);
            std::process::exit(1);
        }
    }
    if let Some(p) = nvram_out {
        if let Err(e) = std::fs::write(p, &m.bus.nvram.ram[..]) {
            eprintln!("cannot write nvram {}: {}", p, e);
            std::process::exit(1);
        }
    }
    println!(
        "\nHARNESS frames={} pc={:08x} nonblack={} video_last={:016x} video_all={:016x} audio={:016x} audio_samples={} audio_nonzero={}",
        frames, m.cpu.pc, nonblack, video_last, video_all, audio_all, audio_samples, audio_nonzero
    );
    if counters || idle_pc.is_some() {
        let idle = idle_first.map_or("none".to_string(), |f| f.to_string());
        let ciap = m.bus.mono4.as_deref().map_or(String::new(), |m4| {
            let c = &m4.ciap;
            format!(" buserr={} seeks={} sectors={} skipped={} dmas={} descrambled={} q={} abuf={} asec={} aover={}", m.bus.counters.bus_errors, c.seeks, c.sectors, c.skipped, c.dmas, c.descrambled, c.q_reports, c.audio_buffers, c.audio_sectors, c.audio_overlap)
        });
        println!("COUNTERS {}{} idle_first={}", m.bus.counters.summary(), ciap, idle);
    }
    if print_script {
        println!("AUTOPLAY_SCRIPT {}", input_script::format(&recorded));
    }
}

// Pixels with any channel above the black level of the selected output range (16 studio, 0 full).
fn nonblack_count(fb: &[u32]) -> usize {
    let black = if opencdi::mcd212::studio_levels() { 16 } else { 0 };
    fb.iter().filter(|&&p| [16, 8, 0].iter().any(|&s| (p >> s) & 0xFF > black)).count()
}

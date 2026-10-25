use opencdi::input_script::{self, Event, Player};
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

fn drain_audio(m: &mut Machine, buf: &mut Vec<(f32, f32)>, h: &mut u64, samples: &mut u64) {
    buf.clear();
    let rate = m.drain_audio(buf);
    for &(l, r) in buf.iter() {
        *h = fnv(*h, &l.to_bits().to_le_bytes());
        *h = fnv(*h, &r.to_bits().to_le_bytes());
        *samples += 1;
    }
    *h = fnv(*h, &rate.to_le_bytes());
}

fn dump_ppm(fb: &[u32], path: &str) {
    let mut v = format!("P6\n{} {}\n255\n", SCREEN_WIDTH, SCREEN_HEIGHT).into_bytes();
    for &p in fb {
        v.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    std::fs::write(path, v).expect("write ppm");
}

fn usage() -> ! {
    eprintln!("usage: harness <bios.rom> <disc|none> <frames> [--dvc <rom>] [--ppm <out.ppm>] [--ntsc] [--autoplay [--print-autoplay-script]] [--input <script|@file>] [--sample <n> [--sample-audio]] [--dump-every <n> <prefix>]");
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
    let mut i = 4;
    while i < a.len() {
        match a[i].as_str() {
            "--dvc" => { i += 1; dvc = a.get(i); }
            "--ppm" => { i += 1; ppm = a.get(i); }
            "--ntsc" => ntsc = true,
            "--autoplay" => autoplay = true,
            "--print-autoplay-script" => print_script = true,
            "--input" => { i += 1; input = Some(a.get(i).cloned().unwrap_or_else(|| usage())); }
            "--sample" => { i += 1; sample = a.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| usage()); }
            "--sample-audio" => sample_audio = true,
            "--dump-every" => {
                let n = a.get(i + 1).and_then(|s| s.parse().ok()).filter(|&n: &u64| n > 0).unwrap_or_else(|| usage());
                dump = Some((n, a.get(i + 2).cloned().unwrap_or_else(|| usage())));
                i += 2;
            }
            _ => usage(),
        }
        i += 1;
    }
    if (print_script && !autoplay) || (autoplay && input.is_some()) || (sample_audio && sample == 0) {
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
    m.load_bios(bios).unwrap_or_else(|e| panic!("load bios {}: {}", bios, e));
    if let Some(p) = dvc {
        m.load_dvc_rom(p).unwrap_or_else(|e| panic!("load dvc {}: {}", p, e));
    }
    if disc != "none" {
        m.load_disc(disc).expect("load disc");
    }
    m.set_video_mode(if ntsc { VideoMode::Ntsc } else { VideoMode::Pal });
    m.reset();

    let mut video_all = FNV_OFFSET;
    let mut audio_all = FNV_OFFSET;
    let mut audio_samples = 0u64;
    let mut disc_launched = false;
    let mut shell_ready_count = 0u32;
    let mut audio_buf: Vec<(f32, f32)> = Vec::new();
    let (mut win_samples, mut win_nonzero) = (0u64, 0u64);

    {
        for frame in 0..frames {
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
                    slave.update_mouse(x, y, b);
                }
            }

            m.run_frame();

            video_all = video_hash(video_all, m.framebuffer());
            drain_audio(&mut m, &mut audio_buf, &mut audio_all, &mut audio_samples);
            if sample_audio {
                win_samples += audio_buf.len() as u64;
                win_nonzero += audio_buf.iter().filter(|&&(l, r)| l != 0.0 || r != 0.0).count() as u64;
            }
            if sample > 0 && (frame + 1) % sample == 0 {
                let nb = m.framebuffer().iter().filter(|&&p| p & 0x00FF_FFFF != 0).count();
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
    }

    let video_last = video_hash(FNV_OFFSET, m.framebuffer());
    let nonblack = m.framebuffer().iter().filter(|&&p| p & 0x00FF_FFFF != 0).count();
    if let Some(p) = ppm {
        dump_ppm(m.framebuffer(), p);
    }
    println!(
        "\nHARNESS frames={} pc={:08x} nonblack={} video_last={:016x} video_all={:016x} audio={:016x} audio_samples={}",
        frames, m.cpu.pc, nonblack, video_last, video_all, audio_all, audio_samples
    );
    if print_script {
        println!("AUTOPLAY_SCRIPT {}", input_script::format(&recorded));
    }
}

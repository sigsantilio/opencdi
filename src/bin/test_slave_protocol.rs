#[path = "../audio.rs"]
mod audio;
#[path = "../bus.rs"]
mod bus;
#[path = "../cdic.rs"]
mod cdic;
#[path = "../cdrom.rs"]
mod cdrom;
#[path = "../dma.rs"]
mod dma;
#[path = "../dvc.rs"]
mod dvc;
#[path = "../mcd212.rs"]
mod mcd212;
#[path = "../nvram.rs"]
mod nvram;
#[path = "../scc_timer.rs"]
mod scc_timer;
#[path = "../slave.rs"]
mod slave;

use crate::bus::CdiBus;

fn expect(cond: bool, name: impl std::fmt::Display, detail: String) {
    if cond {
        println!("[PASS] {}", name);
    } else {
        eprintln!("[FAIL] {}: {}", name, detail);
        std::process::exit(1);
    }
}

fn encode_c0_write_packet(x: i32, y: i32) -> [u8; 3] {
    let xx = x.max(0).min(767);
    let yy = y.max(0).min(559);
    [
        (0xC0 | ((yy & 0x3F) as u8)),
        ((((xx >> 7) & 0x07) << 4) as u8) | (((yy >> 6) & 0x0F) as u8),
        (xx & 0x7F) as u8,
    ]
}

fn write_ch0_packet(bus: &mut CdiBus, packet: &[u8]) {
    for &b in packet {
        bus.slave.write(0, b as u16);
    }
}

fn read_ch0_byte(bus: &mut CdiBus) -> u8 {
    (bus.slave.read(0) & 0xFF) as u8
}

fn expected_mouse_packet(x: i32, y: i32, btn: u8) -> [u8; 4] {
    let x = x.clamp(0, 767);
    let y = y.clamp(0, 559);
    let mut button_bits = 0x01u8;
    if (btn & 1) != 0 { button_bits |= 0x02; }
    if (btn & 2) != 0 { button_bits |= 0x04; }
    if (btn & 4) != 0 { button_bits |= 0x06; }
    let b = button_bits << 3;
    [
        (((x & 0x380) >> 7) as u8) | b,
        (x & 0x7F) as u8,
        ((y & 0x380) >> 7) as u8,
        (y & 0x7F) as u8,
    ]
}

fn main() {
    println!("=== OpenCDi - Test Protocollo Slave XBUS (CH0 mouse) ===");

    // ---- Gruppo 1: wargame-side "Update Mouse Position" (C0), decodifica fedele MAME
    {
        let mut bus = CdiBus::new();

        let p1 = encode_c0_write_packet(100, 50);
        write_ch0_packet(&mut bus, &p1);
        expect(bus.slave.device_mouse_updated && bus.slave.device_mouse_x == 100 && bus.slave.device_mouse_y == 50,
            "warp pacchetto 1 decodifica (100,50)",
            format!("updated={} device=({}, {})", bus.slave.device_mouse_updated, bus.slave.device_mouse_x, bus.slave.device_mouse_y));

        bus.slave.device_mouse_updated = false;

        let p2 = encode_c0_write_packet(400, 100);
        write_ch0_packet(&mut bus, &p2);
        expect(bus.slave.device_mouse_updated && bus.slave.device_mouse_x == 400 && bus.slave.device_mouse_y == 100,
            "warp pacchetto 2 decodifica esatta da pacchetto pieno (400,100), nessun byte stante",
            format!("updated={} device=({}, {})", bus.slave.device_mouse_updated, bus.slave.device_mouse_x, bus.slave.device_mouse_y));

        bus.slave.device_mouse_updated = false;

        bus.slave.write(0, p2[0] as u16);
        expect(!bus.slave.device_mouse_updated,
            "1 byte C0 isolato: nessuna decodifica prematura",
            format!("updated={}", bus.slave.device_mouse_updated));
        expect(bus.slave.device_mouse_x == 400 && bus.slave.device_mouse_y == 100,
            "1 byte C0 isolato: coordinate invariate",
            format!("device=({}, {})", bus.slave.device_mouse_x, bus.slave.device_mouse_y));
    }

    // ---- Gruppo 2: streaming update_mouse -> pacchetto esatto, padding 0xFF dopo fine
    {
        let mut bus = CdiBus::new();
        bus.slave.update_mouse(300, 280, 0);
        expect(bus.slave.irq_pending, "update_mouse: IRQ2 pending a pacchetto pronto", format!("irq_pending={}", bus.slave.irq_pending));

        let pk = expected_mouse_packet(300, 280, 0);
        for (i, &want) in pk.iter().enumerate() {
            let got = read_ch0_byte(&mut bus);
            expect(got == want, format!("stream byte {} == 0x{:02X} (inv 0x{:02X})", i, got, want), format!("got=0x{:02X} want=0x{:02X}", want, got));
        }
        let tail = read_ch0_byte(&mut bus);
        expect(tail == 0xFF, "lettura oltre pacchetto -> 0xFF", format!("got=0x{:02X}", tail));
        expect(!bus.slave.irq_pending, "IRQ2 deassert dopo lettura primo byte/consumo", format!("irq_pending={}", bus.slave.irq_pending));
    }

    // ---- Gruppo 3: niente mix di byte quando un pacchetto arriva a lettura in corso
    {
        let mut bus = CdiBus::new();
        bus.slave.update_mouse(300, 280, 0);
        let p300 = expected_mouse_packet(300, 280, 0);
        let b0 = read_ch0_byte(&mut bus);
        expect(b0 == p300[0], "clobber: primo byte pacchetto 300", format!("got=0x{:02X} want=0x{:02X}", b0, p300[0]));

        bus.slave.update_mouse(340, 280, 0);
        expect(!bus.slave.irq_pending, "pacchetto in stage durante lettura: IRQ2 non riassegnata", format!("irq_pending={}", bus.slave.irq_pending));

        for (i, &want) in p300[1..4].iter().enumerate() {
            let got = read_ch0_byte(&mut bus);
            expect(got == want, format!("clobber: byte pacchetto 300 inalterato (pos {})", i + 1), format!("got=0x{:02X} want=0x{:02X}", got, want));
        }
        expect(bus.slave.irq_pending, "promozione a fine lettura: IRQ2 riassegnata", format!("irq_pending={}", bus.slave.irq_pending));

        let p340 = expected_mouse_packet(340, 280, 0);
        for (i, &want) in p340.iter().enumerate() {
            let got = read_ch0_byte(&mut bus);
            expect(got == want, format!("clobber: stream promosso usa pacchetto 340 integro (pos {})", i), format!("got=0x{:02X} want=0x{:02X}", got, want));
        }
    }

    // ---- Gruppo 4: last-wins se piu' pacchetti in attesa durante la stessa lettura
    {
        let mut bus = CdiBus::new();
        bus.slave.update_mouse(300, 280, 0);
        let p300 = expected_mouse_packet(300, 280, 0);
        let b0 = read_ch0_byte(&mut bus);
        expect(b0 == p300[0], "last-wins: primo byte pacchetto 300", format!("got=0x{:02X} want=0x{:02X}", b0, p300[0]));

        bus.slave.update_mouse(340, 280, 0);
        bus.slave.update_mouse(380, 280, 0);
        for (i, &want) in p300[1..4].iter().enumerate() {
            let got = read_ch0_byte(&mut bus);
            expect(got == want, format!("last-wins: coda pacchetto 300 senza mix (pos {})", i + 1), format!("got=0x{:02X} want=0x{:02X}", got, want));
        }
        let p380 = expected_mouse_packet(380, 280, 0);
        for (i, &want) in p380.iter().enumerate() {
            let got = read_ch0_byte(&mut bus);
            expect(got == want, format!("last-wins: promosso l'ultimo pacchetto 380 (pos {})", i), format!("got=0x{:02X} want=0x{:02X}", got, want));
        }
    }

    // ---- Gruppo 5: bottone nel pacchetto in streaming (bit set MAME)
    {
        let mut bus = CdiBus::new();
        bus.slave.update_mouse(575, 335, 1);
        let pk = expected_mouse_packet(575, 335, 1);
        for (i, &want) in pk.iter().enumerate() {
            let got = read_ch0_byte(&mut bus);
            expect(got == want, format!("bottone: byte {} == 0x{:02X}", i, want), format!("got=0x{:02X} want=0x{:02X}", got, want));
        }
    }

    // ---- Gruppo 6: salvaguardia click rapido (down poi up prima della lettura del byte 0)
    {
        let mut bus = CdiBus::new();
        // Click DOWN
        bus.slave.update_mouse(400, 250, 1);
        // Subito dopo Click UP (rilascio) prima che la CPU abbia letto il byte 0!
        bus.slave.update_mouse(400, 250, 0);

        // Il primo pacchetto letto DEVE contenere il click DOWN (btn = 1)
        let p_down = expected_mouse_packet(400, 250, 1);
        for (i, &want) in p_down.iter().enumerate() {
            let got = read_ch0_byte(&mut bus);
            expect(got == want, format!("click rapido preservato: byte {} del click == 0x{:02X}", i, want), format!("got=0x{:02X} want=0x{:02X}", want, got));
        }

        // Il secondo pacchetto promosso DEVE contenere il rilascio UP (btn = 0)
        let p_up = expected_mouse_packet(400, 250, 0);
        for (i, &want) in p_up.iter().enumerate() {
            let got = read_ch0_byte(&mut bus);
            expect(got == want, format!("rilascio successivo preservato: byte {} del rilascio == 0x{:02X}", i, want), format!("got=0x{:02X} want=0x{:02X}", want, got));
        }
    }

    // ---- Gruppo 7: reattivita sparo/click immediato (movimento mouse poi click prima della lettura)
    {
        let mut bus = CdiBus::new();
        // Mouse si muove (senza click)
        bus.slave.update_mouse(300, 200, 0);
        // Subito dopo arriva lo sparo (click DOWN btn=1) prima che la CPU abbia letto il byte 0
        bus.slave.update_mouse(300, 200, 1);

        // Il pacchetto letto DEVE contenere immediatamente il click (btn=1), senza ritardi!
        let p_shot = expected_mouse_packet(300, 200, 1);
        for (i, &want) in p_shot.iter().enumerate() {
            let got = read_ch0_byte(&mut bus);
            expect(got == want, format!("sparo immediato: byte {} ha il click == 0x{:02X}", i, want), format!("got=0x{:02X} want=0x{:02X}", want, got));
        }
    }

    println!("=== Test protocollo slave completato senza errori ===");
}

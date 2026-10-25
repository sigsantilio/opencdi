# OpenCDi

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Language: Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)

[Versione Italiana](README_IT.md)

OpenCDi is a modern, standalone, open-source **Philips CD-i** emulator written in **Rust**.
The goal of the project is to accurately preserve and run the software and multimedia library of the Philips CD-i system with a clean, modular, and fast architecture.

![OpenCDi Showcase](opencdi_showcase_v016.png)

---

## Architecture & Features

- **CPU:** Philips SCC68070 (full 68000/68010 instruction set, autovectored interrupts, on-chip DMA channels, timers, and UART).
- **Video:** Philips MCD-212 Video Display Controller:
  - Dual-plane rendering (Plane A & Plane B) with hardware backdrop.
  - Video coding methods: CLUT4, CLUT7, CLUT8, Run-Length (7-bit and 8-bit), DYUV (Delta-YUV), and RGB555.
  - Line-by-line Instruction Control Area (ICA) and Dynamic Control Area (DCA) display lists execution.
- **CD-ROM & Audio:**
  - CDIC (CD-i Controller) with sector processing for Mode 2 Form 1 and Form 2.
  - Hardware CD-DA and CD-i ADPCM audio decoding (Level A, B, and C, stereo and mono).
  - Native support for compressed CHD disc images (`chd-rs`) and raw 2352-byte sector images.
- **Slave Microcontroller (68HC05 HLE):**
  - High-Level Emulation for CD-i keyboard, mouse, and gamepads.
  - Real-Time Clock (RTC) and Non-Volatile RAM (NVRAM) state persistence.
- **Digital Video Cartridge (DVC / MPEG-1):**
  - Hardware FMV (Full Motion Video) cartridge emulation.
  - Integrated MPEG-1 system stream demuxer (video and audio packets).
  - Video decoder with YUV-to-RGB planar compositing onto the MCD-212 external video plane.
  - Accurate presentation pacing and end-of-sequence stream drain detection.
- **Frontend GUI:**
  - Interactive 50/60 Hz display window powered by `minifb`.
  - Configurable keyboard, mouse, and gamepad controls.

---

## Verified Game Compatibility

| Game | Graphics / DVC Mode | Current Status |
|---|---|---|
| **Hotel Mario** | MCD-212 / CDIC ADPCM | Fully playable in-game with complete animations and sound |
| **Striker Pro** | MCD-212 / CDIC ADPCM | Menus and in-game match playable with synchronized audio |
| **Dragon's Lair (USA)** | DVC FMV MPEG-1 | Continuous attract mode and interactive gameplay with active MPEG video |
| **The 7th Guest** | DVC FMV MPEG-1 | Boots, plays intro cutscenes, and advances through scenes |
| **Lost Eden** | DVC FMV MPEG-1 / CDIC | Boots, cutscenes, character dialogue, and exploration |
| **Rise of the Robots** | MCD-212 / DVC | Boots and plays intro sequences |
| **Burn:Cycle** | DVC FMV MPEG-1 | Boots and plays video sequences |

---

## Building and Running

### Prerequisites
- [Rust & Cargo](https://rustup.rs/) (version 1.70 or newer).

### Building from Source
```bash
git clone https://github.com/sigsantilio/opencdi.git
cd opencdi
cargo build --release
```

The optimized binary will be built in `target/release/opencdi.exe`.

On Linux the same command builds `target/release/opencdi` (needs the ALSA and udev development packages; the window uses X11 or Wayland).

## System BIOS & Required Files Setup

OpenCDi is a low-level hardware emulator that executes authentic Philips CD-i firmware to initialize the OS-9 operating system kernel, bus controllers, and system drivers. Due to copyright restrictions, **these proprietary system ROMs are not bundled with the emulator and must be provided by the user.**

### 1. System ROMs Specification Table

| Component | Recommended Filename | Supported Alternatives | Exact Size (Bytes) | MD5 Checksum | SHA-1 Checksum | Status |
|---|---|---|---|---|---|---|
| **System BIOS** | `cdi220b.rom` | `cdi220.rom`, `cdi200.rom`, `cdi490a.rom` | 524,288 (512 KB) | `ac0d468be366779c9df633be98da250a` | `53360a1f21ddac952e95306ced64186a3fc0b93e` | **Mandatory** |
| **Digital Video Cartridge (DVC)** | `vmpega.rom` | `impega.rom` | 262,144 (256 KB) | `53ba3bf7443187a657baa272d01d3771` | `be407fbc102f1731a0862554855e963e5a47c17b` | **Required for FMV / VCD** |
| **Timekeeper NVRAM** | `nvram_mk48t08.bin` | `nvram.bin` | 8,192 (8 KB) | `45ed531aec0941ddf57af8f00c71bed1` | `601d780e1544985352535143705ffcc670d282d1` | *Optional (Auto-created)* |

#### Hardware Roles & Details:
- **System BIOS (`cdi220b.rom`):**
  - **What it is:** The official European PAL Mono-board system ROM for the Philips CD-i 220/205/210 player.
  - **Why it is required:** Contains the OS-9 68070 real-time operating system kernel, hardware drivers for the MCD-212 video controller, CDIC disc controller, Slave 68HC05 microcontroller, system fonts, and the Philips player dashboard.
  - **Behavior if missing:** OpenCDi will display an informative GUI error dialog indicating the missing file and required checksums, then exit cleanly.
- **Digital Video Cartridge ROM (`vmpega.rom`):**
  - **What it is:** The firmware ROM for the Philips 22ER9956 Digital Video Cartridge (CL450 MPEG-1 decoder chip).
  - **Why it is required:** Enables hardware MPEG-1 video/audio decoding for Full Motion Video (FMV) games and Video CDs (VCDs).
  - **Titles that require DVC:** *The 7th Guest*, *Dragon's Lair*, *Space Ace*, *Lost Eden*, *Burn:Cycle*, *Rise of the Robots*, *Mad Dog McCree*, *Mutant Rampage: Bodyslam*, *Ultra CD-i Soccer*, and Video CD movies.
  - **Titles that run without DVC (Base CD-i Mode):** *Hotel Mario*, *Striker Pro*, *Lemmings*, *Tetris*, *Zelda: The Wand of Gamelon*, *Link: The Faces of Evil*, *Asterix*, *Defender of the Crown*, etc.
- **Timekeeper NVRAM (`nvram_mk48t08.bin`):**
  - **What it is:** 8 KB non-volatile battery-backed SRAM (MK48T08) storing console configuration, player clock/calendar, high scores, and game progress.
  - **Automatic creation:** If not present, OpenCDi will automatically initialize a clean NVRAM state and persist your game saves to disk.

### 2. Recommended Directory Layout

OpenCDi searches for system ROMs in multiple standard locations. You can place the files in a clean `bios/` subfolder or directly alongside `opencdi.exe`:

```
opencdi/
├── opencdi.exe
├── opencdi_controls.ini       (Auto-generated on first launch)
├── bios/
│   ├── cdi220b.rom            <-- Place System BIOS here (Required)
│   ├── vmpega.rom             <-- Place DVC Cartridge ROM here (Required for FMV)
│   └── nvram_mk48t08.bin      <-- Saves & console settings (Optional / Auto-created)
└── games/
    ├── Hotel Mario (Europe).chd
    └── The 7th Guest (USA).chd
```

*Note: Placing `cdi220b.rom` and `vmpega.rom` directly in the root folder alongside `opencdi.exe`, or inside `dvc/` for `vmpega.rom`, is also fully supported.*

### 3. Supported Disc Image Formats

- **CHD (`.chd`):** Highly recommended. OpenCDi natively decodes compressed MAME CHD disc images with Mode 2 Form 1/2 sectors and CD-DA tracks without decompression delay.
- **ISO (`.iso`):** Standard 2048-byte sector data images.
- **BIN/CUE (`.bin`, `.cue`):** Raw 2352-byte sector images with redbook audio tracks.

### 4. Running a Game
```bash
# Launch a game disc image
opencdi.exe "path/to/game.chd"

# Boot into Standalone BIOS Shell (no disc)
opencdi.exe --bios

# Force Base CD-i Mode without DVC extension
opencdi.exe --no-dvc "path/to/game.chd"
```
Or simply drag and drop any `.chd` CD-i disc image directly onto `opencdi.exe`.

Other options: `--rom <path>` (system BIOS), `--frames <n>` (quit after n frames), `--mouse` (start in mouse mode), `--no-host-input` (ignore keyboard, mouse and gamepads), `--recent [n]` (list the recent disc images or boot number n; on Windows they are also under File, Recent Disc Images), `--help` (all options). Shortcuts: F11 fullscreen, F5 reset, P pause, Space fast-forward, F8 screenshot, Esc quit.

---

## Documentation & Compatibility

- **Compatibility List:** [`COMPATIBILITY.md`](COMPATIBILITY.md)
- **Known Issues & Resolution Plan:** [`KNOWN_ISSUES_AND_TODO.md`](KNOWN_ISSUES_AND_TODO.md)

---

## Contributing

This project is driven by a passion for CD-i preservation and hardware research.
All contributions are warmly welcome:
- Bug reports and game compatibility testing.
- Pull requests improving CPU timing, peripheral registers, or decoder accuracy.
- Hardware traces and documentation from real CD-i units.

---

## Acknowledgments & Third-Party Licenses

This project benefits greatly from the prior work, documentation, and dedication of the digital preservation community:

### MAME (MAMEDev)
Special acknowledgment and credit to the **MAME development team and contributors** (including Harmony, Ryan Holtz, Pete Bernert, and the CD-i driver authors). Portions of the hardware register architecture, CD-i subsystem behavior, and hardware timing research in OpenCDi draw reference from MAME's Philips CD-i drivers, which are licensed under the **BSD 3-Clause License**:
- `src/mame/philips/cdi.cpp`
- `src/mame/philips/mcd212.cpp`
- `src/mame/philips/cdicdic.cpp`
- `src/devices/cpu/m68000/scc68070.cpp` (and `scc68070.cpp`)

*MAME is distributed under the terms of the GNU General Public License v2 or later (GPL-2.0+), with individual driver files licensed under permissive terms such as BSD-3-Clause. Copyright © 1997-2026 MAMEDev and contributors.*

### CDi_MiSTer
Special thanks to the authors and contributors of the **CDi_MiSTer** project (including ash2020 and the MiSTer FPGA community). Their FPGA hardware research, reverse-engineering of CD-i ASICs, bus cycle analysis, and hardware documentation have been instrumental in verifying low-level behaviors of real CD-i hardware.

### The CD-i Preservation Community
Heartfelt thanks to the broader CD-i community for ongoing preservation efforts, testing, and documentation of the CD-i hardware and software library.

---

## License

Distributed under the **MIT License**. See [`LICENSE`](LICENSE) for details.

# OpenCDi

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Language: Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)

[Versione Italiana](README_IT.md)

OpenCDi is a modern, standalone, open-source **Philips CD-i** emulator written in **Rust**.
The goal of the project is to accurately preserve and run the complete software and multimedia library of the Philips CD-i system with a clean, fast, and modular architecture.

![OpenCDi Showcase](opencdi_showcase_v016.png)

---

## What's New in Version 0.3.0

- **Libretro Core (`crates/opencdi-libretro`)**: OpenCDi can now be built as a native core for **RetroArch** (`opencdi_libretro.dll` / `.so`), featuring zero-lag Runahead and deterministic Rewind support.
- **Mono-IV Board Architecture**: Experimental emulation of the **Philips CD-i 490 (Mono-IV)** board, including CIAP (CD-i Audio/Packet), IKAT I/O co-processor, and Dallas DS1216 real-time clock.
- **Runtime BIOS & DVC Selection**: Switch BIOS ROMs on the fly via the menu or `Ctrl+B`, with automatic ROM detection, zip set unpacking (e.g. `cdimono1.zip`), and config persistence in `opencdi.ini`.
- **Interactive Missing-BIOS Assistant**: Built-in visual guide if no system ROM is found, allowing users to browse PC files or open the `bios/` folder in Windows Explorer with clear instructions.
- **Modular In-Memory Savestate Engine**: High-speed, trait-based savestate serialization supporting 1000 slots (`F2` save, `F4` load, `F6`/`F7` slot select), covering Mono-I, Mono-IV, and DVC MPEG states.
- **High-Definition PNG Screenshots**: `F8` and `F12` hotkeys capture uncompressed PNG screenshots with game name and frame counter.
- **Exclusive Gamepad / Mouse Isolation**: Strict separation between Gamepad mode (`F9`) and Mouse/Lightgun mode for precision rail shooters like *Mad Dog McCree*, *Mad Dog II*, and *Drug Wars*.
- **Modern Display Pipeline**: Cross-platform windowing backed by `winit` and `softbuffer`, with Borderless Fullscreen (`F11` / `Alt+Enter`) and native aspect ratio preservation.

---

## Architecture & Hardware Features

- **CPU:** Philips SCC68070 (full 68000/68010 instruction set, autovectored interrupts, on-chip DMA channels, timers, and UART at 15 MHz).
- **Video:** Philips MCD-212 Video Display Controller:
  - Dual-plane rendering (Plane A & Plane B) with hardware backdrop.
  - Video coding methods: CLUT4, CLUT7, CLUT8, Run-Length (7-bit and 8-bit), DYUV (Delta-YUV), and RGB555.
  - Line-by-line Instruction Control Area (ICA) and Dynamic Control Area (DCA) display lists execution.
  - Runtime toggle for studio (black 16) and full-range video levels (`L` key).
- **CD-ROM & Audio:**
  - CDIC (CD-i Controller) with sector processing for Mode 2 Form 1 and Form 2.
  - Hardware CD-DA and CD-i ADPCM audio decoding (Level A, B, and C, stereo and mono).
  - Native support for compressed CHD disc images (`chd-rs`), raw CUE/BIN, and standard ISO formats.
- **Slave Microcontroller (68HC05 HLE):**
  - High-Level Emulation for CD-i keyboard, mouse, Peacekeeper lightguns, and gamepads.
  - Real-Time Clock (RTC) and Non-Volatile RAM (NVRAM) state persistence (MK48T08 / Dallas DS1216).
- **Digital Video Cartridge (DVC / MPEG-1):**
  - Hardware FMV (Full Motion Video) cartridge emulation for VMPEG and IMPEG.
  - Integrated MPEG-1 system stream demuxer (video and audio packets).
  - Video decoder with YUV-to-RGB planar compositing onto the MCD-212 external video plane.
- **Input System & Controls:**
  - `F9` toggle: instantly switch between CD-i Gamepad (USB Controller / Keyboard) and Philips CD-i Mouse / Lightgun.
  - Native USB gamepad support (Xbox, PlayStation, generic XInput / DirectInput controllers via `gilrs`).
  - Adjustable mouse sensitivity (25% - 150%) via Controls menu.

---

## Controls and Keyboard Shortcuts

| Key | Action |
|---|---|
| **F1** | Controls & Keyboard Shortcuts Guide |
| **F2** | Save State into current slot |
| **F4** | Load State from current slot |
| **F6 / F7** | Previous / Next Save State Slot (0 - 999) |
| **F5 / H** | Reset Console |
| **F8 / F12** | Take Screenshot (High-Definition PNG) |
| **F9** | Switch Input Device (Gamepad <-> Mouse) |
| **F11 / Alt+Enter** | Toggle Borderless Fullscreen |
| **P** | Pause / Resume Emulation |
| **Space (hold)** | Fast-Forward |
| **L** | Toggle Video Levels (Studio Range / Full Range) |
| **M** | Mute / Unmute Audio |
| **+ / -** | Volume Up / Volume Down |
| **Ctrl + I** | Insert Disc (Hot-Swap) |
| **Ctrl + O** | Open Disc and Reset Console |
| **Ctrl + B** | Select System BIOS ROM or ZIP |
| **Middle Click** | Capture / Release Mouse Cursor |
| **Esc** | Quit Emulator |

---

## Verified Game Compatibility

| Game | Graphics / DVC Mode | Current Status |
|---|---|---|
| **Hotel Mario** | MCD-212 / CDIC ADPCM | Fully playable in-game with complete animations and sound |
| **The Apprentice** | MCD-212 / CDIC ADPCM | Fully playable at 50 fps, smooth scrolling and audio |
| **Dragon's Lair (USA)** | DVC FMV MPEG-1 | Continuous attract mode and interactive gameplay with active MPEG video |
| **Mad Dog McCree / Mad Dog II** | DVC MPEG-1 / Slave Lightgun | Fully playable with mouse/gun, responsive crosshair, synchronized FMV |
| **Drug Wars** | MCD-212 / Slave Lightgun | Responsive crosshair and shooting mechanics |
| **Lost Eden** | DVC FMV MPEG-1 / CDIC | Boots, cutscenes with synchronized speech and exploration |
| **The 7th Guest** | DVC FMV MPEG-1 | Intro, puzzles, and cutscene transitions |
| **Striker Pro** | MCD-212 / CDIC ADPCM | Menus and in-game match playable with synchronized audio |
| **Chaos Control** | DVC FMV MPEG-1 | Full video sequences and gameplay with DVC |
| **Creature Shock** | DVC FMV MPEG-1 | Multi-disc hot-swap and active cutscenes |
| **Earth Command** | MCD-212 NTSC | Auto-detects NTSC video mode and runs smoothly |

---

## Building and Running

### Prerequisites
- [Rust & Cargo](https://rustup.rs/) (version 1.93 or newer).

### Building Standalone Executable
```bash
git clone https://github.com/sigsantilio/opencdi.git
cd opencdi
cargo build --release --bin opencdi
```
The optimized executable will be built in `target/release/opencdi.exe`.

### Building Libretro Core (for RetroArch)
```bash
cargo build --release -p opencdi-libretro
```
The dynamic library `opencdi_libretro.dll` (or `.so` on Linux) will be generated in `target/release/`.

---

---

## Special Acknowledgments

Heartfelt thanks to everyone who contributed to the development and preservation goals of OpenCDi:
- **ArnoldDeRuiter**: for the outstanding and extensive engineering work, Libretro core implementation, in-memory deterministic savestates, Mono-IV chip models (CIAP, IKAT), regression harness, and countless timing and audio refinements.
- **The MAME / MESS Team**: for their invaluable documentation and foundational work on Philips and Motorola custom chips (SCC68070, MCD212, CDIC, Slave 68HC05).
- **The World of CD-i** and the broader CD-i preservation community for their dedication, technical insights, and continuous support.
- All testers, players, and contributors who helped test games and provided feedback.

---

## License

Distributed under the **MIT License**. See [LICENSE](LICENSE) for details.

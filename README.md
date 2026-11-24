# OpenCDi

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Language: Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)

[Versione Italiana](README_IT.md)

OpenCDi is a modern, standalone, open-source **Philips CD-i** emulator written in **Rust**.
The goal of the project is to accurately preserve and run the complete software and multimedia library of the Philips CD-i system with a clean, fast, and modular architecture.

![OpenCDi](opencdi_banner.png)

---

## What's New in Version 0.4.2

- **MCD-212 External Video (`0xC0` `ICM_EV`) Accuracy**:
  - Restricted External Video enable strictly to Register `0xC0` bit 18 (`ICM_EV`) per the official Motorola MCD212 datasheet, ignoring reserved bits 4 & 12 so stale FMV frames no longer bleed through subsequent screens (fixes *Lucky Luke* boot screen overlap, #89).
- **DVC CL450 Display PRPA Offset**:
  - Preserved decoded picture presentation pointer `PRPA` at DRAM byte `0x56` while keeping Frame Rate Ratio (`1800`) at byte `0xAC`, fixing *BrainDead 13* stopping after the Philips intro (fixes #94) and unskipped FMV end-signal detection (*Lucky Luke*).
- **CD-i Ready & CHD Track Mapping**:
  - Fixed CD-i Ready TOC `disc_type = 0x00` and CHD track alignment so track 1 is no longer skipped in *Steel Machine* (fixes #84) and *The Apprentice* (fixes #88, #92).
- **Optional Motion-Adaptive FMV De-Interlacing**:
  - Added presentation-only motion-adaptive FMV de-interlacing configurable in both standalone and Libretro Core Options (fixes #81).
- **Exclusive Fullscreen & Configurable Audio Latency**:
  - Added Exclusive Fullscreen mode for lower display latency (fixes #80) and Low / Medium / High audio latency presets (fixes #85).
- **Gamepad Remapping, Authentic 3-Speed CD-i Pad & Libretro Cursor Controls**:
  - Configurable gamepad button mapping for CD-i Button 1, Button 2, and Button 1+2 (fixes #86).
  - Authentic Philips `22ER9021` 3-level pad cursor speed switch (`L` in Libretro / configurable button in standalone).
  - Libretro Core Options for independent D-Pad and Left Analog X/Y cursor speeds, proportional analog deflection with deadzone, and hold-`R` speed boost (fixes #91).
- **Hotkeys, Pointer IRQ2 & Documentation**:
  - Fixed standalone Quicksave/Quickload hotkeys and menu/dialog alignment (`F2`/`F4`, slots `F6`/`F7`) (fixes #87).
  - Improved Slave CH0 IRQ2 preservation across synchronous `0xF6` polls and standalone `PT_Pos` warp preservation (progress on #83).
  - Added ES-DE (`EmulationStation Desktop Edition`) `es_systems.xml` configuration guide (fixes #54).

---

## What's New in Version 0.4.1

- **CD-i Ready, Multi-Track CUE & CHD CD-DA Accuracy**:
  - Full support for **CD-i Ready / Red Book hybrid discs** (e.g. *Pavarotti*, *Steel Machine*): accurate `INDEX 00/01` pregap parsing, virtual `PREGAP` silence, Q-subcode Index 00/01 reporting, and complete TOC track tables on both Mono-I CDIC and Mono-IV CIAP (fixes #77).
  - Full **CHD `CHT2`/`CHTR` multi-track parsing**, 4-unit track padding, and automatic big-endian byte-swapping for Red Book `TYPE:AUDIO` tracks (fixes #78).
  - Skip non-Mode 2 gap/pad sectors during CDIC `Read Mode 2` (`0x2A`) playback, resolving the Level 6 music loop in *Tetris* (fixes #76).
- **Slave MCU Audio Attenuation & Stereo Integrity**:
  - Fixed Slave Channel 2 `0xC0..0xCF` attenuation command decoding to honor the 4 channel mute bits packed by `cdapdriv` in bits `3..0` of the command byte, restoring clean stereo separation and preventing cross-channel +6 dB clipping and broken mute on Mono-I titles (*Steel Machine* and others).
- **DVC FMA Interrupt Accuracy (*Mad Dog McCree*)**:
  - Guarded `FMA_IRQ_EOI` latching on video-only `0xB9`/`0xB7` stream end when FMA audio is stopped/unarmed, preventing `madriv` from broadcasting `F$Send(0, 0x2001)` and terminating the `cdi_cd_cp` pointer daemon in *Mad Dog McCree* Practice mode (fixes #73).
- **MCD-212 Matte Interleaving, macOS Standalone & Display Timing**:
  - Interleaved Path 1/0 ICA & DCA execution with full-width matte keying under `ICM_EV` (*L'albero azzurro*, *Mad Dog McCree* targets).
  - Native macOS standalone app support (menu bar, file dialogs, per-user data directory) and documented exact PAL/NTSC vertical refresh rates.

---

## What's New in Version 0.4.0

- **MPEG-1 / DVC Engine Overhaul**:
  - Completely resolved macroblock and graphic artifacts during VideoCD playback (*Black Rain*) and transparent FMV overlays (*The 7th Guest*).
  - Robust Fast-Forward / Rewind (FF/REW) and chapter seek handling on MPEG streams without video freezes or NVRAM corruption.
  - Accurate `VSYNC` synchronization and deterministic skipping of orphan P/B frames prior to the first Intra frame.
- **MCD-212 Display Controller Accuracy**:
  - Timed execution of `DCA` display instructions during HBLANK after `VSR` scanout, aligning matte key activation and boundary edges.
  - Authentic screen stride and geometry handling (720 vs 768 pixels): eliminated corrupted top line in menus and perfectly centered cutscenes in *Flashback*.
  - Fixed horizontal border offset for external video plane matte keying.
- **Audio Continuity & Room Transitions**:
  - Preserved `dclk` across FMV reset commands, fixing background music dropouts and restarts during room transitions in *Lost Eden*.
  - Eliminated duplicate decoding of Mode 2 ADPCM sound maps on the CDIC channel.
- **Zero-Latency Slave MCU Mouse Input**:
  - Immediate IRQ2 assertion on Channel 0 mouse packets: silky smooth, highly responsive cursor movement with zero input lag.
  - Configurable mouse sensitivity multiplier and input profiles in `opencdi_controls.ini`.
- **Mono-IV Audio & Multi-Platform Libretro Cores**:
  - Added CD-DA audio streaming support on Mono-IV (CD-i 490) boards via command E0.
  - Updated `opencdi_libretro.info` metadata to fully comply with the upstream Libretro-Super schema.
  - Automated continuous regression suite with savestate roundtrip determinism verification.

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
- **Advanced Audio Architecture & CD-DA Playback**: Real-time continuous MPEG decode clock cutting startup latency down to 1.6 ms (zero FMV underrun gaps), hardware CDIC `AUDCTL` audio map modeling for flawless sound effects (*Zelda: Wand of Gamelon*, *Link: Faces of Evil*), and CD-DA streaming.

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
  - Native USB gamepad support (Xbox, PlayStation, generic XInput / DirectInput controllers via `gilrs`) with full button remapping for CD-i **Button 1** (`A / Cross`), **Button 2** (`B / Circle`), **Button 1+2 combined** (`X / Square`), and **3-level CD-i Gamepad speed** (`L1 / LB` or `Tab`) via `Controls -> Gamepad Button Mapping`, `opencdi_controls.ini`, or RetroArch's Port 1 Controls.
  - Adjustable mouse sensitivity (25% - 150%) via Controls menu and configurable low-latency host audio buffer (`Audio -> Audio Latency`, default ~46 ms).

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
| **Tab / Gamepad L1 (LB)** | Cycle 3-Level CD-i Gamepad Cursor Speed (1 Slow / 2 Medium / 3 Fast) |
| **F11 / Alt+Enter** | Toggle Fullscreen (Borderless default, Exclusive optional in Video menu) |
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

Core Options include DVC auto-detection, experimental CD-i 490 (Mono-IV) BIOS selection, video levels (`studio`/`full`), motion-adaptive FMV de-interlacing (`opencdi_deinterlace`), and independent cursor speed settings for D-Pad (`opencdi_dpad_speed_x/y`), proportional left analog stick (`opencdi_analog_speed_x/y`), and hold `R` cursor speed boost (`opencdi_cursor_boost`) alongside the `L` 3-level CD-i speed switch.

### ES-DE (EmulationStation Desktop Edition) Integration
To launch Philips CD-i titles with OpenCDi in ES-DE (`~/.emulationstation/custom_systems/es_systems.xml` on Linux/SteamOS, or `%USERPROFILE%\ES-DE\custom_systems\es_systems.xml` on Windows), add:
```xml
<systemList>
  <system>
    <name>cdimono1</name>
    <fullname>Philips CD-i</fullname>
    <path>%ROMPATH%/cdimono1</path>
    <extension>.chd .cue .bin .iso .CHD .CUE .BIN .ISO</extension>
    <command label="OpenCDi (RetroArch)">%EMULATOR_RETROARCH% -L %CORE_RETROARCH%/opencdi_libretro.so %ROM%</command>
    <command label="OpenCDi (Standalone)">%EMULATOR_OPENCDI% %ROM%</command>
    <platform>cdi</platform>
    <theme>cdimono1</theme>
  </system>
</systemList>
```
On Windows/macOS replace `.so` with `.dll`/`.dylib`, or place `opencdi` / `opencdi.exe` on `PATH` (or define `%EMULATOR_OPENCDI%` in `es_find_rules.xml`).

---

## Video Timing & Exact Refresh Rates

OpenCDi derives scanline and field timing from the Philips CD-i MCD-212 clocks (`960` CPU cycles per scanline, `75` CD sectors/s). For custom CRT / VRR / RetroArch display modes, use the following rates:

| Standard | CPU Clock (`XTAL / 2`) | Scanlines (Active + VBlank) | Horizontal Rate | Exact / Libretro Field Rate | Standalone Pacing |
|---|---|---|---|---|---|
| **PAL** | `15,000,000 Hz` (`30 MHz / 2`) | `312` (`280 + 32`) | `15,625.00 Hz` | **`50.0801 Hz`** (`15,000,000 / 299,520`) | `50.00 Hz` (`20.00 ms`) |
| **NTSC** | `15,104,900 Hz` (`30.2098 MHz / 2`) | `262` (`240 + 22`) | `15,734.27 Hz` | **`60.0543 Hz`** (`75 × 201,398 / 251,520`)* | `60.00 Hz` (`16.67 ms`) |

*\*Raw NTSC crystal ratio is `15,104,900 / 251,520 = 60.0545 Hz`; the Libretro core reports `60.0543 Hz` (`audio::frame_rate`) so each emulated video field aligns with the `44,100 Hz` CD sector audio clock. Native resolution output is `768×560` (or `720×560` without `CF`, line-doubled from active lines).*

---

## Project Team & Credits

- **Project Lead & Maintainer:**
  - **Gianluca ("sigsantilio")** — Project management, testing, and Windows frontend curation. Creator of the Italian retrogaming channel [*L'Archivio del Sig. Santilio*](https://www.youtube.com/@sigsantilio).
- **Core Contributors:**
  - **ArnoldDeRuiter** — Core emulation architecture, Libretro core implementation, Mono-IV chips, and deterministic in-memory savestates.

## Special Acknowledgments

Heartfelt thanks to everyone who contributed to the development and preservation goals of OpenCDi:
- **ArnoldDeRuiter**: for the outstanding and extensive engineering work, Libretro core implementation, in-memory deterministic savestates, Mono-IV chip models (CIAP, IKAT), regression harness, and countless timing and audio refinements.
- **whatever-industries** ([`edi_emulator`](https://github.com/whatever-industries/edi_emulator)): for the MIT-licensed motion-adaptive de-interlacing algorithm adapted for OpenCDi's optional FMV de-interlacing, CD-i hardware research, and community collaboration.
- **The MAME / MESS Team**: for their invaluable documentation and foundational work on Philips and Motorola custom chips (SCC68070, MCD212, CDIC, Slave 68HC05).
- **The World of CD-i** and the broader CD-i preservation community for their dedication, technical insights, and continuous support.
- All testers, players, and contributors who helped test games and provided feedback.

---

## License

Distributed under the **MIT License**. See [LICENSE](LICENSE) for details.

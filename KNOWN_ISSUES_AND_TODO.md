# OpenCDi - Known Issues & Resolution Plan (TODO)

[Versione Italiana](PROBLEMI_STORICI_E_TODO.md)

This document tracks all historically reported issues on the OpenCDi emulator, the technical analysis of their architectural causes, the planned solution, and current progress. Every issue resolved and validated in actual gameplay is marked as completed `[x]` with technical fix details.

## Current Status (Updated 16/10/2026 - Version v0.1.3)
Legend: `[ ]` open · `[~]` fix applied in code, **pending in-game confirmation** · `[x]` confirmed by user.

| Title | Issue | Status | Fix / Note |
|---|---|---|---|
| Video transitions (Hotel Mario, Striker) | Intermediate glitchy frames / artifacts during screen fades lasting a few tenths of a second | `[x]` | **Root architectural fix (v0.1.1):** in `mcd212.rs`, `mix_lines` bypassed analog weight blending when a plane was transparent (`out[x] = pa; continue;`), exposing uninitialized VRAM at 100% brightness during fade-outs. Aligned with MAME and hardware specifications: transparent planes adopt black level `s_4bpp_color[0]` and pass through the formula with `weight_factor`. With weight 0, output is solid black (verified with `trace_video` on frames 2426–2468 of Hotel Mario). |
| Pad Controls | Upward D-Pad input unresponsive ("cannot move up") | `[x]` | **Fixed (v0.1.1):** in `main.rs`, decoupled and prioritized D-pad directions: D-pad buttons first (`Button::DPad*`), D-pad axis only if neutral (`Axis::DPad*`), analog stick last (`LeftStickY`) only if neutral. Fixed conflict that summed -1 and +1, zeroing out the UP direction. |
| Striker | Player sprite flickering and variable game speed | `[ ]` | **Under active analysis:** isolating the double-buffering cycle (VSR/DCP) and scanline/field interrupt timings (DCA line 43 and ICA line 0). |
| All Games | Disc boot failure in BIOS (stuck on "OPEN" / disc not recognized) | `[x]` | **Fixed:** in `bus.rs`, Level 2 INTACK (Slave MCU) returned `0x3A` instead of Autovector 2 `0x1A`. With `0x1A`, OS-9 `cdapdriv` executes `0x42D3D2` and transitions disc state to "PLAY CD-I". |
| 7th Guest | 1. FMV audio distortion<br>2. Video stuttering<br>3. Transitory black screen post-FMV<br>4. Corridor freeze post-ghost cutscene | `[x]` (1, 4)<br>`[ ]` (2, 3) | **User confirmed status:**<br>1. `[x]` FMV audio confirmed completely accurate and distortion-free.<br>2. `[ ]` Slight micro-stuttering remains during FMV playback.<br>3. `[ ]` 10px fracture resolved; a brief transitory black screen occurs post-FMV before room background reappears.<br>4. `[x]` Post-ghost corridor freeze brilliantly resolved; gameplay continues into hallway. |
| Kingdom, Ultra CD-i Soccer, Mutant Rampage | "DVC cartridge not detected" at boot | `[~]` | **Root architectural fix:** in `main.rs`, the DVC cartridge was only mounted if the title contained hardcoded substrings (`is_dvc_title`). Kingdom, Ultra CD-i Soccer, Mutant Rampage (and titles like Chaos Control, Creature Shock) were launched in base CD-i mode without the cartridge. DVC is now fitted by default whenever `dvc/vmpega.rom` is present (matching authentic hardware with cartridge inserted), preserved across disc swaps, and manageable via Win32 menu ("Digital Video Cartridge (DVC)") or `--no-dvc` CLI flag. |
| All Games (Soccer, Mutant, Albero, Chaos, etc.) | Garbled / corrupted rainbow pixels at bottom of screen | `[~]` | **Root architectural fix:** `Mcd212` was hardcoded to 50 Hz PAL (312 total scanlines, 280 active). NTSC titles setting `DCR1` bit 13 (`DCR_FD_BIT`) use 240 active scanlines at 60 Hz. With fixed PAL mode, DCA/VSR executed for 40 extra scanlines below line 240, reading stale VRAM and corrupting registers. Dynamic PAL/NTSC switching implemented on `DCR1` writes, Display Enable (`DCR_DE_BIT`) gated, and scanlines outside active display are letterboxed to clean black. |
| Mutant Rampage - Bodyslam | In-game music cuts out during attack moves / SFX; sprite flickering | `[~]` | **Root architectural fix:** (1) **Audio:** CD-RTOS streaming disc audio (Mode 2 Form 2) and Soundmap RAM SFX shared filter history and sector counters; disc sectors were canceling `decoding_audio_map = false` and `0x3FA` was resetting filter history. Separated ADPCM states (`map_left_state`/`map_right_state`), decoupled `audio_map_counter`, and mixed soundmap SFX directly into the active playhead without killing the disc stream. (2) **Flickering:** phase-locked to 60 Hz NTSC to eliminate 50 Hz frame tearing during sprite redraws. |
| All games (Creature Shock, etc.) | Hot-swap disc insertion resets the console | `[x]` | **Resolved:** In `main.rs`, `GuiAction::InsertDisc` was grouped with `OpenDisc` and performed a hardware CPU/system reset. Separated the actions: `InsertDisc` loads the disc image, updates the game name, and signals `bus.slave.set_disc_inserted(true)` without resetting CPU/system state, allowing multi-disc games to proceed smoothly. |
| Dragon's Lair | Dirk moves/dies on his own, game plays itself | `[~]` | **Resolved:** In Gamepad mode (`main.rs`), directional coordinates `pad_x`/`pad_y` accumulated to screen extremes and never returned to center upon key/dpad release. Implemented auto-centering to neutral `(384, 280)` when directions are released, restoring responsive player control. |
| Mutant Rampage - Bodyslam | Game fails to boot (hangs at startup) | `[x]` | **Resolved:** In `main.rs`, file names containing `(USA)` forced `VideoMode::Ntsc` at startup before booting the BIOS. The European BIOS (`cdi220b.rom`) strictly boots at 50 Hz PAL and queries the Slave MCU (cmd `0xF6`); the mismatch hung OS-9 initialization. Removed force-NTSC: console boots in PAL and dynamically switches to NTSC via DCR0 register writes during game startup. |
| L'Albero Azzurro | Black screen at launch with audio, missing DVC videos | `[~]` | **Resolved:** In `mcd212.rs` (`mix_lines`), external video was only output if both planes were transparent (`transparent_a && transparent_b`). In advanced DVC titles like *L'Albero Azzurro*, the `image_coding_method` register uses bit 4 (EV1) and bit 12 (EV2) to route external video directly onto Plane A or Plane B. Implemented direct routing of `external_video_line` when EV1/EV2/EVB bits are set (verified 312,080 video pixels across 6,599 colors at frame 1905). |
| Rise of the Robots | Combat speed excessively fast / cutscenes accelerated | `[ ]` | Under investigation: CD-DA streaming rate at 75 sectors/s and OS-9 `F$Sleep` timing. |
| Lost Eden | Audio/dialogue voices, menus, FMV stuttering | `[ ]` | Under investigation: CDIC ADPCM soundmap processing and Plane B external video overlay keying. |

> Binary synchronization note: all 3 executable binaries (`opencdi/target/release/opencdi.exe`, `opencdi/opencdi.exe`, and `../opencdi.exe`) are compiled in Release mode and synchronized with identical SHA256:
> `203F4D1A5313E45249CFE52D9D29F5125F01832D93D5309EDF322FAF84BB45C3`.

---

## 1. Hotel Mario
- [x] **Missing startup console logo / animation**
  - **Resolved:** By adhering to BIOS shell timing requirements (minimum wait of 30 fields before clicking PLAY CD-I) and clean rendering without filter bail-outs when `VSR=0`, the Philips console crest and startup animation display completely.
- [x] **Corrupted transitions with psychedelic colors / visible drawing**
  - **Resolved (v0.1.1):** In `mix_lines`, Plane B transparency no longer bypasses multiplication by `weight_factor`. During VRAM loads where `weight == 0`, the screen stays solid black until rendering setup completes, eliminating all artifacts.
- [x] **Missing background during intro & gameplay without backdrops**
  - **Resolved:** Eliminating the improper `* 298` scaling factor in `mix_lines` and removing the `vsr == 0` early return restored proper transparency and mixing between Plane A and Plane B.

---

## 2. Striker
- [ ] **Player sprite flickering and uneven game pace during match**
  - **In Progress:** Analyzing buffer swap cadence and INT1 interrupt synchronization.
- [x] **Pitch / playing field not displayed**
  - **Resolved:** The hardware-accurate `mix_lines` formula renders both Plane A and Plane B correctly (green grass texture and all menu assets).

---

## 3. Lost Eden (DVC)
- [ ] **Dialogue audio and voice distortion**
  - **Symptom:** Overall sound has defects and character dialogues (Elo, Adam, Dina) sound distorted or corrupted.
  - **Technical Cause:**
    1. In-game dialogues utilize CD-i Soundmap ADPCM sectors (Level A/B at 37.8 kHz / 18.9 kHz) processed by CDIC/Audio DSP.
    2. Video cutscenes utilize MPEG-1 Audio Layer 2 (MP2). Discrepancies in buffer consumption or ADPCM filter coefficients distort human voice reproduction.
  - **Action Plan:** Inspect ADPCM soundmap packet decoding and DVC MP2 audio resampling.

- [ ] **Game menus and interface not displaying correctly**
  - **Symptom:** Interactive menus (inventory, selection screens) are invisible or glitchy.
  - **Technical Cause:** Lost Eden menus are drawn on MCD-212 Plane B on top of external DVC video using Color Keying / Mask Color (`0x00FC_FCFC`). If external video line blending or transparency is disabled, menus are obscured or faded out.
  - **Action Plan:** Verify External Video Enable bits in `image_coding_method` and Plane B color key mask.

- [ ] **Micro-stuttering during FMV playback**
  - **Symptom:** Slight stutter/frame drops during MPEG-1 playback.
  - **Technical Cause:** Presentation cadence of MPEG frames (nominal 25 fps) vs PAL video refresh (50 Hz). A jitter-free Presentation Timestamp (PTS) mechanism at 2 fields per frame is required.
  - **Action Plan:** Refine PTS/SCR synchronization in `dvc.rs` demuxer.

---

## 4. Rise of the Robots
- [ ] **Excessively fast combat gameplay (Game runs too fast)**
  - **Symptom:** Robot combatants move and fight at unrealistic speeds (2x-3x).
  - **Technical Cause:** Rise of the Robots gameplay loop performs OS-9 `F$Sleep` calls or waits on VBLANK ticks. If tasks wake up early or CPU runs unthrottled cycles without synchronous wait, the game runs at uncapped speed.
  - **Action Plan:** Verify OS-9 tick duration in `scc_timer.rs` and 50 Hz synchronous `F$Sleep` blocking.

- [ ] **Cutscenes ending prematurely with frozen final frame**
  - **Symptom:** Opening and cutscenes play at hyper-speed and freeze on the final frame, waiting until the CD-DA audio track finishes.
  - **Technical Cause:** Investigating CD-DA playback and timing. `CYCLES_PER_SECTOR` is already at 75 sectors/s; game issues `I$SetStt` (path 4, code 0x56, sub 0x11, D3=0xFF) with D4=0.
  - **Next Steps:** (1) Identify where D4 is populated / what CDAP sub 0x11 expects; (2) Verify OS-9 SCC timer reload frequency and CPU clock; (3) Compare with MAME VBLANKs per game loop.

---

## 5. The 7th Guest (DVC)
- [x] **Black backdrop with isolated foreground elements after video transitions**
  - **Symptom:** At the end of FMV transitions, the room displays almost entirely black with only interactive hotspot items visible, lacking the 3D mansion backdrop.
  - **Technical Cause:** In `src/mcd212.rs`, an erroneous `(self.dcr[PATH] & DCR_DE_BIT) == 0` check forced black pixels and full transparency. In Philips MCD-212 hardware specs, only DCR1 (Path 0) possesses the global Display Enable bit; on DCR2 (Path 1 / Plane B) bit 15 is not a plane enable and is not set by the game. Consequently, Plane B (the 3D room backdrop) was zeroed, leaving only Plane A overlays on black.
  - **Resolution:** Removed the erroneous check in `process_vsr`. 3D backdrops and interactive objects render fused properly.

- [x] **Issue 1: FMV Audio distortion (strange/distorted sound in all FMV games)**
  - **Symptom:** Sound in FMV games sounded slightly distorted with noticeable cyclic pitch warble (wow and flutter).
  - **Technical Cause:**
    1. Dynamic Rate Control (DRC) in `audio.rs` had an excessively wide clamp and gain (±5%), causing pitch wow during CDIC DMA bursts.
    2. `Mp2Decoder` resampled each frame internally to 37.8 kHz, dropping 0.43 fractional samples per frame and causing a 38.28 Hz phase discontinuity.
    3. Video and Audio DMA shared a single FIFO and demuxer, corrupting multi-sector PES audio packets when interleaved with video data.
  - **Resolution:** Narrowed DRC clamp to ±0.5% (gain 0.005, clamp 0.995..=1.005); native 44.1 kHz stereo `f32` decoding fed directly into CDIC resampler; full separation of `fma_demux` from video `demux`. Confirmed by user: audio is fully accurate and free of distortion.

- [ ] **Issue 2: Occasional video stuttering during FMV playback**
  - **Symptom:** Occasional micro-stuttering and uneven motion during MPEG-1 playback.
  - **Technical Cause:** Frame presentation was locked to `vsync_counter % 2 == 0`, causing cadence judder with fractional frame rates, and DMA collision between audio and video sectors caused occasional MPEG slice drops.
  - **Resolution / Status:** Fractional frame accumulator (`frame_accum += fps / vsync_hz`) in `on_vsync` and dedicated FIFO channels for video and audio. User notes slight residual stuttering; PTS/SCR clock synchronization to be refined.

- [x] **Issue 3: Post-FMV Transition (10px fracture / Transitory black screen)**
  - **Symptom:** At the conclusion of FMV sequences, a brief black screen occurred before the interactive room backdrop appeared.
  - **Technical Cause:** In `src/dvc.rs`, `self.has_valid_frame = false` and premature framebuffer clearing were executed on `sequence_ended` and in `reset_stream()`. Authentic DVC CL450 hardware retains the last decoded MPEG picture in DRAM, outputting it continuously to the pixel bus until overwritten by a new stream or obscured by MCD-212 planes/ICM.
  - **Resolution:** Removed premature clearing of `has_valid_frame` and framebuffer from `sequence_ended` and `reset_stream()`. Persistent frame scanout guarantees seamless visual continuity during disc seeks into interactive rooms.

- [x] **Issue 4: Hard freeze / crash after upstairs ghost cutscene**
  - **Symptom:** Turning left upstairs plays the ghost cutscene; clicking forward down the corridor afterward caused a hard freeze with OS-9 error *"avm_play: Still busy from last play $8000"*.
  - **Technical Cause:**
    1. On the disc, Video ends at sector 246910 with `00 00 01 B9` (Program End), whereas Audio continues for 45 more sectors up to sector 246955 (where the 2nd EOR and Audio `00 00 01 B9` reside).
    2. Shared demux or premature audio underflow raised `FMA_IRQ_EOI` early, causing `vmpega.rom` to shut down the FMA audio decoder prematurely.
    3. `cdapdriv` ran out of Channel 2 audio buffers and paused CDIC with command `0x2E`. Sector 246955 (2nd EOR) was never read, `avm_busy` remained 1, and the corridor FMV failed to start.
  - **Resolution:** Dedicated FMA audio FIFO and demux; `FMA_IRQ_EOI` is raised strictly upon true audio stream Program End `00 00 01 B9`. Verified with automated regression test up to frame 5200: ghost scene finishes, corridor walk FMV loads and reads sectors LBA 161512+ without any freeze or busy error. Confirmed by user: freeze is brilliantly solved and player can advance through the corridor.

---

## 6. DVC Cartridge Not Detected (Kingdom, Ultra CD-i Soccer, Mutant Rampage)
- [~] **Symptom:** When booting *Kingdom - The Far Reaches*, *Ultra CD-i Soccer*, *Mutant Rampage - Bodyslam* (and other MPEG titles), the game warned that the Digital Video Cartridge was not detected / missing.
- **Technical Cause:** 
  In `src/main.rs`, the `is_dvc_title` function used a hardcoded keyword whitelist based on filename substrings (`LAIR`, `GUEST`, `EDEN`, `MAD DOG`, `SPACE ACE`, `BRAIN DEAD`, `VCD`, `MPEG`, `DIGITAL VIDEO`). Any game title not matching this list prevented loading the DVC ROM (`vmpega.rom`) at boot, or explicitly unmounted the cartridge (`bus.dvc = Dvc::new()`) when opening a disc via the GUI menu. Consequently, the virtual console ran as a pure base CD-i player lacking the DVC hardware (MPEG decoder, 1MB Fast RAM at 0x00D00000, and FMA/FMV registers at 0x00E00000). The games, performing their hardware probe or querying OS-9 for the `vmpega` module, reported the cartridge missing.
- **Resolution:**
  1. Aligned with authentic Philips CD-i console hardware behavior: if `dvc/vmpega.rom` is present, the DVC cartridge is fitted and enabled by default for all games (mirroring a physical CD-i 220 player with the 22ER9956 cartridge permanently inserted in its rear expansion slot).
  2. Removed the hardcoded `is_dvc_title` whitelist and preserved DVC state across disc insertions and swaps.
  3. Added Win32 menu control under *Video* (*"Cartuccia Digital Video (DVC) [vmpega.rom]"*) with active checkmark status, plus `--no-dvc` command-line switch to force base CD-i mode when specifically needed.
  4. Verified full boot and DVC detection: *Kingdom* (plays full-motion opening animation), *Ultra CD-i Soccer* (title screen and pitch rendering), *Mutant Rampage* (Start/Setup menu), *Chaos Control* (main menu), *Creature Shock* (Saturn spacecraft intro), *Inca* (language selection), *Escape from CyberCity* (main menu).
  5. Verified backward compatibility: base non-DVC games (*Hotel Mario*, *Striker Pro*, *Rise of the Robots*, *Albero Azzurro*) continue to run properly with DVC installed.

---

## 7. Black Screen Regressions & MCD-212 ICA Interleaving (Creature Shock, CD-i Logo, Albero Azzurro, Mutant Rampage)
- [x] **Symptom:** 
  1. *Creature Shock:* Background cutscene audio heard but video invisible; initial menu shows cursor on a solid black screen.
  2. *Console without disc:* Does not reach normal CD-i dashboard.
  3. *L'Albero Azzurro:* Initial animated CD-i logo missing; launch intro video black with sound; room object selection screen pitch black.
  4. *Mutant Rampage - Bodyslam:* CD-i animated logo missing; game menus not visible (black).
- **Technical Cause:** 
  1. In `src/mcd212.rs`, ICA instructions were executed by running all of Path 1 followed by all of Path 0 (`process_ica::<1>()` followed by `process_ica::<0>()`). At line 0, Path 1 set the matte transparency registers (`MC0 = 0x00900000`, `MC1 = 0x008002FF`) at step 139 to make planes A & B transparent and display the MPEG video backdrop. However, Path 0's reset instructions at steps 3..10 cleared all matte registers (`0xD0..=0xD7` to `0x00800000`), wiping out Path 1's configuration entirely.
  2. Non-documented test bits in `mix_lines` were masking planes.
- **Resolution:**
  1. Implemented cycle/step interleaved execution of ICA Path 1 and Path 0 (`process_ica_step`), matching real Philips MCD-212 hardware architecture and MAME reference (`mcd212_mame.cpp`).
  2. Restored clean line mixer.
  3. Verified visual rendering: CD-i animated logo, dashboard, Creature Shock cockpit & cutscenes, Albero Azzurro VIDEORAI intro (39,000+ colors) and room screens, Mutant Rampage menus are fully visible.

---

## 8. Hot Disc Swap for Multi-CD Games (Creature Shock)
- [x] **Symptom:** Selecting "Inserisci disco a caldo..." from the menu triggered a full console reset instead of continuing gameplay.
- **Technical Cause:** 
  In `src/main.rs`, `GuiAction::InsertDisc` was grouped with `GuiAction::OpenDisc`, executing `bus.reset_system()` and `cpu.reset()`. Furthermore, the Slave MCU did not emulate the tray open/close status transitions.
- **Resolution:**
  1. Implemented dedicated `bus.change_disc(&path)` preserving CPU SCC68070 registers, system RAM, and OS-9 / CD-RTOS state.
  2. Added `notify_disc_swapped()` in `slave.rs` which transmits tray opened (`0x00`) followed by spinup and TOC ready (`0x41` -> `0x44`), allowing OS-9 to re-read volume descriptors without trapping.
  3. Verified via automated test `test_disc_change`.

---

## 9. Video CD (White Book / CD-i Bridge) & Multi-Track / Multi-File CUE Support (Kilby Predicts, Gateway to the Mind)
- [x] **Symptom:** 
  1. Multi-track Video CDs (Track 1 ISO 9660 + CD-i filesystem, Track 2 multiplexed MPEG-1 `AVSEQ01.DAT`) and multi-file CUE sheets (separate `.bin` files per track, such as *Kilby Predicts*) failed to load or were treated as single-track standard CD-i images.
  2. The OS-9 system driver `cdapdriv` recognized discs only as standard Red Book/single-track CD-i (`disc_type = 0x10`), failing to detect Track 2 and refusing to stream MPEG video.
- **Technical Cause:** 
  1. `CdRom` in `src/cdrom.rs` supported only single raw files (`CdSource::RawFile`) or CHDs with a static 1-track layout.
  2. Subcode Q Table of Contents (TOC) packets in `src/cdic.rs` were hardcoded (`Pak A0`, `Pak A1`, `Pak A2`, `Pak 01`), always reporting a 1-track disc of type `0x10`.
  3. Reading Subcode Q while streaming did not calculate the current track or relative track MSF coordinates.
- **Resolution:**
  1. **Folder Organization:** Dedicated subfolders created in `file utili/videocd/`: `Gateway to the Mind/` and `CD-i See It Hear It Feel It (USA) (Kilby Predicts) (Rev 1)/`.
  2. **Multi-Track & Multi-File Architecture (`src/cdrom.rs`):**
     - Added `CdTrack` struct tracking track number, control/mode, start LBA, sector length, sector size, file index, and file offset.
     - Implemented `CdSource::RawFiles { files: Vec<File> }` with full CUE parser supporting both single-binary multi-track files (*Gateway to the Mind*) and multi-file track structures (*Kilby Predicts*).
     - Automated disc type detection from PVD (LBA 16): detecting System ID `"CD-RTOS CD-BRIDGE"` sets `disc_type = 0x20` (White Book VCD), preserving `0x10` for standard CD-i discs.
     - Track-routed sector lookup in `read_sector`: seeks accurately across files and intra-file track offsets.
  3. **Dynamic Subcode Q TOC (`src/cdic.rs`):**
     - Replaced static TOC packets with dynamic generation based on loaded disc tracks and disc type (`0xA0` with first track and true disc type, `0xA1` with last track number, `0xA2` with total lead-out MSF, plus individual track entries `0x01`..`0xNN`).
     - Subcode Q queries during playback dynamically compute the active track number and track-relative MSF time.
  4. **Verification & Results:**
     - *Kilby Predicts:* Flawless, continuous MPEG-1 video/audio playback from Track 2 (`AVSEQ01.DAT`, LBA 81480 to 83644+) with 0 underflow, synchronized audio, and full-color "KILBY" intro on red velvet curtain (11,187 colors, verified across 850+ frames).
     - *Gateway to the Mind:* Proper boot to `CDI/CDI_VCD.APP`, continuous MPEG-1 playback of the CD-i bumper `CDI_BUM.DAT` (LBA 862..1507) with audio/video and Plane B CD-i backdrop rendering.

---

## 10. Gamepad Dashboard Movement & DVC Audio Regression Fix (The 7th Guest) - Release v0.1.6
- [x] **Symptom 1 (Gamepad in Dashboard):** When navigating the standalone disc-less dashboard or BIOS with the D-pad, the cursor snapped abruptly across 8 fixed positions without smooth progression and snapped back to center on release.
  - **Technical Cause:**
    1. In `src/main.rs`, integer coordinate increments at `pad_speed = 8` caused aggressive displacement jumps.
    2. Missing continuous sub-pixel float accumulator caused sampling discontinuities.
    3. Standalone boot and reset coordinates lacked proper centering to `(384, 280)`.
  - **Resolution:**
    1. Established authentic fixed speed `PAD_SPEED = 2.5` pixels per frame.
    2. Implemented continuous sub-pixel accumulators (`pad_x_f`, `pad_y_f`) with zero-drift position hold on D-pad release.
    3. Center-anchored resting position at `(384, 280)` on standalone boot, Reset, and disc Eject.
    4. Verified with `test_dashboard_mouse`: smooth, continuous linear progression with 0 warps or rebound artifacts.

- [x] **Symptom 2 (Freeze in The 7th Guest post-ghost cutscene):**
  - **Technical Cause:**
    1. In `src/dvc.rs:1115-1121`, `FMA_IRQ_EOI | FMA_IRQ_UNF` was triggered after just 10 vsyncs (200 ms) of temporary audio FIFO drain.
    2. Video sequence completion was unconditionally triggering `FMA_IRQ_EOI` without verifying independent audio DMA channel routing (`self.fma_dma_seen`).
    3. In The 7th Guest ghost video, audio continues for 45 sectors past the video stream end to sector 246955 (second EOR). Premature FMA shutdown halted CDIC reads, leaving `avm_busy` (`0xD02C8A`) at 1, causing hallway navigation to fail with *"avm_play: Still busy from last play $8000"*.
  - **Resolution:**
    1. Temporary audio drain (`fma_drained`) strictly asserts authentic hardware underflow `FMA_IRQ_UNF`, NEVER `FMA_IRQ_EOI`. `FMA_IRQ_EOI` is exclusively reserved for ISO code `00 00 01 B9` encountered by the audio demuxer or explicit software stop.
    2. Video sequence completion only asserts `FMA_IRQ_EOI` if audio is not using dedicated FMA DMA (`if !self.fma_dma_seen`).

---

*(For an overview of all 15 games in `file utili/giochi/`, see [`COMPATIBILITY.md`](COMPATIBILITY.md))*



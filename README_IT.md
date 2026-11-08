# OpenCDi (Italiano)

[English Version](README.md)

OpenCDi è un emulatore open source moderno per **Philips CD-i** scritto in **Rust**.
L'obiettivo del progetto è preservare e rendere fruibile l'intera libreria software e multimediale del sistema Philips CD-i con un'architettura moderna, accurata e veloce.

![OpenCDi Showcase](opencdi_showcase_v016.png)

---

## Novità Versione 0.2.0

- **Libretro Core (`crates/opencdi-libretro`)**: OpenCDi è ora disponibile come core per **RetroArch** (`opencdi_libretro.dll`) e frontend compatibili, con supporto nativo a Runahead a zero latenza e Rewind deterministico.
- **Supporto Architettura Mono-IV**: Emulazione sperimentale della scheda **Philips CD-i 490 (Mono-IV)** con controller CIAP (CD-i Audio/Packet), coprocessore I/O IKAT e RTC Dallas DS1216.
- **Selettore BIOS & DVC Runtime**: Scelta interattiva del BIOS dal menu di sistema o con scorciatoia `Ctrl+B`, auto-rilevamento delle ROM nelle cartelle, estrazione automatica da archivi ZIP (es. `cdimono1.zip`) e persistenza della configurazione in `opencdi.ini`.
- **Assistente Interattivo per BIOS Mancante**: Guida visuale in caso di ROM mancante, con possibilità di sfogliare i file del PC o aprire direttamente la cartella `bios/` in Esplora Risorse.
- **Nuova Architettura Savestate In-Memory**: Salvataggi di stato deterministici ad altissima velocità su 1000 slot (`F2` salva, `F4` carica, `F6`/`F7` selezione slot), estesi a Mono-I, Mono-IV e alla cartuccia DVC.
- **Cattura Screenshot PNG ad Alta Definizione**: Scorciatoie `F8` e `F12` per salvare istantaneamente screenshot non compressi con nome del gioco e numero di frame.
- **Isolamento Esclusivo Mouse / Gamepad**: Separazione rigorosa tra modalità Gamepad (`F9`) e modalità Mouse/Lightgun per giochi di tiro come *Mad Dog McCree*, *Mad Dog II* e *Drug Wars*.
- **Display & Windowing Moderno**: Finestra fluida e multi-piattaforma basata su `winit` e `softbuffer`, supporto per Borderless Fullscreen (`F11` / `Alt+Enter`) e aspect ratio preservation a risoluzione nativa.

---

## Caratteristiche & Architettura Hardware

- **CPU:** Philips SCC68070 (set di istruzioni 68000/68010, interrupt autovettorati, timer interni, UART e DMA su chip a 15 MHz).
- **Video:** Philips MCD-212 Video Display Controller:
  - Rendering a doppio piano (Plane A & Plane B) e hardware backdrop.
  - Formati grafici supportati: CLUT4, CLUT7, CLUT8, Run-Length (7 e 8 bit), DYUV (Delta-YUV) e RGB555.
  - Istruzioni display linea per linea ICA (Instruction Control Area) e DCA (Dynamic Control Area).
  - Selezione al volo dei livelli video studio (nero 16) o full-range (tasto `L`).
- **CD-ROM & Audio:**
  - Controller CDIC (CD-i Controller) per la decodifica dei settori Mode 2 Form 1 e Form 2.
  - Filtraggio e riproduzione audio CD-DA e CD-i ADPCM (Livelli A, B e C, stereo e mono).
  - Supporto nativo per immagini disco compresse CHD (`chd-rs`), file CUE/BIN e ISO standard.
- **Microcontrollore Slave (68HC05 HLE):**
  - Gestione input per tastiera, mouse, pistole ottiche (Peacekeeper) e gamepad CD-i.
  - Orologio Real-Time (RTC) e memoria non volatile NVRAM (MK48T08 / Dallas DS1216).
- **Digital Video Cartridge (DVC / MPEG-1):**
  - Emulazione hardware per cartucce VMPEG e IMPEG per riproduzione FMV (Full Motion Video).
  - Demuxer di sistema integrato (video pack MPEG-1 e audio Layer 2).
  - Decoder video MPEG con compositing YUV -> RGB sul piano video esterno del MCD-212.
- **Sistema di Controllo & Input:**
  - Tasto `F9`: commutazione immediata tra CD-i Gamepad (Controller USB / Tastiera) e Philips CD-i Mouse / Lightgun.
  - Supporto controller USB nativo (Xbox, PlayStation, standard XInput / DirectInput tramite `gilrs`).
  - Sensibilità del mouse regolabile (25% - 150%) dal menu Controls.

---

## Controlli e Scorciatoie da Tastiera

| Tasto | Azione |
|---|---|
| **F1** | Informazioni e Guida Comandi |
| **F2** | Salva Savestate nello slot corrente |
| **F4** | Carica Savestate dallo slot corrente |
| **F6 / F7** | Slot Savestate Precedente / Successivo (0 - 999) |
| **F5 / H** | Reset Console |
| **F8 / F12** | Cattura Screenshot (PNG ad alta definizione) |
| **F9** | Cambia Dispositivo Input (Gamepad <-> Mouse) |
| **F11 / Alt+Enter** | Attiva / Disattiva Schermo Intero (Borderless Fullscreen) |
| **P** | Pausa / Riprendi Emulazione |
| **Spazio (tieni premuto)** | Fast-Forward (Avanzamento Veloce) |
| **L** | Commuta Livelli Video (Studio Range / Full Range) |
| **M** | Mute Audio |
| **+ / -** | Volume Su / Volume Giù |
| **Ctrl + I** | Inserisci Disco a Caldo (Hot-Swap) |
| **Ctrl + O** | Apri Disco e Riavvia Console |
| **Ctrl + B** | Seleziona File BIOS ROM o ZIP |
| **Tasto Centrale Mouse** | Cattura / Rilascia Puntatore Mouse |
| **Esc** | Esci dall'emulatore |

---

## Compatibilità Verificata

| Titolo | Grafica / DVC | Stato Gameplay |
|---|---|---|
| **Hotel Mario** | MCD-212 / CDIC ADPCM | Pienamente giocabile, audio ADPCM e animazioni complete |
| **The Apprentice** | MCD-212 / CDIC ADPCM | Pienamente giocabile a 50 fps, scrolling e audio perfetti |
| **Dragon's Lair (USA)** | DVC FMV MPEG-1 | Attract mode continuo e gameplay interattivo completo |
| **Mad Dog McCree / Mad Dog II** | DVC MPEG-1 / Slave Lightgun | Giocabile con mouse/gun, mira reattiva e video FMV sincronizzato |
| **Drug Wars** | MCD-212 / Slave Lightgun | Giocabile, sparo e mirino reattivi |
| **Lost Eden** | DVC MPEG-1 / CDIC | Sequenze FMV con parlato e dialoghi sincronizzati |
| **The 7th Guest** | DVC FMV MPEG-1 | Intro, puzzle e navigazione completa |
| **Striker Pro** | MCD-212 / CDIC ADPCM | Menù e gameplay funzionanti, audio sincronizzato |
| **Chaos Control** | DVC FMV MPEG-1 | Sequenze video e gameplay con DVC |
| **Creature Shock** | DVC FMV MPEG-1 | Hot-swap multi-disco e sequenze attive |
| **Earth Command** | MCD-212 NTSC | Auto-rilevamento NTSC e gameplay funzionante |

---

## Compilazione e Avvio

### Prerequisiti
- [Rust & Cargo](https://rustup.rs/) (versione 1.93 o più recente).

### Compilazione Standalone
```bash
git clone https://github.com/sigsantilio/opencdi.git
cd opencdi
cargo build --release --bin opencdi
```
L'eseguibile ottimizzato viene generato in `target/release/opencdi.exe`.

### Compilazione Core Libretro (per RetroArch)
```bash
cargo build --release -p opencdi-libretro
```
La libreria `opencdi_libretro.dll` (o `.so` su Linux) viene generata in `target/release/`.

---

---

## Ringraziamenti Speciali

Un sincero ringraziamento a tutti coloro che hanno reso possibile lo sviluppo e l'evoluzione di OpenCDi:
- **ArnoldDeRuiter**: per l'eccezionale e massiccio lavoro di reverse engineering, implementazione del core Libretro, savestates in memoria, chip Mono-IV (CIAP, IKAT), harness di test deterministico e innumerevoli rifiniture al timing e all'audio.
- **Il Team di MAME / MESS**: per la fondamentale documentazione di riferimento sull'hardware personalizzato Philips e Motorola (SCC68070, MCD212, CDIC, Slave 68HC05).
- **The World of CD-i** e l'intera comunità di preservazione CD-i per la passione, le informazioni tecniche e il supporto continuo.
- Tutti i tester e appassionati che hanno provato l'emulatore e fornito feedback prezioso.

---

## Licenza

Distribuito sotto licenza **MIT**. Vedere il file [LICENSE](LICENSE) per ulteriori dettagli.

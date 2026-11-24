# OpenCDi (Italiano)

[English Version](README.md)

OpenCDi è un emulatore open source moderno per **Philips CD-i** scritto in **Rust**.
L'obiettivo del progetto è preservare e rendere fruibile l'intera libreria software e multimediale del sistema Philips CD-i con un'architettura moderna, accurata e veloce.

![OpenCDi](opencdi_banner.png)

---

## Novità Versione 0.4.2

- **Accuratezza External Video MCD-212 (`0xC0` `ICM_EV`)**:
  - Limitata l'abilitazione External Video al solo bit 18 (`ICM_EV`) del registro `0xC0` come da datasheet ufficiale Motorola MCD212, ignorando i bit riservati 4 e 12 così che gli ultimi frame FMV non traspaiano sulle schermate successive (risolve la sovrapposizione su *Lucky Luke*, #89).
- **Offset PRPA Display CL450 DVC**:
  - Preservato `decoded_prpa` all'offset byte DRAM `0x56` mantenendo il Frame Rate Ratio (`1800`) al byte `0xAC`: risolve il blocco di *BrainDead 13* dopo l'intro Philips (#94) e il mancato avanzamento dai filmati FMV non saltati (*Lucky Luke*).
- **CD-i Ready & Mappatura Tracce CHD**:
  - Corretti `disc_type = 0x00` nella TOC CD-i Ready e l'allineamento delle tracce CHD così che la traccia 1 non venga più saltata su *Steel Machine* (#84) e *The Apprentice* (#88, #92).
- **De-Interlacing Adattivo Opzionale per FMV**:
  - Aggiunto de-interlacing motion-adaptive solo in fase di presentazione per sequenze FMV interlacciate sia in standalone che nelle Core Options Libretro (#81).
- **Schermo Intero Esclusivo & Latenza Audio Configurabile**:
  - Aggiunto Schermo Intero Esclusivo per ridurre la latenza video (#80) e preset Latenza Audio Bassa / Media / Alta (#85).
- **Rimappatura Gamepad, Selettore Velocità Pad CD-i (3 livelli) & Controlli Cursore Libretro**:
  - Rimappatura configurabile per Pulsante 1, Pulsante 2 e 1+2 del gamepad (#86).
  - Selettore autentico a 3 velocità stile Philips `22ER9021` (`L` su Libretro / tasto configurabile in standalone).
  - Nuove Core Options Libretro per velocità D-Pad/Analog X/Y indipendenti, stick analogico proporzionale con deadzone e boost cursore tenendo premuto `R` (#91).
- **Hotkey, IRQ2 Puntatore & Documentazione ES-DE**:
  - Allineati hotkey e menu Quicksave/Quickload/Slot (`F2`/`F4`, `F6`/`F7`) in standalone (#87).
  - Migliorata la preservazione di IRQ2 su `CH0` durante i poll sincroni `0xF6` e la gestione dei warp `PT_Pos` (avanzamento su #83).
  - Aggiunta la guida alla configurazione ES-DE (`es_systems.xml`) (#54).

---

## Novità Versione 0.4.1

- **Accuratezza CD-i Ready, CUE Multi-Traccia & CD-DA su CHD**:
  - Supporto completo ai **dischi ibridi CD-i Ready / Red Book** (es. *Pavarotti*, *Steel Machine*): parsing accurato dei pregap `INDEX 00/01`, `PREGAP` virtuali, Q-subcode Index 00/01 e tabelle TOC complete su Mono-I CDIC e Mono-IV CIAP (risolve #77).
  - Parsing completo dei metadati **CHD `CHT2`/`CHTR` multi-traccia**, padding a 4 unità e byte-swap automatico delle tracce CD-DA `TYPE:AUDIO` in big-endian (risolve #78).
  - Scarto dei settori gap/pad non Mode 2 durante `Read Mode 2` (`0x2A`) sul CDIC, risolvendo il loop musicale del Livello 6 in *Tetris* (risolve #76).
- **Attenuazione Audio Slave MCU & Separazione Stereo**:
  - Corretto il decoding del comando Slave Channel 2 `0xC0..0xCF` per rispettare i 4 bit di mute codificati da `cdapdriv` nei bit `3..0` del byte di comando, ripristinando la separazione stereo e risolvendo il clipping +6 dB e il mute rotto su titoli Mono-I (tra cui *Steel Machine*).
- **Accuratezza Interrupt FMA DVC (*Mad Dog McCree*)**:
  - Impedito il latch spurio di `FMA_IRQ_EOI` al termine di flussi solo video (`0xB9`/`0xB7`) quando l'audio FMA è fermo/non armato, evitando il broadcast `F$Send(0, 0x2001)` di `madriv` che terminava il demone puntatore `cdi_cd_cp` nella modalità Practice di *Mad Dog McCree* (risolve #73).
- **Interleaving Matte MCD-212, Standalone macOS & Timing Display**:
  - Esecuzione interleaved ICA/DCA Path 1/0 e matte keying a piena larghezza con `ICM_EV` (*L'albero azzurro*, bersagli *Mad Dog McCree*).
  - Supporto app standalone nativa macOS e documentazione dei refresh rate esatti PAL/NTSC.

---

## Novità Versione 0.4.0

- **Perfezionamento Engine MPEG-1 / DVC**:
  - Risolti completamente gli artefatti grafici e macroblock sui VideoCD (*Black Rain*) e sui filmati con trasparenze/chroma-key (*The 7th Guest*).
  - Gestione robusta dell'avanzamento e riavvolgimento rapido (FF/REW) e salto capitolo su flussi MPEG senza corruzione del video né blocchi di sistema.
  - Sincronizzazione precisa `VSYNC` e scarto deterministico dei frame P/B orfani prima dell'Intra frame.
- **Accuratezza Video Controller Philips MCD-212**:
  - Esecuzione temporizzata delle istruzioni `DCA` in HBLANK dopo la scansione `VSR`, correggendo il timing di attivazione/chiusura dei matte.
  - Risoluzione geometrica e stride autentici (720 vs 768 pixel) con allineamento perfetto dei bordi: eliminata la linea corrotta nei menu e centrate perfettamente le cutscene di *Flashback*.
  - Corretto l'offset orizzontale del piano video esterno in modalità matte keying.
- **Continuità Audio & Transizioni Ambienti**:
  - Preservazione del clock `dclk` attraverso i comandi di reset FMV, eliminando le interruzioni e il riavvio della musica durante i cambi di stanza in *Lost Eden*.
  - Prevenzione del doppio decoding di mappe audio ADPCM Mode 2 sul canale CDIC.
- **Input Slave MCU a Latenza Zero**:
  - Asserzione immediata dell'interrupt IRQ2 sui pacchetti mouse del canale CH0: puntatore reattivo e fluido senza scatti né perdita di campioni.
  - Sensibilità configurabile e profili comandi persistenti in `opencdi_controls.ini`.
- **Audio Mono-IV & Libretro Multi-Piattaforma**:
  - Supporto riproduzione CD-DA su schede Mono-IV (CD-i 490) con comando disco E0.
  - File metadati `opencdi_libretro.info` pienamente allineato alle specifiche Libretro-Super.
  - Suite di regressione automatica con verifica deterministica dei savestate (`--roundtrip`).

---

## Novità Versione 0.3.0

- **Libretro Core (`crates/opencdi-libretro`)**: OpenCDi è ora disponibile come core per **RetroArch** (`opencdi_libretro.dll`) e frontend compatibili, con supporto nativo a Runahead a zero latenza e Rewind deterministico.
- **Supporto Architettura Mono-IV**: Emulazione sperimentale della scheda **Philips CD-i 490 (Mono-IV)** con controller CIAP (CD-i Audio/Packet), coprocessore I/O IKAT e RTC Dallas DS1216.
- **Selettore BIOS & DVC Runtime**: Scelta interattiva del BIOS dal menu di sistema o con scorciatoia `Ctrl+B`, auto-rilevamento delle ROM nelle cartelle, estrazione automatica da archivi ZIP (es. `cdimono1.zip`) e persistenza della configurazione in `opencdi.ini`.
- **Assistente Interattivo per BIOS Mancante**: Guida visuale in caso di ROM mancante, con possibilità di sfogliare i file del PC o aprire direttamente la cartella `bios/` in Esplora Risorse.
- **Nuova Architettura Savestate In-Memory**: Salvataggi di stato deterministici ad altissima velocità su 1000 slot (`F2` salva, `F4` carica, `F6`/`F7` selezione slot), estesi a Mono-I, Mono-IV e alla cartuccia DVC.
- **Cattura Screenshot PNG ad Alta Definizione**: Scorciatoie `F8` e `F12` per salvare istantaneamente screenshot non compressi con nome del gioco e numero di frame.
- **Isolamento Esclusivo Mouse / Gamepad**: Separazione rigorosa tra modalità Gamepad (`F9`) e modalità Mouse/Lightgun per giochi di tiro come *Mad Dog McCree*, *Mad Dog II* e *Drug Wars*.
- **Display & Windowing Moderno**: Finestra fluida e multi-piattaforma basata su `winit` e `softbuffer`, supporto per Borderless Fullscreen (`F11` / `Alt+Enter`) e aspect ratio preservation a risoluzione nativa.
- **Architettura Audio Avanzata & Supporto CD-DA**: Clock di decodifica audio MPEG in tempo reale sincronizzato alla CPU che riduce la latenza a soli 1.6 ms (eliminando buchi audio nei filmati FMV), modellazione fedele del registro `AUDCTL` del CDIC per effetti sonori perfetti (*Zelda: Wand of Gamelon*, *Link: Faces of Evil*) e riproduzione tracce CD-DA.

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
  - Supporto controller USB nativo (Xbox, PlayStation, standard XInput / DirectInput tramite `gilrs`) con rimappatura completa dei pulsanti CD-i **Button 1** (`A / Croce`), **Button 2** (`B / Cerchio`), **Button 1+2 combinato** (`X / Quadrato`) e **velocità Gamepad CD-i a 3 livelli** (`L1 / LB` o `Tab`) dal menu `Controls -> Gamepad Button Mapping`, da `opencdi_controls.ini` o da RetroArch (*Controlli Porta 1*).
  - Sensibilità del mouse regolabile (25% - 150%) dal menu Controls e buffer audio a bassa latenza configurabile (`Audio -> Audio Latency`, default ~46 ms).

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
| **Tab / Gamepad L1 (LB)** | Cicla la Velocità del Gamepad CD-i su 3 Livelli (1 Lento / 2 Medio / 3 Veloce) |
| **F11 / Alt+Enter** | Attiva / Disattiva Schermo Intero (Borderless di default, Exclusive opzionale nel menu Video) |
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

Le Core Options includono il rilevamento automatico della cartuccia DVC, il BIOS sperimentale CD-i 490 (Mono-IV), i livelli video (`studio`/`full`), il de-interlacing motion-adaptive per FMV (`opencdi_deinterlace`) e controlli granulari per il cursore su controller: velocità D-Pad X/Y (`opencdi_dpad_speed_x/y`), stick analogico sinistro proporzionale (`opencdi_analog_speed_x/y`) e boost temporaneo tenendo premuto `R` (`opencdi_cursor_boost`), oltre al selettore hardware a 3 livelli su `L`.

### Integrazione con ES-DE (EmulationStation Desktop Edition)
Per avviare i giochi Philips CD-i con OpenCDi da ES-DE (`~/.emulationstation/custom_systems/es_systems.xml` su Linux/SteamOS o `%USERPROFILE%\ES-DE\custom_systems\es_systems.xml` su Windows), aggiungi:
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
Su Windows/macOS sostituisci `.so` con `.dll`/`.dylib` (e definisci `%EMULATOR_OPENCDI%` in `es_find_rules.xml` se usi la versione standalone).

---

## Timing Video e Frequenze di Refresh

OpenCDi calcola le frequenze di riga e di quadro a partire dai clock hardware dell'MCD-212 (`960` cicli CPU per scanline, `75` settori CD/s). Per configurare risoluzioni personalizzate CRT / VRR / RetroArch, i valori di riferimento sono:

| Standard | Clock CPU (`XTAL / 2`) | Scanline (Attive + VBlank) | Freq. Orizzontale | Freq. Verticale Esatta / Libretro | Pacing Standalone |
|---|---|---|---|---|---|
| **PAL** | `15.000.000 Hz` (`30 MHz / 2`) | `312` (`280 + 32`) | `15.625,00 Hz` | **`50.0801 Hz`** (`15.000.000 / 299.520`) | `50.00 Hz` (`20.00 ms`) |
| **NTSC** | `15.104.900 Hz` (`30.2098 MHz / 2`) | `262` (`240 + 22`) | `15.734,27 Hz` | **`60.0543 Hz`** (`75 × 201.398 / 251.520`)* | `60.00 Hz` (`16.67 ms`) |

*\*Il rapporto grezzo del quarzo NTSC è `15.104.900 / 251.520 = 60.0545 Hz`; il core Libretro espone `60.0543 Hz` (`audio::frame_rate`) per mantenere sincronizzato ogni quadro video con il clock audio dei settori CD a `44.100 Hz`. L'uscita nativa è `768×560` (o `720×560` senza bit `CF`, con raddoppio di riga sulle scanline attive).*

---

## Team del Progetto & Crediti

- **Project Lead & Maintainer:**
  - **Gianluca ("sigsantilio")** — Coordinamento progetto, test e cura del frontend Windows. Curatore del canale YouTube retrogaming [*L'Archivio del Sig. Santilio*](https://www.youtube.com/@sigsantilio).
- **Contributori Principali:**
  - **ArnoldDeRuiter** — Architettura del core, implementazione Libretro, emulazione chip Mono-IV e savestate deterministici in memoria.

## Ringraziamenti Speciali

Un sincero ringraziamento a tutti coloro che hanno reso possibile lo sviluppo e l'evoluzione di OpenCDi:
- **ArnoldDeRuiter**: per l'eccezionale e massiccio lavoro di reverse engineering, implementazione del core Libretro, savestates in memoria, chip Mono-IV (CIAP, IKAT), harness di test deterministico e innumerevoli rifiniture al timing e all'audio.
- **whatever-industries** ([`edi_emulator`](https://github.com/whatever-industries/edi_emulator)): per l'algoritmo di de-interlacing motion-adaptive sotto licenza MIT adattato per il de-interlacing FMV opzionale di OpenCDi, la ricerca hardware CD-i e la collaborazione con la comunità.
- **Il Team di MAME / MESS**: per la fondamentale documentazione di riferimento sull'hardware personalizzato Philips e Motorola (SCC68070, MCD212, CDIC, Slave 68HC05).
- **The World of CD-i** e l'intera comunità di preservazione CD-i per la passione, le informazioni tecniche e il supporto continuo.
- Tutti i tester e appassionati che hanno provato l'emulatore e fornito feedback prezioso.

---

## Licenza

Distribuito sotto licenza **MIT**. Vedere il file [LICENSE](LICENSE) per ulteriori dettagli.

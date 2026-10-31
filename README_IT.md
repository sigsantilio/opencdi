# OpenCDi (Italiano)

[English Version](README.md)

OpenCDi è un emulatore open source per **Philips CD-i** scritto in **Rust**.
L'obiettivo del progetto è preservare e rendere fruibile la libreria software e multimediale del sistema Philips CD-i con un'architettura moderna, veloce e pulita.

![OpenCDi Showcase](opencdi_showcase_v016.png)

---

## Caratteristiche & Architettura

- **CPU:** Philips SCC68070 (set di istruzioni 68000/68010, interrupt autovettorati, timer interni, UART e canali DMA su chip).
- **Video:** Philips MCD-212 Video Display Controller:
  - Rendering a doppio piano (Plane A & Plane B) e backdrop.
  - Formati grafici supportati: CLUT4, CLUT7, CLUT8, Run-Length (7 e 8 bit), DYUV (Delta-YUV) e RGB555.
  - Istruzioni display lista linea per linea ICA (Instruction Control Area) e DCA (Dynamic Control Area).
- **CD-ROM & Audio:**
  - Controller CDIC (CD-i Controller) per la decodifica dei settori Mode 2 Form 1 e Form 2.
  - Filtraggio e riproduzione audio CD-DA e CD-i ADPCM (Livelli A, B e C, stereo e mono).
  - Supporto per immagini disco CHD (formato compresso MAME/MESS) e binari raw.
- **Microcontrollore Slave (68HC05 HLE):**
  - Gestione input per tastiera, mouse e gamepad CD-i.
  - Orologio Real-Time (RTC) e memoria non volatile (NVRAM).
- **Digital Video Cartridge (DVC / MPEG-1):**
  - Emulazione della cartuccia DVC per la riproduzione FMV (Full Motion Video).
  - Demuxer hardware integrato (video pack MPEG-1 e audio layer 2).
  - Decoder video MPEG e pipeline di compositing YUV -> RGB su MCD-212.
- **Salvataggi di Stato & Persistenza NVRAM:**
  - Sistema Save State a 5 slot con salvataggio atomico dell'hardware (`F6` Quick Save, `F7` Quick Load).
  - Persistenza reale della memoria console (NVRAM MK48T08) conservata fedelmente su disco.
- **Sistema di Controllo & Input:**
  - Modalità esclusiva di input (`F9` toggle): CD-i Gamepad vs CD-i Mouse senza sovrapposizione di comandi.
  - Supporto nativo controller USB (Xbox, PlayStation, generici) e profili tastiera personalizzabili.
- **Frontend GUI & Feedback:**
  - Finestra interattiva a 50/60 Hz basata su `minifb`.
  - Barra dei menu Win32 con pulsante in primo piano `Send Feedback (F1)` collegato alla piattaforma tester.

---

## Compatibilità Verificata

| Titolo | Supporto Video / DVC | Stato Gameplay |
|---|---|---|
| **Hotel Mario** | Grafica MCD212 / CDIC | Pienamente giocabile, audio ADPCM e animazioni complete |
| **Striker Pro** | Grafica MCD212 / CDIC | Menù e gameplay funzionanti, audio sincronizzato |
| **Dragon's Lair (USA)** | DVC FMV MPEG-1 | Attract mode continuo e gameplay con video FMV MPEG attivo |
| **The 7th Guest** | DVC FMV MPEG-1 | Boot, FMV introduttivo e caricamento scene |
| **Rise of the Robots** | Grafica MCD212 / DVC | Boot e sequenze iniziali |
| **Lost Eden** | DVC FMV MPEG-1 / CDIC | Boot, filmati e dialoghi |
| **Burn:Cycle** | DVC FMV MPEG-1 | Boot e sequenze video |

---

## Come Compilare ed Eseguire

### Prerequisiti
- [Rust & Cargo](https://rustup.rs/) (versione 1.70 o superiore).

### Compilazione
```bash
git clone https://github.com/sigsantilio/opencdi.git
cd opencdi
cargo build --release
```

L'eseguibile ottimizzato verrà generato in `target/release/opencdi.exe`.

## Setup del BIOS e File di Sistema Necessari

OpenCDi è un emulatore low-level che esegue il firmware originale Philips CD-i per inizializzare il kernel del sistema operativo OS-9, i controller di bus e i driver di periferica. Per ragioni di copyright, **queste ROM proprietarie non sono incluse nell'emulatore e devono essere fornite dall'utente.**

### 1. Tabella di Specifiche delle ROM di Sistema

| Componente | Nome File Consigliato | Nomi Alternativi Supportati | Dimensione Esatta (Byte) | Checksum MD5 | Checksum SHA-1 | Stato |
|---|---|---|---|---|---|---|
| **BIOS di Sistema** | `cdi220b.rom` | `cdi220.rom`, `cdi200.rom`, `cdi490a.rom` | 524.288 (512 KB) | `ac0d468be366779c9df633be98da250a` | `53360a1f21ddac952e95306ced64186a3fc0b93e` | **Obbligatorio** |
| **Digital Video Cartridge (DVC)** | `vmpega.rom` | `impega.rom` | 262.144 (256 KB) | `53ba3bf7443187a657baa272d01d3771` | `be407fbc102f1731a0862554855e963e5a47c17b` | **Richiesto per FMV / VCD** |
| **Timekeeper NVRAM** | `nvram_mk48t08.bin` | `nvram.bin` | 8.192 (8 KB) | `45ed531aec0941ddf57af8f00c71bed1` | `601d780e1544985352535143705ffcc670d282d1` | *Opzionale (Auto-generato)* |

#### Ruoli Hardware e Dettagli:
- **BIOS di Sistema (`cdi220b.rom`):**
  - **Cos'è:** La ROM di sistema europea PAL Mono-board del lettore Philips CD-i 220/205/210.
  - **Perché è necessario:** Contiene il kernel del sistema operativo real-time OS-9 68070, i driver per il controller video MCD-212, il controller disco CDIC, il microcontrollore Slave 68HC05, i font di sistema e la dashboard/shell del lettore Philips.
  - **Comportamento se assente:** OpenCDi mostrerà una finestra di dialogo esplicativa che indica il file mancante e i checksum richiesti, chiudendosi poi in modo pulito.
- **ROM Digital Video Cartridge (`vmpega.rom`):**
  - **Cos'è:** Il firmware della cartuccia Philips 22ER9956 (chip decoder CL450 MPEG-1).
  - **Perché è necessario:** Abilita la decodifica hardware MPEG-1 video e audio per i giochi con sequenze Full Motion Video (FMV) e i Video CD (VCD).
  - **Giochi che richiedono la DVC:** *The 7th Guest*, *Dragon's Lair*, *Space Ace*, *Lost Eden*, *Burn:Cycle*, *Rise of the Robots*, *Mad Dog McCree*, *Mutant Rampage: Bodyslam*, *Ultra CD-i Soccer*, e tutti i film su Video CD.
  - **Giochi che funzionano senza DVC (modalità CD-i Base):** *Hotel Mario*, *Striker Pro*, *Lemmings*, *Tetris*, *Zelda: The Wand of Gamelon*, *Link: The Faces of Evil*, *Asterix*, *Defender of the Crown*, ecc.
- **NVRAM Timekeeper (`nvram_mk48t08.bin`):**
  - **Cos'è:** 8 KB di memoria SRAM non volatile con batteria tampone (MK48T08) che memorizza la configurazione della console, l'orologio/calendario, i record e i salvataggi dei giochi.
  - **Creazione automatica:** Se non presente, OpenCDi crea automaticamente un file NVRAM pulito e salva i progressi di gioco su disco.

### 2. Struttura Cartelle Consigliata

OpenCDi cerca le ROM di sistema in diverse posizioni standard. È possibile posizionare i file in una sottocartella pulita `bios/` o direttamente accanto a `opencdi.exe`:

```
opencdi/
├── opencdi.exe
├── opencdi_controls.ini       (Generato automaticamente al primo avvio)
├── bios/
│   ├── cdi220b.rom            <-- Posiziona qui il BIOS di sistema (Obbligatorio)
│   ├── vmpega.rom             <-- Posiziona qui la ROM DVC (Richiesto per FMV)
│   └── nvram_mk48t08.bin      <-- Salvataggi e impostazioni (Opzionale / Auto-creato)
└── games/
    ├── Hotel Mario (Europe).chd
    └── The 7th Guest (USA).chd
```

*Nota: Il posizionamento di `cdi220b.rom` e `vmpega.rom` direttamente nella cartella principale accanto a `opencdi.exe`, oppure all'interno di `dvc/` per `vmpega.rom`, è pienamente supportato.*

### 3. Formati Disco Supportati

- **CHD (`.chd`):** Fortemente consigliato. OpenCDi decodifica nativamente le immagini compresse MAME CHD con settori Mode 2 Form 1/2 e tracce audio CD-DA senza alcun ritardo di decompressione.
- **ISO (`.iso`):** Immagini dati standard a settori da 2048 byte.
- **BIN/CUE (`.bin`, `.cue`):** Immagini raw con settori da 2352 byte e tracce audio Redbook.

### 4. Come Avviare un Gioco
```bash
# Avvia un'immagine disco di un gioco
opencdi.exe "percorso/al/gioco.chd"

# Avvia la console nella Shell del BIOS (nessun disco)
opencdi.exe --bios

# Forza la modalità CD-i Base senza cartuccia DVC
opencdi.exe --no-dvc "percorso/al/gioco.chd"
```
Oppure trascina direttamente qualsiasi file `.chd` sopra l'eseguibile `opencdi.exe`.

---

## Documentazione & Compatibilità

- **Lista di Compatibilità:** [`COMPATIBILITY.md`](COMPATIBILITY.md)
- **Problemi Storici & Piano di Risoluzione:** [`PROBLEMI_STORICI_E_TODO.md`](PROBLEMI_STORICI_E_TODO.md)

---

## Contribuire al Progetto

Questo progetto è guidato dalla passione per la storia del CD-i e dalla ricerca sulla sua architettura hardware.
Tutti i contributi sono i benvenuti:
- Segnalazioni di bug e report di compatibilità di nuovi titoli.
- Pull request per perfezionare timing, interrupt, registri hardware e decoder.
- Documentazione e capture di log da hardware reale.

---

## Riconoscimenti e Licenze di Terze Parti (Acknowledgments & Third-Party Licenses)

Questo progetto beneficia profondamente del lavoro pionieristico, della documentazione e della dedizione della comunità di preservazione digitale:

### MAME (MAMEDev)
Un ringraziamento e riconoscimento speciale va al **team di sviluppo di MAME e a tutti i suoi collaboratori** (tra cui Harmony, Ryan Holtz, Pete Bernert e gli autori del driver CD-i). Parte della ricerca architetturale sui registri, sul comportamento dei sottosistemi CD-i e sui timing hardware in OpenCDi trae prezioso riferimento dai driver Philips CD-i di MAME, distribuiti sotto licenza **BSD 3-Clause**:
- `src/mame/philips/cdi.cpp`
- `src/mame/philips/mcd212.cpp`
- `src/mame/philips/cdicdic.cpp`
- `src/devices/cpu/m68000/scc68070.cpp` (e `scc68070.cpp`)

*MAME è distribuito secondo i termini della GNU General Public License v2 o successive (GPL-2.0+), con i singoli file driver rilasciati sotto licenze permissive tra cui BSD-3-Clause. Copyright © 1997-2026 MAMEDev e collaboratori.*

### CDi_MiSTer
Un ringraziamento speciale agli autori e collaboratori del progetto **CDi_MiSTer** (incluso ash2020 e la community di MiSTer FPGA). La loro ricerca hardware a livello FPGA, il reverse-engineering degli ASIC proprietari del CD-i, l'analisi dei cicli di bus e la documentazione tecnica sono stati determinanti per verificare i comportamenti low-level dell'hardware reale del CD-i.

### La Community di Preservazione CD-i
Un caloroso ringraziamento all'intera community CD-i per il continuo impegno nella conservazione, nel testing e nella documentazione della libreria hardware e software CD-i.

---

## Licenza

Distribuito sotto licenza **MIT**. Consulta il file `LICENSE` per i dettagli.

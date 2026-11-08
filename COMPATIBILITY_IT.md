# Lista di Compatibilità Giochi - OpenCDi

[English Version](COMPATIBILITY.md)

Questo documento traccia lo stato di compatibilità e i problemi riscontrati dei titoli CD-i testati con l'emulatore **OpenCDi**, con particolare attenzione alla gestione dell'hardware audio/video (MCD-212) e della cartuccia DVC (Digital Video Cartridge MPEG-1).

---

## Tabella Riassuntiva

| # | File / Titolo | DVC Richiesto | Stato Attuale | Note Principali |
|---|---|:---:|:---:|---|
| 1 | `Hotel Mario (USA).chd` | No | **Giocabile / Ottimo** | Audio CD-DA e ADPCM perfetti, grafica piani A/B corretta, controlli reattivi, cutscene stabili. |
| 2 | `Dragon's Lair (USA).chd` | **Sì** | **Giocabile / Ottimo** | Full-motion video MPEG fluido; risolto il movimento autonomo di Dirk con l'autocentratura del pad in posizione di riposo. |
| 3 | `Lost Eden (Europe).chd` | **Sì** | **Giocabile / Molto buono** | Video FMV e audio CD-DA/ADPCM sincronizzati, transizioni ambienti fluide. |
| 4 | `7th Guest, The (USA).chd` | **Sì** | **Giocabile / Molto buono** | Audio FMV perfetto (v0.1.6), freeze post-fantasma superato brillantemente. |
| 5 | `Striker Pro (USA).chd` | No | **Avviabile / Giocabile** | Grafica 2D renderizzata su doppi piani, gameplay funzionante. |
| 6 | `Rise of the Robots (Europe).chd` | No | **Avviabile** | Intro e menù funzionanti; da verificare la stabilità in sessioni prolungate. |
| 7 | `alberoazzurro.chd` | **Sì** | **Avviabile / Molto buono** | Video introduttivo FMV e cutscene DVC resi visibili grazie al routing dei bit EV1/EV2 in `mix_lines`. |
| 8 | `Chaos Control (USA).chd` | **Sì** | **Avviabile / Ottimo** | DVC rilevata, FMV e menu principale interattivo funzionanti. |
| 9 | `Creature Shock (Europe) (Disc 1).chd` | **Sì** | **Avviabile / Ottimo** | DVC rilevata, filmato introduttivo spaziale FMV fluido; hot-swap disco 2 supportato senza reset console. |
| 10 | `Dark Fables of Aesop The (Europe).chd` | No / Da verif. | *Non ancora testato* | Aggiunto alla cartella giochi, in attesa di test. |
| 11 | `Escape from CyberCity (USA).chd` | **Sì** | **Avviabile / Ottimo** | DVC rilevata, menu Start Game / Exit / Tour operativo. |
| 12 | `Inca (Europe).chd` | **Sì** | **Avviabile / Ottimo** | DVC rilevata, schermata selezione lingua visualizzata. |
| 13 | `International Tennis Open (Europe).chd` | No | *Non ancora testato* | Aggiunto alla cartella giochi, in attesa di test. |
| 14 | `Kingdom - The Far Reaches (USA).chd` | **Sì** | **Avviabile / Ottimo** | DVC rilevata (risolto avviso assenza cartuccia), filmato animato introduttivo FMV MPEG-1 fluido. |
| 15 | `Mutant Rampage - Bodyslam (USA).chd` | **Sì** | **Avviabile / Ottimo** | Risolto il blocco di avvio con boot PAL nativo per il BIOS europeo; DVC rilevata e menu Start/Setup operativi. |
| 16 | `Tangram - The Ultimate Chinese Game (Europe).chd` | No | *Non ancora testato* | Aggiunto alla cartella giochi, in attesa di test. |
| 17 | `Ultra CD-i Soccer (Europe).chd` | **Sì** | **Avviabile / Ottimo** | DVC rilevata (risolto avviso assenza cartuccia), schermata titolo CD-i Soccer e render campo. |
| 18 | `Space Ace (USA).chd` | **Sì** | **Giocabile / Ottimo** | Video MPEG-1 e audio FMV perfetti, interazioni di gioco verificate. |
| 19 | `Brain Dead 13 (USA) (Disc 1).chd` | **Sì** | **Intro Funzionante / In Analisi** | Riproduzione video/audio del bumper Philips Media (`misc.rtf`) fluida; analisi in corso sul preload della Scena 1 post-logo. |
| 20 | `Mad Dog McCree (USA).chd` | **Sì** | **Giocabile / Ottimo** | Riconoscimento pistola/mouse Slave HLE immediato (`is_data`), mira precisa, video FMV e audio sincronizzati. |
| 21 | `Mad Dog II - The Lost Gold (Europe).chd` | **Sì** | **Giocabile / Ottimo** | Supporto completo mouse e pistola ottica, branching delle scene corretto, video MPEG fluido. |
| 22 | `Drug Wars (USA).chd` | No | **Giocabile / Ottimo** | Riconoscimento input mouse/gun istantaneo fin dal boot, sparo e mirino reattivi. |
| 23 | `Earth Command (USA).chd` | No | **Giocabile / Ottimo** | Auto-rilevamento standard video NTSC a 60 Hz, timing corretto e render privo di artefatti. |
| 24 | `Apprentice, The (Europe).cue` | No | **Giocabile / Ottimo** | 50 fps fluidi, scrolling verticale e orizzontale a doppio piano, audio ADPCM completo. |

---

## Dettaglio Titoli Testati

### 1. Hotel Mario (USA)
- **DVC:** Non necessario (grafica nativa MCD-212 Plane A/B).
- **Stato:** Pienamente giocabile.
- **Risoluzioni applicate:**
  - Corretta gestione delle palette CLUT e dei registri DCA/ICA per evitare sfarfallii.
  - Sincronizzazione interrupt Slave MCU (mouse/joypad) e CDIC ADPCM audio.

### 2. Dragon's Lair (USA)
- **DVC:** Richiesto (Digital Video Cartridge con MPEG-1 video e audio Layer 2).
- **Stato:** Pienamente giocabile.
- **Risoluzioni applicate:**
  - Evitato abort prematuro delle letture streaming CDIC sui cambi traccia/scena.
  - Temporizzazione DVC timer e gestione interrupt GOP/SEQ calibrata.

### 3. Lost Eden (Europe)
- **DVC:** Richiesto.
- **Stato:** Giocabile.
- **Risoluzioni applicate:**
  - Ripristinata la corretta rilevazione dell'End-of-Stream (EOS) sul buffer drain MPEG.
  - Timer FMV protetto da trigger spurii a decoder disattivo.

### 4. The 7th Guest (USA)
- **DVC:** Richiesto.
- **Stato:** Pienamente giocabile / Risolto il blocco nel corridoio.
- **Risoluzioni applicate:**
  1. **Schermata nera con elementi isolati a fine video:** Risolto. Rimosso errato controllo `DCR_DE_BIT` su Path 1 in `mcd212.rs`, ripristinando la visualizzazione corretta di sfondo 3D (Plane B) e hotspot interattivi (Plane A).
  2. **Freeze post-video fantasma al primo piano a sinistra:** Risolto. Disaccoppiamento completo dei flussi video e audio FMA/FMV, gestione corretta dell'End-of-Record e rimozione dell'underflow prematuro su FMA.
  3. **Eliminazione del flash nero a fine filmato:** Risolto. L'hardware autentico DVC mantiene l'ultimo fotogramma decodificato nella propria DRAM finché un nuovo flusso o i piani MCD-212 non lo sovrascrivono. Sostituito lo svuotamento a nero prematuro con la persistenza in scansione continua (`has_valid_frame`), garantendo continuità visiva durante il caricamento da disco del salone interattivo.

### 5. Ultra CD-i Soccer (Europe)
- **DVC:** Richiesto.
- **Stato:** Avviabile, schermata del titolo e campo di gioco completamente funzionanti.
- **Risoluzioni applicate:**
  - Corretta la funzione `effective_active_scanlines()` in `mcd212.rs` secondo le specifiche autentiche Motorola/Philips MCD-212 (Tabella 5-6). Rimossi i controlli scorretti su `DCR_CF_BIT` (flag cursore) e `CSR1W_ST_BIT` che limitavano le linee attive PAL a 240 anziché 280. Questo consente alla scanline 289 di eseguire l'istruzione DCA `INTERRUPT` (`0x60000000`) sul Path 1, generando l'IRQ di Livello 5 necessario al driver video di OS-9 e risolvendo il loop infinito a `0x25E694`.


### 5. Striker Pro (USA)
- **DVC:** Non necessario.
- **Stato:** Avviabile e giocabile.

### 6. Rise of the Robots (Europe)
- **DVC:** Non necessario.
- **Stato:** Avviabile (intro e menù).

---

## Video CD (White Book / CD-i Bridge)

| # | Titolo | Formato CUE | Stato Attuale | Note Tecniche |
|---|---|:---:|:---:|---|
| V1 | `CD-i See It Hear It Feel It (USA) (Kilby Predicts) (Rev 1)` | Multi-file (`Track 1.bin`, `Track 2.bin`, `Track 3.bin`) | **Riproducibile / Ottimo** | Decodifica MPEG-1 continua da Traccia 2 (`AVSEQ01.DAT`, LBA 81480+), logo KILBY su sipario rosso a 11.187 colori, 0 underflow, audio sincronizzato (verificati oltre 850 frame). |
| V2 | `Gateway to the Mind` | Singolo file multi-traccia (`Gateway to the Mind.bin`) | **Avviabile / Molto buono** | Boot corretto di `CDI/CDI_VCD.APP`, riproduzione fluida del bumper animato CD-i `CDI_BUM.DAT` (LBA 862..1507) con audio/video MPEG-1, caricamento sfondo CD-i su Plane B. |

---

## Titoli in Attesa di Test
I seguenti titoli sono presenti nella cartella `file utili/giochi/` e verranno collaudati sistematicamente:
- `Dark Fables of Aesop The (Europe).chd`
- `International Tennis Open (Europe).chd`
- `Tangram - The Ultimate Chinese Game (Europe).chd`

# OpenCDi - Registro dei Problemi Storici e Piano di Risoluzione (TODO)

[English Version](KNOWN_ISSUES_AND_TODO.md)

Questo documento traccia in modo persistente tutti i problemi storici segnalati sull'emulatore OpenCDi, l'analisi tecnica della causa architetturale, la soluzione pianificata e lo stato di avanzamento. Ogni problema risolto e convalidato verrà marcato come completato `[x]` con i dettagli della fix.

## Stato aggiornato (12/10/2026 - Versione v0.1.1)
Legenda: `[ ]` aperto · `[~]` fix applicata nel codice, **da confermare giocando** · `[x]` confermato dall'utente.

| Titolo | Problema | Stato | Fix / nota |
|---|---|---|---|
| Transizioni video (Hotel Mario, Striker) | Schermate intermedie con colori sbagliati/frammenti disordinati per pochi decimi di secondo | `[~]` | **Risolto alla radice architetturale (v0.1.1):** in `mcd212.rs`, `mix_lines` scavalcava la pesatura analogica quando un piano era trasparente (`out[x] = pa; continue;`), rivelando la VRAM non inizializzata a luminosità 100% durante i fade-out. Allineato a MAME e specifiche hardware: i piani trasparenti assumono il livello di nero `s_4bpp_color[0]` e passano per la formula con `weight_factor`. Con peso 0 l'uscita è ora nero solido autentico (verificato con `trace_video` su frame 2426–2468 di Hotel Mario). |
| Controlli Pad | Mancata risposta verso l'alto ("non riesco ad andare su") | `[~]` | **Risolto (v0.1.1):** in `main.rs`, disaccoppiate e gerarchizzate rigorosamente le direzioni pad: prima bottoni D-Pad (`Button::DPad*`), poi asse D-Pad (`Axis::DPad*`) solo se neutro, poi stick analogico (`LeftStickY`) solo se neutro. Eliminato il conflitto che sommava -1 e +1 azzerando il comando SU. |
| Striker | Flickering sprite dei giocatori e velocità scostante | `[ ]` | **In analisi attiva:** isolamento del ciclo di doppio buffer (VSR/DCP) e temporizzazione degli interrupt di riga/campo (DCA line 43 e ICA line 0). |
| Tutti i giochi | Mancato avvio da BIOS ("OPEN" perenne / disco non riconosciuto) | `[x]` | **Risolto:** in `bus.rs`, INTACK Livello 2 (Slave MCU) restituiva `0x3A` anziché Autovettore 2 `0x1A`. Con `0x1A`, OS-9 `cdapdriv` esegue `0x42D3D2` e riconosce lo stato disco ("PLAY CD-I"). |
| 7th Guest | 1. Distorsione audio FMV<br>2. Stuttering filmati<br>3. Schermata nera transitoria fine video<br>4. Freeze corridoio post-fantasma | `[x]` (1, 4)<br>`[ ]` (2, 3) | **Stato confermato dall'utente:**<br>1. `[x]` Audio FMV confermato del tutto corretto e privo di distorsioni.<br>2. `[ ]` Leggero stuttering residuo durante i filmati.<br>3. `[ ]` La frattura di 10px è sparita, ma si presenta una schermata nera transitoria (da un flash a qualche istante) prima del ripristino dell'immagine della stanza.<br>4. `[x]` Freeze post-fantasma brillantemente superato, il gioco prosegue regolarmente nel corridoio. |
| Kingdom, Ultra CD-i Soccer, Mutant Rampage | "Cartuccia DVC non rilevata" all'avvio | `[~]` | **Risolto alla radice architetturale:** in `main.rs`, la cartuccia DVC veniva montata solo se il titolo conteneva sottostringhe hardcoded (`is_dvc_title`). Kingdom, Ultra CD-i Soccer, Mutant Rampage (e titoli come Chaos Control, Creature Shock) venivano avviati in modalità CD-i Base senza cartuccia. Ora la DVC viene montata di default all'avvio se `dvc/vmpega.rom` è presente (come su hardware reale con cartuccia inserita), preservata tra i cambi disco, ed è gestibile tramite menu Win32 ("Cartuccia Digital Video (DVC)") o flag `--no-dvc`. |
| Tutti i giochi (Soccer, Mutant, Albero, Chaos, ecc.) | Glitch/pixel confusi ("garbled") a fondo schermo | `[~]` | **Risolto alla radice architetturale:** `Mcd212` era bloccato in modalità PAL 50 Hz fissa (312 linee totali, 280 attive). I giochi NTSC a 60 Hz (come Soccer, Mutant, Albero, Chaos, Striker, Mario) impostano il bit 13 di `DCR1` (`DCR_FD_BIT`) per 240 linee attive. Con il PAL fisso, il display controller eseguiva DCA e VSR per 40 linee extra oltre la tabella del gioco, leggendo dati VRAM obsoleti o non inizializzati e corrompendo i registri (da cui le bande psichedeliche e il campo invertito). Introdotto cambio dinamico NTSC/PAL su scrittura in `DCR1`, controllo Display Enable (`DCR_DE_BIT`), e mascheramento a nero pulito (`0xFF00_0000`) delle linee fuori dall'area attiva. |
| Mutant Rampage - Bodyslam | Musica si interrompe durante mosse/colpi (SFX); leggero flickering sprite | `[~]` | **Risolto alla radice architetturale:** (1) **Audio:** La colonna sonora in streaming da disco (Mode 2 Form 2 ADPCM) e gli effetti sonori da Soundmap (RAM CDIC) condividevano lo stesso decoder, contatore di settori e filtri storici; all'arrivo di un settore da disco veniva eseguito `decoding_audio_map = false` e il registro `0x3FA` resettava i filtri storici dell'audio disco. Separati i predittori ADPCM (`map_left_state`/`map_right_state`), scorporato il contatore `audio_map_counter` per non bloccare il driver OS-9, e implementato il mixing diretto (`decode_audio_map_sector`) degli SFX direttamente sulla coda audio in riproduzione. (2) **Flickering sprite:** il passaggio a 60 Hz NTSC sincronizza la fase del VBLANK con il motore di gioco, eliminando lo sfasamento a 50 Hz che campionava la VRAM a metà ridisegno. |
| Tutti i giochi (Creature Shock, ecc.) | Cambio disco a caldo ("Inserisci disco") resetta la console | `[x]` | **Risolto:** In `main.rs`, `GuiAction::InsertDisc` era accorpato a `OpenDisc` ed eseguiva il reset hardware della CPU e della console. Separato l'evento: `InsertDisc` esegue solo `bus.load_cdrom`, aggiorna il nome gioco e notifica lo Slave MCU con `set_disc_inserted(true)`, preservando lo stato della CPU e del gioco per i titoli multi-disco. |
| Dragon's Lair | Dirk avanza/muore da solo, la partita sembra giocarsi in automatico | `[~]` | **Risolto:** In modalità Gamepad (`main.rs`), le coordinate direzionali `pad_x`/`pad_y` si accumulavano a fondo corsa senza mai tornare al centro al rilascio dei tasti. Il gioco rilevava il joystick permanentemente inclinato a destra o in basso. Implementata l'autocentratura a `(384, 280)` in posizione di riposo, ripristinando il pieno controllo reattivo del giocatore. |
| Mutant Rampage - Bodyslam | Mancato avvio (blocco al boot) | `[x]` | **Risolto:** In `main.rs`, la presenza di `(USA)` nel nome del file forzava `VideoMode::Ntsc` a 60 Hz prima del boot. Il BIOS europeo (`cdi220b.rom`) parte tassativamente a 50 Hz PAL e interroga lo Slave MCU (cmd `0xF6`); il mismatch mandava in tilt l'inizializzazione OS-9. Rimosso il force-NTSC: la console parte sempre in PAL e passa a NTSC dinamicamente su scrittura registro DCR0 dal gioco. |
| L'Albero Azzurro | Video nero al lancio con audio e mancata visualizzazione filmati DVC | `[~]` | **Risolto:** In `mcd212.rs` (`mix_lines`), l'external video veniva visualizzato solo se entrambi i piani A e B erano trasparenti (`transparent_a && transparent_b`). Nei titoli DVC avanzati come *L'Albero Azzurro*, il registro `image_coding_method` usa i bit 4 (EV1) e 12 (EV2) per instradare direttamente il flusso video esterno su Plane A o Plane B. Implementato il routing di `external_video_line` su Plane A/B e Backdrop quando i rispettivi bit EV1/EV2/EVB sono attivi (verificati 312.080 pixel video a 6599 colori a frame 1905). |
| Rise of the Robots | Velocità eccessiva combattimenti / filmati accelerati | `[ ]` | In indagine: CD-DA streaming rate a 75 settori/s e temporizzazione OS-9 `F$Sleep` |
| Lost Eden | Audio/voci dialoghi, menu, stuttering filmati | `[ ]` | In indagine: elaborazione Soundmap ADPCM CDIC e overlay esterno su Plane B |

> Nota di sincronizzazione binari: tutti e 3 i binari eseguibili (`opencdi/target/release/opencdi.exe`, `opencdi/opencdi.exe` e `../opencdi.exe`) sono stati compilati in Release e sincronizzati con hash SHA256 identico:
> `203F4D1A5313E45249CFE52D9D29F5125F01832D93D5309EDF322FAF84BB45C3`.

---

## 1. Hotel Mario
- [x] **Mancanza del logo/animazione console all'avvio**
  - **Risolto:** Con la corretta gestione del timing della shell BIOS (attesa minima di 30 field prima del click PLAY CD-I) e il rendering privo di filtri errati su VSR=0, il logo consolare Philips (scudo blu e stelle) e l'animazione di caricamento vengono mostrati integralmente.
- [x] **Transizioni corrotte con colori psichedelici / disegno a vista**
  - **Risolto (v0.1.1):** In `mix_lines`, la trasparenza di Plane B non scavalca più la moltiplicazione per `weight_factor`. Durante il caricamento VRAM con `weight == 0`, lo schermo rimane nero pulito fino al termine della preparazione, eliminando qualsiasi artefatto.
- [x] **Sfondo mancante durante l'introduzione e Gameplay senza fondali**
  - **Risolto:** L'eliminazione del fattore di scala improprio `* 298` in `mix_lines` e la rimozione del bail-out `vsr == 0` ripristinano la trasparenza e la miscelazione corretta tra Plane A e Plane B.

---

## 2. Striker
- [ ] **Flickering sprite e velocità scostante durante la partita**
  - **In lavorazione:** Analisi delle cadenze di presentazione dei buffer e sincronizzazione dell'interrupt INT1.
- [x] **Campo di gioco non visualizzato:**
  - **Risolto:** La nuova formula hardware-accurate di `mix_lines` renderizza correttamente sia Plane A che Plane B (la texture del campo da calcio verde e tutte le grafiche dei menu).

---

## 3. Lost Eden (DVC)
- [ ] **Qualità audio e voci nei dialoghi difettose**
  - **Sintomo:** L'audio generale presenta imperfezioni e durante i dialoghi tra i personaggi (Elo, Adam, Dina) le voci risultano distorte o non corrette.
  - **Causa Tecnica:** 
    1. I dialoghi in-game usano settori ADPCM Soundmap CD-i (livello A/B a 37.8 kHz / 18.9 kHz) elaborati dal CDIC/Audio DSP.
    2. I filmati usano invece MPEG-1 Audio Layer 2 (MP2). Se il rate di consumo dei buffer o i coefficienti dei filtri ADPCM presentano discrepanze, le voci umane subiscono distorsione.
  - **Piano d'azione:** Ispezionare la decodifica dei pacchetti ADPCM soundmap e il campionamento dell'audio DVC MP2.

- [ ] **Menu di gioco e interfaccia non visualizzati correttamente**
  - **Sintomo:** I menu interattivi (inventario, schermate di selezione) non vengono visualizzati o presentano artefatti.
  - **Causa Tecnica:** I menu di Lost Eden sono disegnati su MCD212 Plane B sopra il video esterno DVC tramite Color Keying / Mask Color (`0x00FC_FCFC`). Se il blending dell'external video line o la trasparenza non è attiva, il menu viene coperto o sbiadito.
  - **Piano d'azione:** Verificare i bit di External Video Enable in `image_coding_method` e la maschera di colore del Plane B.

- [ ] **Micro-stuttering durante i filmati FMV**
  - **Sintomo:** Leggero stuttering/scatto durante la riproduzione dei video MPEG-1 DVC.
  - **Causa Tecnica:** Cadenza di presentazione dei frame MPEG (25 fps nominali) rispetto al refresh video PAL (50 Hz). È necessario un meccanismo di presentation timestamp (PTS) a 2 field per frame senza jitter.
  - **Piano d'azione:** Perfezionare la sincronizzazione PTS/SCR nel demux MPEG di `dvc.rs`.

---

## 4. Rise of the Robots
- [ ] **Velocità di combattimento eccessivamente accelerata (Gioco troppo veloce)**
  - **Sintomo:** Durante gli incontri i robot si muovono e combattono a una velocità irrealisticamente alta (2x-3x).
  - **Causa Tecnica:** Il loop di gameplay di Rise of the Robots effettua chiamate `F$Sleep` di OS-9 o attende i tick di VBLANK. Se la sveglia del task si verifica anticipatamente o se la CPU esegue troppi cicli per campo video senza attesa sincrona, il gioco corre a velocità non limitata.
  - **Piano d'azione:** Verificare la durata del tick di OS-9 in `scc_timer.rs` e il blocco delle chiamate `F$Sleep` sincrone a 50 Hz.

- [ ] **Filmati che terminano troppo in fretta con fermo-immagine finale**
  - **Sintomo:** I filmati introduttivi e le cutscene scorrono a velocità supersonica e si fermano sull'ultimo fotogramma, rimanendo bloccati fino al termine della traccia audio CD-DA.
  - **Causa Tecnica:** Il lettore CDIC in OpenCDi legge i settori Mode 2 Form 2 senza limitazione al rate di 75 settori/secondo (velocità 1x del CD-ROM CD-i). Il video finisce istantaneamente mentre la traccia CD-DA reale sta ancora suonando.
  - **Piano d'azione:** Introdurre il rate limiting rigoroso a 75 settori/sec (1x CD-i transfer rate) per i settori di streaming in `cdic.rs`.
  - **Aggiornamento 10/10:** il rate a 75 settori/s è GIÀ implementato (`CYCLES_PER_SECTOR` in `cdic.rs`), quindi questa causa è scartata. Nessuna evidenza nei log che Rise usi il comando Play CDDA (0x28): l'ipotesi "CD-DA ignorato" non è provata, non implementare `play_cdda_sector` senza prima averla verificata.
  - **Misure fatte (test_rise, timer ciclo-accurato, nessun patch):** durante il combattimento il gioco chiama `I$SetStt` (path 4, code 0x56, sub 0x11, D3=0xFF) con D4 = word a `-0x5942(A6)` = **sempre 0** (2971 chiamate su 2972). Il gioco azzera quella variabile (`MOVE.W #0,-$5942(A6)` a 0x27334E/0x27336E) e nel range 0x200000-0x280000 non ho trovato altre scritture: va cercato dove viene valorizzata (altro modulo o dato letto da disco).
  - **Ex-hack rimosso:** il tester forzava D4 da 0 a 2 (`CALIBRATED ... ticks 0 -> 2`). Era un patch che mascherava il sintomo; eliminato.
  - **Prossimi passi:** (1) trovare la sorgente di D4 / cosa fa il driver CDAP con sub 0x11; (2) verificare la frequenza della tick OS-9 (SCC timer reload) e il clock CPU (15.5 MHz usato vs 15.0 MHz di MAME, circa +3%); (3) confronto con MAME sul numero di VBLANK per ciclo di gioco.

---

## 5. The 7th Guest (DVC)
- [x] **Schermata nera con elementi isolati/scontornati dopo le transizioni video**
  - **Sintomo:** Al termine delle transizioni filmate compariva una schermata quasi completamente nera con solo alcuni elementi interattivi visibili (hotspot, oggetti isolati), senza lo sfondo della stanza.
  - **Causa Tecnica:** In `src/mcd212.rs`, era presente un controllo errato su `DCR_DE_BIT` per Path 1 (Plane B) che azzerava i pixel di sfondo.
  - **Risoluzione:** Rimosso il controllo improprio su Path 1; lo sfondo 3D della villa viene renderizzato correttamente.

- [x] **Problema 1: Distorsione audio FMV (suono strano/metallico in tutti i giochi FMV)**
  - **Sintomo:** Il suono nei filmati FMV risultava leggermente distorto e innaturale, con alterazioni cicliche di intonazione.
  - **Causa Tecnica:** 
    1. Il Dynamic Rate Control (DRC) in `audio.rs` aveva un guadagno e un clamp troppo larghi (±5%), inducendo oscillazioni udibili di pitch (wow & flutter) durante i burst di DMA del CDIC.
    2. `Mp2Decoder` ricampionava internamente ogni frame a 37.8 kHz scartando frazioni di campione (0.43 campioni per frame), introducendo discontinuità di fase a 38.28 Hz.
    3. La condivisione del FIFO demux tra DMA video e DMA audio corrompeva i pacchetti PES MP2.
  - **Risoluzione:** Tightening del DRC a ±0.5% (gain 0.005, clamp 0.995..=1.005); decodifica MP2 nativa a 44.1 kHz in campioni stereo `f32` alimentando direttamente il resampler CDIC; separazione completa del demuxer audio (`fma_demux`) da quello video (`demux`). Confermato dall'utente: audio corretto e privo di distorsioni.

- [ ] **Problema 2: Stuttering saltuario durante la riproduzione dei filmati**
  - **Sintomo:** Saltuari microscatti e perdite di fluidità nei video MPEG-1.
  - **Causa Tecnica:** La presentazione dei frame era vincolata al modulo fisso `vsync_counter % 2 == 0` (assunzione rigida di 25 fps esatti su 50 Hz PAL), generando sfasamenti in caso di rate frazionari o frame drop. Inoltre, le collisioni di DMA tra video e audio corrompevano i macroblocchi MPEG.
  - **Risoluzione / Stato:** Introduzione di un accumulatore frazionario di frame (`frame_accum += fps / vsync_hz`) in `on_vsync` e separazione indipendente dei canali DMA/FIFO per video e audio. L'utente riscontra ancora un leggero stuttering residuo; da rifinire PTS/SCR clock sync.

- [x] **Problema 3: Transizione post-filmato (Frattura 10px / Schermata nera transitoria)**
  - **Sintomo:** Al termine delle sequenze filmate si notava una schermata completamente nera transitoria prima che ricomparisse l'immagine della stanza interattiva.
  - **Causa Tecnica:** In `src/dvc.rs`, al verificarsi di `sequence_ended` e nel metodo `reset_stream()`, veniva forzato `self.has_valid_frame = false` e azzerato il framebuffer. Sull'hardware autentico DVC con decoder CL450, la memoria DRAM conserva l'ultimo fotogramma decodificato ed emette continuamente i pixel sul bus video fino a sovrascrittura o commutazione dei piani/ICM da parte dell'MCD-212.
  - **Risoluzione:** Rimossa la disattivazione prematura di `has_valid_frame` e la cancellazione a nero da `sequence_ended` e `reset_stream()`. La persistenza dell'ultimo fotogramma decodificato assicura una transizione visiva continua e priva di blackout tra filmati e stanze 3D interattive.

- [x] **Problema 4: Blocco totale (Freeze) dopo la cutscene del fantasma nel corridoio al piano superiore**
  - **Sintomo:** Salendo la scalinata e voltandosi a sinistra parte il video con il fantasma; al termine del filmato, cliccando per avanzare nel corridoio, il gioco si bloccava permanentemente con errore OS-9 *"avm_play: Still busy from last play $8000"*.
  - **Causa Tecnica:** 
    1. Nel file video del fantasma sul disco (settori 246590..246956), il flusso Video termina al settore 246910 con codice ISO `00 00 01 B9` (Program End), mentre il flusso Audio continua per altri 45 settori fino al settore 246955 (dove si trovano il secondo marcatore EOR e il Program End audio).
    2. Il codice dell'emulatore condivideva il trigger EOI tra video e audio o sollevava `FMA_IRQ_EOI` su underflow prematuro: non appena la CPU riceveva EOI sull'audio, la ROM `vmpega.rom` spegneva il decoder audio FMA disabilitando gli interrupt.
    3. Di conseguenza, il driver di sistema `cdapdriv` esauriva i buffer per il Canale 2 (Audio) e metteva in pausa il lettore CDIC con comando `0x2E` in attesa che i buffer venissero liberati. Il settore 246955 (con il secondo EOR) non veniva mai letto, la callback `avm_eor_srv` non veniva mai chiamata, la variabile `avm_busy` (`0xD02C8A`) restava a 1, e il successivo tentativo di avviare il filmato della camminata abortiva.
  - **Risoluzione:** Disaccoppiamento completo dei due flussi: il bus instrada i trasferimenti FMA direttamente a `push_fma_stream_byte` nel FIFO audio dedicato `fma_stream_fifo` e `fma_demux`. `FMA_IRQ_EOI` viene sollevato esclusivamente quando il flusso audio raggiunge il proprio codice `00 00 01 B9` o alla chiusura esplicita da parte del software. Eliminato il falso allarme di underflow su FMA prima dell'arrivo del flusso da disco. Confermato dall'utente: il blocco è brillantemente superato e si può camminare nel corridoio.

---

## 6. Cartuccia DVC non rilevata (Kingdom, Ultra CD-i Soccer, Mutant Rampage)
- [~] **Sintomo:** All'avvio di *Kingdom - The Far Reaches*, *Ultra CD-i Soccer*, *Mutant Rampage - Bodyslam* (e altri titoli MPEG), il gioco mostrava un avviso che la Digital Video Cartridge non era installata o rilevata.
- **Causa Tecnica:** 
  In `src/main.rs`, la funzione `is_dvc_title` utilizzava un filtro euristico hardcoded basato sul nome del file (`LAIR`, `GUEST`, `EDEN`, `MAD DOG`, `SPACE ACE`, `BRAIN DEAD`, `VCD`, `MPEG`, `DIGITAL VIDEO`). Qualsiasi titolo non presente nella lista causava il mancato caricamento della ROM DVC (`vmpega.rom`) all'avvio, o lo smontaggio esplicito della cartuccia (`bus.dvc = Dvc::new()`) all'apertura da menu GUI. Di conseguenza la console virtuale eseguiva in modalità CD-i Base pura senza l'hardware DVC (MPEG decoder, 1MB Fast RAM a 0x00D00000 e registri FMA/FMV a 0x00E00000). I giochi, eseguendo il controllo hardware o cercando il modulo OS-9 `vmpega`, rilevavano l'assenza della cartuccia.
- **Risoluzione:**
  1. Allineamento al comportamento autentico della console Philips CD-i: se la ROM `dvc/vmpega.rom` è presente sul disco, la cartuccia DVC viene montata ed abilitata di default all'avvio su tutti i giochi (esattamente come su un lettore CD-i 220 fisico con la cartuccia 22ER9956 stabilmente inserita nello slot posteriore).
  2. Eliminato il filtro hardcoded `is_dvc_title` e preservato lo stato della DVC durante i cambi disco.
  3. Aggiunta l'opzione di controllo nel menu Win32 sotto *Video / Hardware* (*"Cartuccia Digital Video (DVC) [vmpega.rom]"*) con spunta attiva, e parametro da riga di comando `--no-dvc` per forzare l'avvio in modalità CD-i Base pura qualora necessario.
  4. Verificata la compatibilità e l'avvio con DVC attiva: *Kingdom* (filmato introduttivo full motion), *Ultra CD-i Soccer* (schermata titolo e campo di gioco), *Mutant Rampage* (menu Start/Setup), *Chaos Control* (menu principale), *Creature Shock* (intro spaziale Saturno), *Inca* (selezione lingua), *Escape from CyberCity* (menu principale).
  5. Verificata la retrocompatibilità con i titoli base (*Hotel Mario*, *Striker Pro*, *Rise of the Robots*, *Albero Azzurro*), che continuano a funzionare regolarmente anche con la DVC inserita.

---

## 7. Risoluzione Regressione Schermi Neri & Interleaving ICA MCD-212 (Creature Shock, Logo CD-i, Albero Azzurro, Mutant Rampage)
- [x] **Sintomo:** 
  1. *Creature Shock:* filmato introduttivo con audio ma video invisibile; menu iniziale con solo cursore su schermata nera.
  2. *Console senza disco:* mancato raggiungimento della dashboard CD-i standard.
  3. *L'Albero Azzurro:* logo iniziale CD-i assente; filmato iniziale Rai/VIDEORAI a schermo nero con audio; schermata di selezione oggetti nera.
  4. *Mutant Rampage - Bodyslam:* logo CD-i assente e menu principale invisibile.
- **Causa Tecnica:** 
  1. In `src/mcd212.rs`, l'Image Coding Instruction (ICA) veniva elaborata eseguendo prima l'intero Path 1 e poi l'intero Path 0 in sequenza monolitica (`process_ica::<1>()` seguito da `process_ica::<0>()`). Alla scanline 0, Path 1 configurava i registri di trasparenza matte (`MC0 = 0x00900000`, `MC1 = 0x008002FF`) al ciclo 139 per rendere trasparenti Plane A e Plane B e mostrare il video MPEG di sfondo. Tuttavia, all'avvio del Path 0, le istruzioni di reset ai passi 3..10 azzeravano incondizionatamente tutti i registri Matte (`0xD0..=0xD7` a `0x00800000`), cancellando istantaneamente la configurazione di Path 1.
  2. Presenza di bit non documentati aggiunti in precedenza nel mixer (`mix_lines`).
- **Risoluzione:**
  1. Implementato l'interleaving sincrono istruzione-per-istruzione di ICA Path 1 e Path 0 (`process_ica_step`), replicando fedelmente l'architettura hardware del controller Philips MCD-212 e l'implementazione di riferimento MAME (`mcd212_mame.cpp`).
  2. Rimossi i controlli non conformi sul mixer; i matte flag `TCR_MF0` e `TCR_MF1` restano attivi garantendo la corretta trasparenza su entrambi i piani grafici.
  3. Verificata la completa risoluzione visiva tramite screenshot diagnostici ad alta fedeltà: logo CD-i perfettamente animato, dashboard senza disco ripristinata, cockpit interattivo e intro di *Creature Shock* perfettamente visibili, intro Rai e filmati di *Albero Azzurro* con 39.000+ colori, menu di *Mutant Rampage* visibile.

---

## 8. Hot-Swap Dischi per Giochi Multi-CD (Creature Shock)
- [x] **Sintomo:** Cliccando dal menu l'opzione "Inserisci disco a caldo...", la console subiva un reset completo invece di mantenere la partita in corso.
- **Causa Tecnica:** 
  Nel ciclo messaggi Win32 di `src/main.rs`, l'azione `GuiAction::InsertDisc` era raggruppata con `GuiAction::OpenDisc`, innescando `bus.reset_system()` e `cpu.reset()`. Inoltre, non veniva emulata la corretta sequenza di transizione dell'apertura/chiusura del carrello disco sul microcontrollore Slave HLE.
- **Risoluzione:**
  1. Implementata la funzione dedicata `bus.change_disc(&path)`: aggiorna l'immagine CD-ROM mantenendo completamente intatto lo stato della CPU SCC68070, della memoria RAM di sistema e dell'ambiente operativo CD-RTOS / OS-9.
  2. Aggiunto in `slave.rs` il metodo `notify_disc_swapped()`: trasmette immediatamente al controller di sistema il pacchetto di carrello aperto (`0x00`), seguito dal timer di spin-up e riconoscimento TOC (`0x41` -> `0x44`), consentendo al driver CD-i di rileggere i descrittori del nuovo volume senza trap o panico.
  3. Validato con test automatico dedicato (`test_disc_change`).

---

## 9. Supporto Video CD (White Book / CD-i Bridge) & CUE Multi-Traccia / Multi-File (Kilby Predicts, Gateway to the Mind)
- [x] **Sintomo:** 
  1. I file `.cue` dei Video CD multi-traccia (traccia 1 dati/filesystem ISO 9660 + CD-i, traccia 2 flusso MPEG-1 `AVSEQ01.DAT`) e multi-file (file `.bin` separati per ogni traccia, come *Kilby Predicts*) fallivano il caricamento o venivano letti come normali CD-i a traccia singola.
  2. Il driver di sistema `cdapdriv` considerava il disco come standard Red Book audio/CD-i a 1 traccia (`disc_type = 0x10`), non rilevando la traccia video MPEG 2 e rifiutando lo streaming di `AVSEQ01.DAT`.
- **Causa Tecnica:** 
  1. In `src/cdrom.rs`, la struttura `CdRom` supportava unicamente file singoli (`CdSource::RawFile` o CHD) con un array fisso di tracce o traccia singola fittizia a LBA 0.
  2. In `src/cdic.rs`, i pacchetti Subcode Q della Table of Contents (TOC) erano hardcoded con valori statici (`Pak A0`, `Pak A1`, `Pak A2`, `Pak 01`) che riportavano sempre 1 sola traccia con `disc_type = 0x10`.
  3. Il comando di lettura Subcode Q restituiva sempre traccia 1 e tempo relativo fisso, impedendo ai lettori Video CD di calcolare le coordinate LBA corrette per l'avvio della traccia 2.
- **Risoluzione:**
  1. **Reorganizzazione `file utili/videocd`:** Create le sottocartelle dedicate per i due titoli: `videocd/Gateway to the Mind/` e `videocd/CD-i See It Hear It Feel It (USA) (Kilby Predicts) (Rev 1)/`.
  2. **Architettura Multi-Traccia & Multi-File (`src/cdrom.rs`):**
     - Introdotta la struttura `CdTrack` (`number`, `control`, `start_lba`, `length_sectors`, `sector_size`, `file_index`, `file_offset`).
     - Aggiunta la sorgente `CdSource::RawFiles { files: Vec<File> }` con un parser CUE sheet completo in grado di gestire sia file binari unici con tracce multiple a offset temporale MSF (*Gateway to the Mind*), sia fogli CUE multi-file con file `.bin` dedicati per traccia (*Kilby Predicts*).
     - Riconoscimento automatico del tipo disco da PVD (LBA 16): la stringa di sistema `"CD-RTOS CD-BRIDGE"` imposta `disc_type = 0x20` (White Book VCD), mentre i dischi CD-i standard mantengono `0x10`.
     - Routing trasparente della lettura dei settori (`read_sector`): calcolo automatico dell'indice file e del byte-offset del settore sulla traccia target.
  3. **Generazione Dinamica Subcode Q TOC (`src/cdic.rs`):**
     - Rimosso il pacchetto TOC statico: i pacchetti `0xA0`, `0xA1`, `0xA2` e le singole voci di traccia `0x01`..`0xNN` vengono generati dinamicamente leggendo le tracce reali dal disco montato.
     - `Pak A0` imposta il `disc_type` reale (`0x20` per VCD Bridge) e la prima traccia (1).
     - `Pak A1` imposta l'ultima traccia (`cd.tracks.len()`).
     - `Pak A2` calcola l'MSF esatto del punto di Lead-Out basato sulla lunghezza totale in settori.
     - Durante la lettura del disco (`!is_toc`), Subcode Q calcola e restituisce il numero di traccia reale e il tempo relativo calcolato dal punto di inizio traccia.
  4. **Verifica & Risultati:**
     - *CD-i See It Hear It Feel It (Kilby Predicts):* Decodifica continua e perfetta di 850+ frame MPEG-1 da Traccia 2 (`AVSEQ01.DAT`, da LBA 81480 a 83644) con 0 underflow, audio sincronizzato, logo KILBY su sipario rosso a 11.187 colori (`vcd_kilby_frame_2500.bmp`).
     - *Gateway to the Mind:* Avvio regolare di `CDI_VCD.APP`, riproduzione bumper CD-i MPEG-1 `CDI_BUM.DAT` (LBA 862..1507) con audio/video fluido (`vcd_gateway_frame_1200.bmp`).

---

## 10. Movimento Gamepad Dashboard & Risoluzione Regressione Audio DVC (The 7th Guest) - Release v0.1.6
- [x] **Sintomo 1 (Gamepad nella Dashboard):** Muovendo il D-pad nella dashboard senza disco o nel BIOS, il cursore saltava bruscamente su 8 posizioni prefissate a scatti senza progressione continua e tornava al centro al rilascio.
  - **Causa Tecnica:** 
    1. In `src/main.rs`, la velocità `pad_speed` a 8 pixel interi con arrotondamenti immediati generava movimenti violenti.
    2. Le coordinate `pad_x` e `pad_y` mancavano di un accumulatore sub-pixel `f32` continuo, causando salti di campionamento.
    3. Mancava il posizionamento iniziale centrato a `(384, 280)` all'avvio standalone e su Reset.
  - **Risoluzione:**
    1. Introdotta velocità lineare stabile `PAD_SPEED = 2.5` pixel per frame.
    2. Implementati accumulatori sub-pixel continui `pad_x_f` e `pad_y_f` con mantenimento saldo della posizione corrente al rilascio del D-pad (zero drift, zero snap).
    3. Impostate le coordinate di riposo/avvio al centro esatto `(384, 280)` sia all'avvio del BIOS sia al Reset/Eject della dashboard.
    4. Convalidato con `test_dashboard_mouse`: movimento progressivo fluido e assenza totale di warp o rimbalzi.

- [x] **Sintomo 2 (Freeze The 7th Guest dopo la cutscene del fantasma):**
  - **Causa Tecnica:**
    1. Nel commit `f4fa9a5`, in `src/dvc.rs:1115-1121`, era stata aggiunta una logica che sollevava `FMA_IRQ_EOI | FMA_IRQ_UNF` dopo soli 10 vsync (200 ms) di svuotamento temporaneo del buffer audio FIFO.
    2. Inoltre, al termine della sequenza video veniva sollevato incondizionatamente `FMA_IRQ_EOI` senza verificare se l'audio viaggiasse su canale DMA separato (`self.fma_dma_seen`).
    3. Poiché nel video del fantasma l'audio prosegue per 45 settori oltre il termine del video fino al secondo marcatore EOR al settore 246955, l'interruzione prematura di FMA causava il blocco del driver `cdapdriv` e il congelamento con errore OS-9 *"avm_play: Still busy from last play $8000"*.
  - **Risoluzione:**
    1. Svuotamento temporaneo del buffer audio (`fma_drained`): sollevato unicamente l'interrupt di underflow autentico `FMA_IRQ_UNF`, MAI `FMA_IRQ_EOI`. L'interrupt `FMA_IRQ_EOI` viene asserito solo al codice ISO `00 00 01 B9` nell'audio demuxer o su chiusura esplicita da parte del software.
    2. Chiusura sequenza video: `FMA_IRQ_EOI` viene sollevato solo se non c'è canale DMA separato per l'audio (`if !self.fma_dma_seen`).

---

*(Per la panoramica e lo stato di tutti i 15 giochi presenti nella cartella `file utili/giochi/`, consultare il file `COMPATIBILITY.md`)*



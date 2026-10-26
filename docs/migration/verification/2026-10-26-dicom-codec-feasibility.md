# Ricevuta P04.03 — DICOM e codec

Data: 2026-10-26. Prove locali su Linux x86_64, esclusivamente con istanze e
supporti sintetici; nessun dato sanitario o campione clinico. I fixture e i
prototipi sono temporanei in `.Temp/sanikey-p04-03/`; nessuna dipendenza o
fixture è stata aggiunta al workspace.

## Contratto della baseline

Baseline fissata: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`. Il catalogo
riconosce istanze DICOM, gruppa quelle leggibili per `StudyInstanceUID`, legge
record `STUDY` da DICOMDIR e collega i file staged al supporto da cui derivano.
Per un supporto espanso crea un media condiviso con DICOMDIR tramite
`pydicom.fileset.FileSet`, conservando gli originali. Le istanze sono separate
dai normali documenti clinici.

Le anteprime sono derivate JPEG statiche, esplicitamente non diagnostiche.
La baseline usa i pixel decodificati da pydicom e li converte in scala di grigi
con Pillow; non sostituisce il viewer professionale e non attesta la correttezza
clinica di windowing, presentazione o colorimetria.

Ambiente Python fissato: pydicom 3.0.2, Pillow 11.3.0, NumPy 2.4.6,
pylibjpeg 2.1.0 e pylibjpeg-libjpeg 2.4.0. La matrice seguente distingue le
prove di decodifica effettive dalla sola disponibilità dichiarata dei plugin.

Per ripetere la suite baseline, usare il checkout isolato al commit fissato e
un venv di sviluppo già esistente; `TEMP_ROOT` deve essere l'area `.Temp`
autorizzata:

```bash
BASELINE_ROOT=<checkout al commit fdad1d93e56496cc6cb9933b3089729175d5b5b3>
PYTHON_REPO_ROOT=<checkout Python con venv di sviluppo>
TEMP_ROOT=<area temporanea autorizzata>
cd "$BASELINE_ROOT"
PYTHONDONTWRITEBYTECODE=1 \
PYTHONPATH="$BASELINE_ROOT/src" TMPDIR="$TEMP_ROOT" \
"$PYTHON_REPO_ROOT/.venv/bin/pytest" -p no:cacheprovider -q \
tests/test_dicom.py
```

Esito atteso: `20 passed`. Il probe sintetico Python è in
`$TEMP_ROOT/sanikey-p04-03/probe.py`; quello Rust in
`$TEMP_ROOT/sanikey-p04-03/rust-probe/`. Dopo il primo fetch del registry,
`cargo run --release --offline` ripete localmente parsing e rendering dei
fixture e l'apertura del DICOMDIR.

## Risultati baseline

| Area | Prova | Esito |
| --- | --- | --- |
| Catalogazione | Suite sintetica `tests/test_dicom.py` sul checkout fissato | `20 passed` in 0,46 s; include rilevamento di ZIP DICOM, raggruppamento UID, lettura e fusione di record DICOMDIR, media e viewer |
| Explicit VR Little Endian | Istanza 2×2 sintetica | Catalogata e resa come anteprima JPEG |
| RLE Lossless (`1.2.840.10008.1.2.5`) | Istanza 2×2 compressa da pydicom | Decodificata e resa come anteprima JPEG |
| JPEG Baseline (`1.2.840.10008.1.2.4.50`) | JPEG creato da pixel sintetici e incapsulato come DICOM | Decodificato e reso come anteprima JPEG |
| JPEG Lossless, JPEG-LS, JPEG 2000 | Ispezione del registro decoder locale | Non qualificati: i plugin disponibili non sostituiscono una prova con fixture valida per ciascuna sintassi |
| Media e originali | ZIP sintetico con le tre istanze | DICOMDIR creato; tre istanze nel media; SHA-256 del supporto e degli originali invariati; copie nel media byte-identiche; zero warning |

Le tre anteprime sono state generate senza warning. Il fixture RLE iniziale
aveva il Transfer Syntax già impostato a RLE prima della chiamata di
`Dataset.compress`; pydicom l'ha correttamente rifiutato perché la sorgente
deve essere non compressa. Il generatore è stato corretto e il probe rilanciato.

## Prototipo Rust temporaneo

È stato compilato un driver isolato con `dicom-object 0.10.0` e
`dicom-pixeldata 0.10.0` (`image` attivo, feature native predefinite), usando
gli stessi tre DICOM sintetici. Parsing, lettura della sintassi e conversione
in immagine 2×2 sono riusciti per Explicit VR Little Endian, RLE Lossless e
JPEG Baseline. Il driver ha anche aperto il DICOMDIR baseline come generico
oggetto DICOM; non ne ha validato la gerarchia dei record.

Il set nativo documentato da DICOM-rs include JPEG e RLE in Rust puro.
JPEG-LS richiede la feature CharLS con dipendenza C++; JPEG 2000 richiede
OpenJPEG (binding C statico) oppure la port Rust `openjp2`, soggetta a
verifica di portabilità; l'integrazione GDCM richiede CMake e compilatore C++.
Nel codice sorgente 0.10.0 consultato non è stata trovata un'API FileSet
equivalente a quella pydicom: la scrittura conforme di DICOMDIR resta da
prototipare. Non è stata provata la compatibilità con viewer professionali.

Il driver release temporaneo misura 4.673.928 byte, 100 crate unici nel grafo
normale e linkage osservato a libc, libgcc_s, libm e loader Linux. È una misura
del driver, non del prodotto; le licenze transitive non sono state auditate.
I crate DICOM-rs dichiarano MIT OR Apache-2.0. Il lockfile e il codice scratch
restano in `.Temp`; il workspace principale è rimasto senza dipendenze DICOM.

Il primo `cargo run --release` scratch non ha risolto `index.crates.io` nel
contesto sandbox e si è interrotto (exit 130). L'esecuzione successiva con
escalation di rete sandbox ha scaricato le dipendenze pubbliche nel registry
Cargo locale e compilato il probe con exit 0. Un primo giro del driver ha
tentato di aprire come DICOM anche i PNG da esso stesso generati; il probe è
stato ristretto ai soli `.dcm` e completato con successo.

Fonti ufficiali consultate: [guide pydicom ai file-set](https://github.com/pydicom/pydicom/blob/main/doc/tutorials/filesets.rst),
[decodifica Pixel Data pydicom](https://github.com/pydicom/pydicom/blob/main/doc/tutorials/pixel_data/introduction.rst),
[API DICOM-rs pixeldata 0.10.0](https://docs.rs/dicom-pixeldata/0.10.0/dicom_pixeldata/),
[registro sintassi DICOM-rs 0.10.0](https://docs.rs/dicom-transfer-syntax-registry/0.10.0/dicom_transfer_syntax_registry/),
[manifest e feature pixeldata](https://github.com/Enet4/dicom-rs/blob/master/pixeldata/Cargo.toml).
Context7 è stato consultato per pydicom e DICOM-rs; le versioni scaricate sono
state verificate nei manifest Cargo locali.

## Limiti e stato

- Prove limitate a Linux x86_64 e fixture sintetici; nessuna verifica Windows,
  macOS, dispositivo, dataset multi-frame, immagini RGB o viewer reale.
- JPEG Lossless, JPEG-LS e JPEG 2000 non hanno una fixture end-to-end in questa
  scheda. Non dichiarare supporto Rust per tali sintassi in base alla sola
  presenza delle feature.
- DICOMDIR generato da pydicom e letto come oggetto da DICOM-rs; creazione e
  validazione della gerarchia DICOMDIR in Rust restano aperte per P04.06.
- Le anteprime restano non diagnostiche e non equivalgono a un viewer clinico.

Non serve G2: la scheda non propone una modifica del contratto o una deroga.
La capability `dicom` resta `pending` fino al porting e alla parità funzionale.

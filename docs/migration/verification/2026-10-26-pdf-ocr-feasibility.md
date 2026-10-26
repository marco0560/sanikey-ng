# Ricevuta P04.01 — Fattibilità PDF e OCR

Data: 2026-10-26. Prove locali su Gentoo Linux x86_64, con fixture sintetiche
non sensibili. Nessuna modifica al checkout Python fissato o al workspace Rust.
Gli artefatti temporanei sono in `.Temp/sanikey-p04-01/` e non sono necessari
per ripetere i controlli del repository.

## Contratto osservato nella baseline

La baseline è il commit `fdad1d93e56496cc6cb9933b3089729175d5b5b3`. Il percorso
digitale estrae il testo per pagina con PyMuPDF e conserva `page_spans`. Il
testo è sufficiente quando contiene almeno 40 caratteri non whitespace. Per
PDF scansionati con almeno il 90% di copertura immagine per pagina, OCRmyPDF è
il fallback; il sidecar viene letto come testo. La riga di comando usa
`--skip-text --output-type pdf --continue-on-soft-render-error --sidecar` e
può passare il dizionario medico locale. Immagini sorgente `.jpg`, `.jpeg` e
`.png` non sono inviate direttamente all'OCR, secondo l'ADR accettata.

Il test OCR della baseline non specifica la lingua; la prova locale ha quindi
usato il default Tesseract inglese. L'installazione locale dispone anche di
`ita`, ma non dimostra che la baseline lo selezioni.

## Prove e risultati

Sono stati eseguiti i due test d'integrazione della baseline fissata:

```bash
BASELINE_ROOT=<isolated checkout at the pinned baseline commit>
PYTHON_REPO_ROOT=<SaniKey Python checkout with its existing development venv>
TEMP_ROOT=<approved .Temp directory>
PYTHONDONTWRITEBYTECODE=1 \
PYTHONPATH="$BASELINE_ROOT/src" \
TMPDIR="$TEMP_ROOT" \
"$PYTHON_REPO_ROOT/.venv/bin/pytest" \
-p no:cacheprovider -q \
tests/test_documents.py::test_extract_text_reads_synthetic_pdf_with_pymupdf \
tests/test_documents.py::test_extract_text_reads_image_only_pdf_with_ocrmypdf
```

Esito: `2 passed` in 2.40 s. Il primo verifica testo sintetico, assenza di
warning, una pagina e inizio dello span a carattere zero. Il secondo verifica
un PDF composto da immagine, fallback OCR, testo `SYNTHETIC OCR TEST` e assenza
di warning.

Il gate del repository è stato eseguito dalla radice Rust:

```bash
cargo xtask check
```

Exit 0: 9 unit test, 1 doctest, fmt, Clippy, build, smoke test CLI,
documentazione e controlli di pubblicabilità superati.

Per una prova Rust isolata, un driver temporaneo usa `lopdf = 0.40.0` per il
PDF digitale e `std::process::Command` per lanciare OCRmyPDF sul PDF sintetico
scansionato. Nessuna dipendenza è stata aggiunta al repository. Entrambe le
prove hanno prodotto il testo atteso:

- `lopdf::Document::load`, `get_pages` ed `extract_text` hanno restituito
  `Synthetic non-sensitive PDF text for PyMuPDF integration testing.`; la
  stringa coincide esattamente con quella inserita dal test Python.
- La chiamata OCRmyPDF con gli stessi flag funzionali della baseline ha
  restituito `SYNTHETIC OCR TEST` nel sidecar, con exit 0.

Comandi Cargo del prototipo, dalla radice del repository. `TEMP_ROOT` punta
all'area `.Temp` autorizzata per gli artefatti temporanei:

```bash
CARGO_HOME="$TEMP_ROOT/sanikey-p04-01/cargo-home" \
CARGO_TARGET_DIR="$TEMP_ROOT/sanikey-p04-01/target" \
cargo run --offline --manifest-path "$TEMP_ROOT/sanikey-p04-01/lopdf/Cargo.toml" -- \
digital "$TEMP_ROOT/sanikey-p04-01/digital.pdf"

CARGO_HOME="$TEMP_ROOT/sanikey-p04-01/cargo-home" \
CARGO_TARGET_DIR="$TEMP_ROOT/sanikey-p04-01/target" \
cargo run --offline --manifest-path "$TEMP_ROOT/sanikey-p04-01/lopdf/Cargo.toml" -- \
ocr "$TEMP_ROOT/sanikey-p04-01/image-only.pdf" \
"$TEMP_ROOT/sanikey-p04-01/sidecar.txt" \
"$TEMP_ROOT/sanikey-p04-01/ocr-output.pdf"
```

La prova PDF digitale è una sola pagina semplice; la prova OCR è una pagina,
testo inglese maiuscolo e raster a 250 DPI. Non dimostra ordine di lettura su
layout complessi, CMap/font non standard, tabelle, ligature, PDF cifrati o
malformati, più pagine, qualità OCR clinica, annullamento, prestazioni o
parità degli span Rust. Non sono state eseguite prove Windows o macOS.

## Dipendenze, licenze e dimensioni osservate

| Percorso | Dipendenze e licenze | Misure locali e vincoli |
| --- | --- | --- |
| PyMuPDF 1.28.0, baseline digitale | PyMuPDF/MuPDF è offerto sotto AGPL-3.0 o licenza commerciale Artifex. | Wheel manylinux x86_64 fissata dal lock: 25.716.599 byte; file installati nella venv Python: 61.507.855 byte. Include codice nativo. La licenza richiede valutazione prima di riusare questo percorso nel prodotto MIT. |
| `lopdf` 0.40.0, candidato digitale Rust | MIT dichiarata dal manifest del crate. Nessuna libreria nativa esterna osservata nel prototipo; restano da verificare e distribuire le licenze delle dipendenze transitive. | Crate scaricato: 7.490.989 byte; 81 crate normali nel grafo Cargo con feature predefinite. Eseguibile release di prova Linux x86_64: 2.068.992 byte, con linkage a libc e libgcc_s. È una misura del driver di prova, non del generatore. |
| OCRmyPDF 17.13.0 + Tesseract 5.5.3 | OCRmyPDF core MPL-2.0; Tesseract Apache-2.0; Leptonica BSD-2-Clause. OCRmyPDF richiede Python e Tesseract; la rasterizzazione usa pypdfium2 oppure Ghostscript. Richiede anche fpdf2 e uharfbuzz per il livello testuale; Noto è raccomandato. Altri strumenti sono opzionali. | `/usr/bin/ocrmypdf` è uno script di 2.899 byte, non la misura delle sue dipendenze. `/usr/bin/tesseract` è 55.576 byte e ha 33 righe di dipendenze dinamiche in `ldd`, incluse Tesseract, Leptonica, libarchive, curl, codec immagine, OpenSSL e runtime C/C++. I modelli `eng`, `ita`, `osd` presenti occupano rispettivamente 4.113.088, 2.701.314 e 10.562.727 byte (totale 17.377.129 byte); sono dati separati dal binario. |

Sorgenti ufficiali: [licenza PyMuPDF](https://pymupdf.readthedocs.io/en/latest/about.html),
[installazione PyMuPDF](https://pymupdf.readthedocs.io/en/latest/installation.html),
[manifest lopdf 0.40.0](https://github.com/J-F-Liu/lopdf/blob/v0.40.0/Cargo.toml),
[API lopdf 0.40.0](https://docs.rs/lopdf/0.40.0/lopdf/struct.Document.html#method.extract_text),
[installazione OCRmyPDF](https://ocrmypdf.readthedocs.io/en/latest/installation.html),
[dipendenze OCRmyPDF](https://ocrmypdf.readthedocs.io/en/latest/maintainers.html),
[licenza OCRmyPDF](https://github.com/ocrmypdf/OCRmyPDF),
[Tesseract e modelli](https://tesseract-ocr.github.io/tessdoc/Installation),
[licenza Leptonica](https://github.com/DanBloomberg/leptonica/blob/master/src/environ.h).

## Valutazione e alternative

`lopdf` è un candidato promettente per PDF digitale, non ancora selezionato per
il porting: la corrispondenza osservata copre il testo semplice di una fixture
e non gli span per pagina né gli altri casi del contratto. PyMuPDF resta un
riferimento di confronto; il suo AGPL/commerciale e il peso nativo rendono
necessaria una scelta di licenza e packaging prima di un suo eventuale riuso.

Per OCR, l'integrazione Rust→OCRmyPDF ha funzionato, ma OCRmyPDF usa Python e
componenti esterni: non è adatto a essere assunto come dipendenza incorporata
del generatore che deve funzionare senza Python. L'opzione coerente con tale
vincolo è mantenere un confine provider e valutare rendering PDFium più
Tesseract/Leptonica nativi, distribuendo in modo esplicito librerie e modelli;
questa alternativa non è stata prototipata né qualificata su altri OS. In
entrambi i casi vanno inventariate le licenze transitive e definite le
modalità di distribuzione. OCR rimane specifico dei PDF scansionati; l'ADR
esclude l'OCR diretto delle immagini sorgente.

Non serve aprire G2: non è stata proposta una deroga al contratto e sono emersi
candidati da valutare nelle schede di porting. La fattibilità qui non equivale
a parità funzionale o qualifica del pacchetto.

## Tentativi e limiti ambientali

- `uv run` dalla baseline read-only ha tentato di creare lì una `.venv` ed è
  fallito per permessi; nessun file del campione è stato modificato. I test
  sono stati eseguiti con la venv esistente, `PYTHONPATH` fissato e cache/
  bytecode temporanei disabilitati.
- Cargo senza rete non ha risolto `index.crates.io`. Il retry autorizzato ha
  scaricato il crate e i relativi sorgenti solo nella cache temporanea.
- L'opzione `nom_parser` richiesta dalla documentazione corrente non esiste
  nel crate 0.40.0; la prova è stata corretta usando le feature predefinite
  della versione bloccata.
- La prima compilazione del driver OCR ha segnalato un valore spostato; il
  prestito corretto è stato applicato solo al prototipo `.Temp`, che poi ha
  compilato e completato la prova.

Esito della scheda: completata come studio di fattibilità Linux locale. La
capability `pdf` resta `pending` in `parity.toml` fino a porting e verifiche
complete.

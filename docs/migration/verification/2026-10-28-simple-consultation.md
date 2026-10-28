# P07.03 — Immagini e consultazione semplice

Data: 2026-10-28. Verifica locale Linux con fixture sintetiche.

## Comportamento

`build_consultation_pipeline` produce un catalogo combinato per i sorgenti che
il browser può aprire direttamente: PDF, TXT, Markdown, HTML, JPEG, JPG e PNG.
Il catalogo conserva metadati, tag, hash e percorsi relativi; copia i byte
originali in `documents/` e genera link relativi verificabili nel JSON e nella
pagina HTML. I testi TXT/Markdown e PDF restano ricercabili; HTML e immagini
sono linkati come originali senza testo derivato. Formati Office convertiti e
membri di container restano nelle schede P07.04/P07.05.

Le immagini sono classificate come `image`, conservate byte per byte e
catalogate con testo, warning e provenienza PDF vuoti. Il percorso immagine non
chiama PDFium o Tesseract; non serve installare OCR per consultarne i file.
Questa scheda non verifica il rendering di codec immagine nel browser.

## Verifica

Fixture: due immagini sintetiche, Markdown, HTML, TXT, PDF digitale e ZIP non
apribile. Il test `txt_pipeline::tests::consultation_pipeline_copies_openable_sources_without_ocr_on_images`
verifica cardinalità del catalogo, assenza dello ZIP, classificazione e
warning delle immagini, href relativi, presenza dei target copiati e identità
byte-per-byte degli originali. PDFium e Tesseract sono stati deliberatamente
configurati su percorsi inesistenti: il PDF della fixture contiene testo
digitale sufficiente, così il test esercita anche il catalogo combinato senza
richiedere alcun provider OCR.

```bash
SANIKEY_TESSERACT=/missing/sanikey-test-tesseract \
SANIKEY_PDFIUM_LIBRARY=/missing/sanikey-test-pdfium \
CARGO_HOME=<cache Cargo temporanea> \
CARGO_TARGET_DIR=<target temporaneo> \
CARGO_NET_OFFLINE=true \
cargo test --offline -p sanikey-core consultation_pipeline_copies_openable_sources_without_ocr_on_images -- --nocapture
```

Esito: test mirato superato su Linux con PDFium e Tesseract indisponibili.

## Gate e commit

Il primo `cargo xtask check` si è fermato con exit 1 su `cargo fmt --all
-- --check`, per la formattazione del messaggio d'errore. Dopo `cargo fmt
--all`, il gate completo è terminato con exit 0: Clippy, 74 test core, 8
test xtask, 3 test CLI, 1 doctest, build workspace, CLI info/versione,
controllo di pubblicabilità (208 file) e rustdoc superati.

Il hook pre-commit e il commit-msg sono terminati con exit 0: 74 test core,
8 test xtask, 1 test del protocollo GUI; formattazione e test dei workspace
egui e Slint superati (0 test unitari in ciascun prototipo). Commit
implementativo: `a82e47c` (`feat(core): porta immagini e consultazione
semplice`).

Qualifica browser manuale e prove Windows/macOS non eseguite. Le capability
frontend ed export e la parità complessiva restano pending.

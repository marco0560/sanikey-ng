# P07.01 — PDF digitale

Data: 2026-10-28. Sistema verificato: Linux.

## Perimetro e comportamento

`sanikey-core::pdf::extract_pdf_text` legge solo testo digitale con `lopdf`
0.40.0. Restituisce pagine, testo concatenato con separatore newline, warning
ed offset Unicode per pagina. Mantiene il testo se è insufficiente e avvisa
che OCR non è disponibile; non invoca OCR, non normalizza il contenuto e non
modifica i byte originali. `pdf_page_count` restituisce il conteggio quando il
documento è leggibile.

`build_pdf_pipeline` scrive il testo nel JSON, nell'indice SQLite FTS5 e nella
pagina HTML offline. La tabella `document_page_spans` associa documento,
pagina e intervallo testuale per conservare la provenienza utile alle fasi che
derivano parametri. Copia il PDF originale invariato e usa un link relativo.
PDF malformati e PDF senza testo digitale vengono conservati con warning; OCR
resta P07.02. La capability PDF resta `pending` finché OCR e qualifica
multipiattaforma non sono completati.

I test generano PDF sintetici a runtime con lopdf; non sono stati aggiunti
binari PDF al corpus versionato. Coprono due pagine, testo accentato, offset,
conteggio, warning, FTS, JSON, HTML, copia byte-per-byte e PDF malformato.

## Dipendenza

Dipendenza esatta: `lopdf = 0.40.0`, con feature predefinite disattivate per
non includere i provider di data/ora e parallelismo non usati dall'estrazione.
Le API `Document::load`, `get_pages`, `extract_text` e `save_modern` sono state
verificate sulla documentazione ufficiale e sul sorgente della versione
fissata, tramite Context7. L'estrazione usa implementazione Rust e dipendenze
Rust incluse nel lockfile; non richiede Python, processi PDF esterni o il
toolkit OCR. La verifica di confezionamento e licenze sulle altre piattaforme
resta aperta.

## Verifiche

- `CARGO_HOME=/home/marco/Personalia/Progetti/.Temp/sanikey-p04-01/cargo-home cargo test -p sanikey-core pdf --offline`: 4 test superati.
- `CARGO_NET_OFFLINE=true CARGO_HOME=/home/marco/Personalia/Progetti/.Temp/sanikey-p04-01/cargo-home cargo xtask check`: exit 0; fmt, Clippy, 3 test CLI, 70 test core, 8 test xtask, doctest, build, smoke CLI, 3 test del controllo pubblicabilità, 204 file pubblicabili e rustdoc.
- Test workspace isolati: protocollo 1/1, egui 0 test, Slint 0 test; build dei tre workspace superata. Lockfile isolati aggiornati per `sanikey-core` e dipendenze PDF.
- `cargo fmt --all` e gate documentale inclusi nel gate completo.
- Nessuna verifica Windows/macOS o browser manuale; nessuna esecuzione OCR.

## Limiti dei tentativi

Il primo test offline si è fermato perché mancavano nel cache i metadati e
alcuni crate di lopdf. Il download diretto non funzionava per DNS nella
sandbox; il download è stato poi autorizzato nella cache temporanea `.Temp`.
Le prime compilazioni hanno rilevato: tipo ambiguo nel test dell'elenco vuoto,
conversioni `usize` non supportate da `rusqlite::ToSql`, complessità Clippy
nell'alias del reader, e aspettative non corrette del fixture WinAnsi/newline.
Le correzioni sono state verificate dai test mirati e dal gate completo.

Codira non ha fornito orientamento indicizzato: `uv run codira ctx` ha
segnalato l'indice locale corrotto o illeggibile. La CLI `caps` ha confermato
la disponibilità dell'analizzatore Rust; non è stato ripetuto il rebuild che
aveva già fallito con SIGBUS/SIGSEGV durante P06.02. I sorgenti baseline sono
stati letti direttamente dal commit fissato
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

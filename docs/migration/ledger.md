# Registro di avanzamento

## Stato della preparazione

Data: 2026-10-26. Il piano è approvato con la modifica preliminare P01.
La sessione di bootstrap si è fermata dopo repository e documentazione;
l'implementazione approvata è ripresa in seguito con P03.01.

Baseline Python: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.
Stato Rust: P07.05 completata; staging ZIP/7z/RAR/TAR.XZ e percorso
IMG/ISO implementati e verificati su fixture sintetiche Linux. La capability
containers è implemented ma non verified: immagini disco reali, alcuni codec
e Windows/macOS restano da qualificare. La prossima scheda è P07.06.
Stato GUI: G1 approvato il 2026-10-27 per egui/eframe 0.37.0; crate desktop
di prodotto non ancora introdotto.

| Scheda | Stato | Evidenza |
| --- | --- | --- |
| P01.01 | completata | Commit preliminare con tutti i dodici file; working tree pulito |
| P00.01 | completata | Destinazione locale disponibile; GitHub 404 e account marco0560 verificati |
| P00.02 | completata | 53 schede; TOML, link locali, grafo e invarianti verificati |
| P01.02 | completata | Manifest e ricevuta confrontati con lo SHA reale |
| P00.03 | completata | Repository pubblico creato; primo commit pubblicato e SHA remoto verificato |
| P01.03 | completata | 2026-10-26; checkout isolato sul commit fissato, sync locked e gate exit 0; fetch remoto diretto dello SHA riuscito; commit `cb5d088`; [ricevuta](verification/2026-10-26-python-clean-checkout.md) |
| P02.01 | completata | 2026-10-26; inventario di 20 capability, CLI, formati, persistenza, default, errori e ADR; TOML/link/diff check superati; nessuna ambiguità materiale; commit `d70e743`; [ricevuta](verification/2026-10-26-contract-inventory.md) |
| P02.02 | completata | 2026-10-26; 17 fixture sintetici con hash, 11 generatori/adattatori referenziati con hash; baseline positiva e due negativi eseguiti; expected normalizzati; commit `b1bf5c0`; [ricevuta](verification/2026-10-26-synthetic-corpus.md) |
| P02.03 | completata | 2026-10-28; comparatori per originali, SQLite, JSON, testo e frontend; 8 test xtask, mutazione JSON exit 1, gate Rust/documentale, Codira e smoke comparatori exit 0 su Linux; commit `b59cae1`, refactoring Codira `a7012e2`, docstring audit `7e1d174`; [ricevuta](verification/2026-10-28-comparators.md) |
| P03.01 | completata | 2026-10-26; workspace di tre crate, toolchain 1.98.1, CLI info/versione, xtask e doctest; build, Clippy, rustdoc, unit test e doctest superati; commit `597bdf7`; [ricevuta](verification/2026-10-26-rust-foundation.md) |
| P03.02 | completata | 2026-10-26; errori codificati, eventi ordinati, progresso e annullamento cooperativo; info passa dal runner; 9 unit test, doctest, Clippy e rustdoc superati; commit `1ed6545`; [ricevuta](verification/2026-10-26-operation-contract.md) |
| P03.03 | completata | 2026-10-26; `cargo xtask check` exit 0; run 40404493580 completato su Ubuntu/Windows/macOS; commit `6a671e1`; [ricevuta](verification/2026-10-26-rust-gate-ci.md) |
| P03.04 | completata | 2026-10-26; installazione CLI release fuori checkout, `PATH` vuoto senza Git/Python; info/versione exit 0, comando sconosciuto exit 2; ELF 474.392 byte con linkage dinamico Linux registrato; commit `4b25bc0`; [ricevuta](verification/2026-10-26-cli-minimum-linux.md) |
| P04.01 | completata | 2026-10-26; PyMuPDF baseline e `lopdf 0.40.0` concordano sulla fixture PDF digitale; OCRmyPDF su fixture solo immagine restituisce il testo atteso; gate e limiti Linux registrati; commit `e8b1db8`; [ricevuta](verification/2026-10-26-pdf-ocr-feasibility.md) |
| P04.02 | completata | 2026-10-26; 12 test baseline e XLSX separato superati; conversioni Office e prototipi Calamine/ZIP/7z su fixture sintetiche; gate Rust e pubblicabilità exit 0; commit `d49d05b`; [ricevuta](verification/2026-10-26-office-container-feasibility.md) |
| P04.03 | completata | 2026-10-26; suite baseline DICOM 20/20; baseline e DICOM-rs decodificano tre sintassi sintetiche; media DICOMDIR e preservazione byte verificate; gate completo superato; scrittura DICOMDIR Rust non ancora prototipata; commit `ccc4a20`; [ricevuta](verification/2026-10-26-dicom-codec-feasibility.md) |
| P04.04 | completata con limiti | 2026-10-26; suite USB baseline 20 passed, 1 skipped; lettura Linux di mount, filesystem e spazio su directory `.Temp`; Win/macOS documentati ma non provati; semantica `usb_uuid` da chiarire prima del porting multiOS; commit `32a5e45`; [ricevuta](verification/2026-10-26-volume-identity-feasibility.md) |
| P04.05 | completata | 2026-10-26; candidati fissati a egui/eframe 0.37.0 e Slint 1.18.1; protocollo identico per P04.06; desktop resta pending e G1 non deciso; gate `cargo xtask check` superato; commit `02b2fc1`; [ricevuta](verification/2026-10-26-gui-toolkit-candidates.md) |
| P04.06 | completata; G1 approvato | 2026-10-26; prototipi egui e Slint con eventi sintetici condivisi; check, test, Clippy e build release superati su Linux; binari 29.263.024/30.923.256 byte e grafo normale 366/443 nodi; gate `cargo xtask check` superato; accessibilità manuale, DPI, dialoghi e Windows/macOS non provati; commit `96ddfab`; approvazione egui/eframe 0.37.0 il 2026-10-27; [ricevuta](verification/2026-10-26-gui-toolkit-prototypes.md), [ADR](../decisions/0002-confronto-toolkit-gui.md) |
| P05.01 | completata | 2026-10-28; parser TOML e validazione account con 22 corrispondenze dirette ai test baseline, ulteriori casi per chiavi chiuse e conflitti UUID; gate `cargo xtask check`, hook di commit e Codira index/cov/audit superati su Linux; Codira ha richiesto moduli più piccoli dopo SIGSEGV sui file grandi; commit `d783429`; la capability `configuration` resta pending fino a P05.03; [ricevuta](verification/2026-10-28-configuration.md) |
| P05.02 | completata | 2026-10-28; modelli e loader metadata con validazione atomica; preflight privacy filesystem-only e guardia Git dev-only; originali invariati su fixture sintetica; `cargo xtask check`, hook di commit e Codira index/cov/audit superati su Linux; capability `metadata` e `privacy` implementate ma non ancora verificate; commit `da07e04`; [ricevuta](verification/2026-10-28-metadata-privacy.md) |
| P05.03 | completata | 2026-10-28; importazione account legacy da percorso assoluto; copia TOML byte-per-byte in nuova destinazione sotto la stessa base, con no-overwrite, riapertura e verifica semantica; test con cwd cambiata, spazi, Unicode e riferimenti relativi; gate `cargo xtask check` e hook superati su Linux; Codira coverage completa, audit exit 0 con quattro rilievi rustdoc preesistenti fuori ambito; Windows/macOS non provati; capability `configuration` implementata, non ancora verificata multipiattaforma; commit `fbe1f33` e documentazione test `f3f5684`; [ricevuta](verification/2026-10-28-account-profiles.md) |
| P06.01 | completata | 2026-10-28; inventario canonico, categorie, filtri include/exclude, SHA-256 e duplicati; corpus sintetico coincide con baseline e originali invariati; `cargo xtask check`, Codira index/cov/audit e hook pre-commit superati su Linux; Codira audit exit 0 con quattro rilievi rustdoc preesistenti; primo hook rileva lockfile obsoleti nei workspace GUI isolati, aggiornati offline e poi validati; Windows/macOS non provati; capability `inventory` implementata, non ancora verificata multipiattaforma; commit `913284d`; [ricevuta](verification/2026-10-28-inventory.md) |
| P06.02 | completata | 2026-10-28; pipeline `.txt` con lettura compatibile con baseline, metadati documento, SQLite/FTS5, JSON, pagina offline e originali linkati; corpus expected, FTS, link e byte originali verificati; `cargo xtask check`, pubblicabilità, hook e test dei tre workspace GUI superati su Linux; Codira index non disponibile per SIGBUS/BrokenProcessPool/SIGSEGV anche con output pulito; capability complete `sqlite`, `exports` e `frontend` restano pending; commit `471d54e`; [ricevuta](verification/2026-10-28-txt-pipeline.md) |
| P06.03 | completata | 2026-10-28; export TXT su directory simulata con manifest/checksum, link verificati, due manifest byte-identici e mutazione rilevata; originali immutati; `cargo xtask check`, pubblicabilità, hook, 66 test core e prototipi GUI superati su Linux; nessun target fisico; capability USB completa pending; commit `f238bf7`; [ricevuta](verification/2026-10-28-simulated-export.md) |
| P07.01 | completata; parità PDF pending | 2026-10-28; estrazione PDF digitale con `lopdf 0.40.0`, warning e intervalli Unicode per pagina; testo verificato in JSON, FTS5 e HTML, copie originali identiche; PDF malformato e senza testo verificati senza OCR; `cargo xtask check`, hook e tre workspace GUI superati su Linux; OCR e qualifica multipiattaforma restano pending; commit `c2b2f2f`; [ricevuta](verification/2026-10-28-pdf-digital.md) |
| P07.02 | completata; parità PDF pending | 2026-10-28; OCR PDF via PDFium e Tesseract, soglia baseline, forzatura su scansioni e glossario; OCR sintetico, fallback digitale, errori e provenienza verificati; `cargo xtask check` exit 0; pre-commit superato con 73 test core e i tre workspace GUI; Windows/macOS e distribuzione nativa pending; commit `a4e15a5`; [ricevuta](verification/2026-10-28-pdf-ocr.md) |
| P07.03 | completata; parità frontend/export pending | 2026-10-28; consultazione PDF, TXT, Markdown, HTML, JPEG e PNG; copie byte-identiche e href verificati; immagini con testo/warning vuoti e test passato con PDFium/Tesseract non disponibili; `cargo xtask check` exit 0 dopo correzione fmt; hook pre-commit superato (74 core, 8 xtask, 1 protocollo GUI; test egui/Slint e formattazione superati); browser e Windows/macOS non qualificati; commit `a82e47c`; [ricevuta](verification/2026-10-28-simple-consultation.md) |
| P07.04 | completata; parità Office pending | 2026-10-28; Pandoc per DOCX/ODT/RTF, LibreOffice per DOC e fogli; originali conservati/linkati, PDF derivati verificati; conversione reale RTF e DOCX, warning DOCX corrotto, assenza convertitore e timeout verificati; `cargo xtask check` exit 0 (82 core, 1 ignored, 8 xtask, 3 CLI, doctest); hook superati (82 core, 1 ignored, 8 xtask, 3 CLI, GUI); XLSB/XLSM, browser e Windows/macOS pending; commit `e2e9586`; [ricevuta](verification/2026-10-28-office-conversion.md) |
| P07.05 | completata; parità containers pending | 2026-10-28; staging confinato, manifest v1, filtri, ricorsione e deduplica; ZIP Stored/DEFLATE/BZIP2/LZMA, 7z, RAR e TAR.XZ provati; fixture `.iso` con payload 7z per il flusso disco, ISO/IMG reali pending; baseline `tests/test_containers.py` 20/20; `cargo xtask check` exit 0 (96 core, 1 ignored, 8 xtask, 3 CLI, doctest); pre-commit e GUI superati dopo aggiornamento offline dei lockfile isolati; Codira index SIGSEGV; commit `13ac794`, lockfile GUI `a8d287c`; [ricevuta](verification/2026-10-28-container-staging.md) |
| P07.06 | completata; parità DICOM pending | 2026-10-28; baseline `tests/test_dicom.py` 20/20; catalogo Rust raggruppa UID, legge DICOMDIR, coalesca record e conserva provenienza staged/danneggiata; 5 test DICOM, `cargo xtask check` exit 0 (101 core, 1 ignored, 8 xtask, 3 CLI, doctest, pubblicabilità, rustdoc); pre-commit e tre GUI superati; Codira index SIGSEGV; commit `418baa3`; [ricevuta](verification/2026-10-28-dicom-catalog.md) |

Le schede non registrate sopra sono da eseguire; l'indice completo è in
[tasks/README.md](tasks/README.md). La parità per capability è mantenuta in
[parity.toml](parity.toml); lo stato `implemented` richiede ancora verifiche
sui sistemi non provati prima di dichiarare la parità completa.

## Risultati del campione

- Gate Python: exit 0, 359 passed, 1 skipped, copertura complessiva 92%.
- Documentazione MkDocs strict: exit 0.
- Hook pre-commit e commit-msg: superati.
- Nessuna prova USB fisica e nessuna qualifica Python Windows/macOS.
- Nessuna push del Python effettuata.
- Dettagli e limiti nella [ricevuta](verification/2026-10-26-python-baseline.md).

## Recuperabilità remota della baseline

P01.03 ha verificato il fetch diretto dello SHA dal repository Python remoto.
La baseline è recuperabile senza pubblicare una nuova ref. La CI remota potrà
usare il commit esistente; ogni push futuro del Python resta soggetto ad
autorizzazione specifica.

## Checkpoint

| Checkpoint | Stato |
| --- | --- |
| G1 — toolkit GUI | Approvato il 2026-10-27: egui/eframe 0.37.0; [ADR](../decisions/0002-confronto-toolkit-gui.md) |
| G2 — cambiamenti materiali | Da chiarire prima del porting USB multiOS: distinguere UUID filesystem da ID deploy (`usb_uuid`); nessuna modifica applicata; [P04.04](verification/2026-10-26-volume-identity-feasibility.md) |
| G3 — transizione operativa | Non raggiunto |
| G4 — phase-out Python | Non raggiunto |

## Prossima sessione

Leggere [START-HERE.md](START-HERE.md). G1 approvato per egui/eframe 0.37.0;
la prossima scheda è [P07.07](tasks/P07.07-portare-media-viewer-e-anteprime-dicom.md).
Le pipeline PDF e Office sono implementate e verificate su Linux, ma la parità
Office completa, la parità containers completa, il browser e il
confezionamento multipiattaforma restano da verificare. Il crate desktop di
prodotto resta pianificato per P11. P07.06 ha portato la catalogazione DICOM;
media, viewer e anteprime restano nel perimetro di P07.07 e la capability
DICOM rimane pending.

A ogni scheda aggiungere data, commit, comandi ed esiti effettivi, eventuali
skip con motivazione, evidenze, blocchi e successiva attività.
Non modificare retroattivamente questa cronologia per nascondere tentativi falliti.

## Consegna della preparazione

- Repository pubblico: https://github.com/marco0560/sanikey-ng.
- Primo commit: `9eeafcc4ff3d31271cae3bf72bc1d4d773d23bfd`.
- Pubblicazione iniziale su main verificata con lettura della ref GitHub.
- 73 file di bootstrap; 53 schede; 20 capacità con parità ancora pending.
- Controlli documentali e git diff --cached --check: superati.
- SaniKey Python: commit preliminare presente e working tree pulito.
- Al bootstrap Rust, corpus e comparatori non erano avviati; P03.01 ha creato
  il workspace minimo e P02.02 ha fissato il corpus. GUI e comparatori restano
  da realizzare.
- Il toolkit rimane da scegliere in G1.
- Ripresa iniziale prevista: P01.03; completata il 2026-10-26. La scheda
  successiva è P02.01, come registrato sopra.

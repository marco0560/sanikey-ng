# P11.04 — Verifica GUI Linux disponibile

Data: 2026-11-03. Qualifica parziale su Linux.

## Verifica eseguita

- La GUI vuota è stata avviata fuori dalla sandbox sul compositor Plasma dell'host: `timeout 6s cargo run --package sanikey-desktop --offline` è rimasto attivo fino al timeout (`exit=124`) senza errori di inizializzazione. Non è stato caricato un profilo né aperto un file.
- `cargo test -p sanikey-desktop --offline`: 9/9 test; coprono caricamento profilo, preflight, duplicati, percorsi, generazione, errori, annullamento e ripresa. Non sostituiscono l'interazione con la finestra.
- Ispezione statica: i campi di profilo e percorso associano l'etichetta visibile tramite `labelled_by`; pulsanti e checkbox usano controlli standard egui con nomi visibili.
- `CARGO_NET_OFFLINE=true cargo xtask check`: exit 0; 203 test core (2 ignorati), 3 CLI, 9 desktop, 1 Windows-volume, 2 macOS-volume, 8 xtask, 2 doctest, smoke CLI, pubblicabilità 295 file e rustdoc.
- `uv run codira index`: 6 indicizzati, 225 riusati, 0 fallimenti; rimangono 12 file per cui non è registrato un parser.

## Limiti

Non sono stati inviati eventi di tastiera o mouse e non è stata acquisita una schermata: la sandbox non può connettersi al socket Wayland host (`NoCompositor`) e non sono disponibili strumenti input/cattura. Restano da verificare navigazione e focus, scaling DPI, temi chiaro/scuro, viewport stretto, stati di errore interattivi e tecnologie assistive. Nessuna prova Windows/macOS o USB fisica è stata eseguita.

La scheda P11.04 resta aperta. Per completarla serve una sessione Linux interattiva con tastiera/display; l'avvio senza errori e i test core/desktop automatizzati non sono qualificati come end-to-end.

# P11.04 — Qualifica GUI Linux parziale

Data: 2026-11-03. Qualifica parziale su Linux.

## Verifica eseguita

- La GUI vuota è stata avviata sul compositor Plasma dell'host e la finestra è rimasta aperta senza errori. La schermata iniziale mostra titolo, istruzioni, controlli e messaggi italiani; nessun profilo è stato caricato.
- La finestra nativa è stata ridimensionata a 460×360 pixel. La prima ispezione ha mostrato il pulsante “Apri profilo…” tagliato a destra. Le righe con campi e azioni ora usano il layout a capo; l'intera vista è contenuta in uno scroll verticale.
- `desktop_ui_renders_with_keyboard_focus_themes_scaling_and_narrow_viewports` invia Tab a egui e verifica il focus, il tema di sistema chiaro/scuro, lo scaling 1×/2× e un viewport 400×280 punti. Nel viewport stretto asserisce anche che l'intera etichetta “Apri profilo…” resti dentro il rettangolo di clip.
- `cargo fmt --all --check` e `CARGO_NET_OFFLINE=true cargo test -p sanikey-desktop --offline`: superati, 10/10 test desktop.
- `CARGO_NET_OFFLINE=true cargo xtask check`: exit 0; 203 test core (2 ignorati), 3 CLI, 10 desktop, 1 Windows-volume, 2 macOS-volume, 8 xtask, 2 doctest, smoke CLI, pubblicabilità 295 file e rustdoc.
- `uv run codira index`: aggiornamento riuscito, nessun fallimento; 12 file restano fuori copertura perché non hanno parser configurato.

## Limiti ed esiti non qualificati

- La correzione di layout è stata provata con l'asserzione sul rettangolo di clip. Non è stata acquisita una seconda schermata nativa affidabile dopo la correzione: la finestra attiva è diventata un selettore file del portale desktop; la cattura è stata eliminata e la finestra non è stata manipolata.
- Tab e focus sono stati verificati tramite eventi sintetici egui, non con una tastiera fisica. DPI e temi sono varianti di rendering headless; la schermata host è stata osservata nel tema scuro corrente.
- Non sono stati provati flussi di errore interattivi né tecnologie assistive/screen reader. Non è stata eseguita alcuna prova USB fisica.
- Windows e macOS non sono qualificati. Il manutentore proseguirà con i test Windows dopo la verifica Linux.

P11.04 resta aperta per le verifiche interattive Linux residue e le qualifiche Windows/macOS. Le prove headless e di avvio non sono qualificate come end-to-end.

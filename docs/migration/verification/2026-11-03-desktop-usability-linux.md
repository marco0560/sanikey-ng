# P11.04 — Qualifica GUI Linux parziale

Data: 2026-11-03. Qualifica parziale su Linux.

## Verifica eseguita

- La GUI vuota è stata avviata sul compositor Plasma dell'host e la finestra è rimasta aperta senza errori. La schermata iniziale mostra titolo, istruzioni, controlli e messaggi italiani; nessun profilo è stato caricato.
- La finestra nativa è stata ridimensionata a 460×360 pixel. La prima ispezione ha mostrato il pulsante “Apri profilo…” tagliato a destra. Le righe con campi e azioni ora usano il layout a capo; l'intera vista è contenuta in uno scroll verticale.
- `desktop_ui_renders_with_keyboard_focus_themes_scaling_and_narrow_viewports` verifica che Tab e Shift+Tab raggiungano controlli distinti agli estremi dell'ordine in contesti puliti; controlla inoltre il tema di sistema chiaro/scuro, lo scaling 1×/2× e un viewport 400×280 punti. Nel viewport stretto asserisce che l'intera etichetta “Apri profilo…” resti dentro il rettangolo di clip.
- `accessibility_tree_exposes_form_names_and_invalid_profile_status` abilita AccessKit in egui e verifica il nome del campo “Profilo account”, il pulsante di apertura e la presenza del messaggio del profilo invalido nell'albero.
- `clicking_preflight_with_an_empty_path_shows_accessible_error` invia a egui eventi sintetici di spostamento, pressione e rilascio sul pulsante preflight; il percorso mancante produce il messaggio italiano atteso e lo espone nell'albero AccessKit.
- `clicking_preflight_with_a_valid_profile_exposes_the_duplicate_review` usa una fixture sintetica con due documenti duplicati, attiva il pulsante con eventi egui, attende il preflight asincrono e verifica che il riepilogo e il suo stato siano accessibili in AccessKit; test mirato superato (1/1).
- `warning_text_color_matches_light_and_dark_themes` verifica colori d'avviso chiari nel tema scuro e scuri nel tema chiaro; i tre messaggi colorati seguono ora la palette attiva.
- `cargo fmt --all --check` e `CARGO_NET_OFFLINE=true cargo test --offline -p sanikey-desktop clicking_preflight_with_a_valid_profile_exposes_the_duplicate_review`: superati; il test mirato passa 1/1.
- `CARGO_NET_OFFLINE=true cargo test --offline -p sanikey-desktop desktop_ui_renders_with_keyboard_focus_themes_scaling_and_narrow_viewports`: superato dopo l'estensione della verifica Tab/Shift+Tab (1/1).
- `CARGO_NET_OFFLINE=true cargo xtask check`, rieseguito dopo il test focus: exit 0; 201 test core superati (2 ignorati), 3 CLI, 14 desktop, 1 Windows-volume, 2 macOS-volume, 8 xtask, 2 doctest, smoke CLI, pubblicabilità 295 file e rustdoc.
- `uv run codira index` dopo il test focus: 3 file indicizzati, 229 riusati, 0 fallimenti; 12 file restano fuori copertura perché non hanno parser configurato.

## Limiti ed esiti non qualificati

- Le correzioni di layout e colore sono verificate dai test headless. I tentativi di cattura post-fix hanno acquisito finestre diverse da SaniKey (dialogo del portale desktop e Konsole); il manutentore ha precisato di aver minimizzato SaniKey perché interferiva col desktop, prima della cattura finale. I file sono stati eliminati e nessuna finestra altrui è stata manipolata. Non c'è una schermata nativa post-fix affidabile.
- Tab e focus sono stati verificati tramite eventi sintetici egui, non con una tastiera fisica. DPI e temi sono varianti di rendering headless; la schermata host è stata osservata nel tema scuro corrente.
- Il flusso di profilo invalido e il click di preflight con percorso mancante sono verificati con frame/eventi sintetici e il messaggio compare nell'albero AccessKit; non sono stati provati con uno screen reader reale né con input nativo. Non sono stati esplorati tutti i flussi d'errore. Nessuna prova USB fisica.
- Windows e macOS non sono qualificati. Il manutentore proseguirà con i test Windows dopo la verifica Linux.

P11.04 resta aperta per le verifiche interattive Linux residue e le qualifiche Windows/macOS. Le prove headless e di avvio non sono qualificate come end-to-end.

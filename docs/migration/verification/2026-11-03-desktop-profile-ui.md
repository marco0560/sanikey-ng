# P11.01 — GUI profili desktop

Data: 2026-11-03. Sistema verificato: Linux.

## Modifica

- Aggiunto `crates/sanikey-desktop` al workspace con eframe 0.37.0, toolkit approvato in G1.
- Importazione del profilo mediante `AccountsProfile::import`; il core resta responsabile del parsing e della validazione.
- La schermata consente di includere/escludere persone e scegliere cartelle documenti, metadati e build per la sessione corrente. Non salva né modifica il TOML importato.
- Ctrl+O apre il selettore del profilo; gli input hanno etichette accessibili e sono raggiungibili tramite navigazione standard della UI.
- Profilo non caricato e selezione vuota sono stati visibili. Gli errori di import sono presentati con codice e messaggio dal core.

## Verifiche

- `cargo check -p sanikey-desktop --offline`: exit 0.
- `cargo fmt --all -- --check`: superato dopo aver applicato `cargo fmt --all`.
- `cargo test -p sanikey-desktop --offline`: 2/2 test superati; profilo valido non modificato e persona disabilitata non preselezionata; TOML invalido rifiutato e stato GUI ripulito.
- `CARGO_NET_OFFLINE=true cargo xtask check`: exit 0; Clippy, test workspace (199 core con 2 ignored, 3 CLI, 2 desktop, 2 Windows-volume, 2 macOS-volume, 8 xtask), 2 doctest, smoke CLI, controlli di pubblicabilità (290 file) e rustdoc superati. Il primo tentativo del gate si era fermato sul lint Clippy `items_after_test_module`; il modulo test è stato spostato in fondo e il gate ripetuto con successo.
- `uv run codira index`: 2 indicizzati, 223 riusati, 0 fallimenti; 12 file tracked restano senza parser (CSS/JS/SVG/template/user-words). `uv run codira audit` non segnala `sanikey-desktop`; restano gli avvisi Rustdoc preesistenti in altri moduli.
- Il primo `cargo check -p sanikey-desktop --locked` ha tentato di aggiornare l'indice crates.io e non è riuscito perché il nome host non era risolvibile. La compilazione successiva offline ha risolto le dipendenze già presenti in cache e ha avuto esito positivo.
- `timeout --signal=TERM 8s cargo run -p sanikey-desktop --offline`: exit 1 prima di mostrare la finestra, `WaylandError(Connection(NoCompositor))`.
- `timeout --signal=TERM 8s env -u WAYLAND_DISPLAY cargo run -p sanikey-desktop --offline`: exit 1 prima di mostrare la finestra, `XOpenDisplayFailed`.
- Esecuzione e interazione manuale della finestra non verificate perché questo ambiente non espone un compositor Wayland né un display X11. Qualifica Windows/macOS, pacchetti e accessibilità assistiva restano pending.

## Prossimo passo

P11.02: collegare i controlli della GUI ai servizi di generazione core mantenendo l'interfaccia reattiva e gli errori espliciti.

La capability desktop nella matrice resta `pending`: questa scheda crea solo
la base profili; collegamento build/export e qualifica d'uso appartengono alle
schede P11.02–P11.04.

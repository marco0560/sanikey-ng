# P08.02c — Caricamento delle regole dei parametri

Data: 2026-10-28. Baseline Python fissata:
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.
Commit funzionale `d18e7b2`; correzione di precisione `03821b1`.

## Ambito

Il loader valida configurazioni TOML, limiti di discovery, campi ammessi e
obbligatori, unità, formati numerici, politiche, conversioni e riferimenti al
dizionario di ricerca. Le regole comuni e paziente vengono unite per ID con
sostituzione completa; la discovery del paziente prevale solo se dichiarata.
Il digest canonico coincide con la baseline per un TOML sintetico controllato:
`853a7bf9d62194da2160729940688641fcc07336bf81b22463a9c5e02afef49a`.

Fattori, offset e limiti sono conservati in `rust_decimal::Decimal`, come i
`Decimal` interni della baseline, e non in virgola mobile. `rust_decimal 1.43.0`
è fissata nel manifest; la crate espone parsing decimale esatto e operazioni
decimali ([API ufficiale](https://docs.rs/rust_decimal/1.43.0/rust_decimal/)).
La normalizzazione NFKC delle unità usa `unicode-normalization 0.1.25`
([API ufficiale](https://docs.rs/unicode-normalization/0.1.25/unicode_normalization/trait.UnicodeNormalization.html)).

Questo sottopasso non valuta ancora i candidati, non crea punti/serie e non
applica le conversioni; non porta report né segni vitali. P08.02 e la parità
della capability osservazioni restano in corso/pending.

## Verifiche

- Baseline: digest calcolato con `load_parameter_rules` sul fixture sintetico e
  dizionario usati dal test Rust — corrispondenza esatta.
- Rust mirato: `CARGO_NET_OFFLINE=true cargo test --offline -p sanikey-core parameter_rules -- --nocapture` — 3 passed.
- Clippy mirato: `CARGO_NET_OFFLINE=true cargo clippy --offline -p sanikey-core --all-targets -- -D warnings` — exit 0.
- Gate finale: `CARGO_NET_OFFLINE=true cargo xtask check` — exit 0; 130 test core,
  1 ignored, 8 xtask, 3 CLI, 2 doctest, smoke CLI, pubblicabilità (229 file) e rustdoc.
- Hook pre-commit sui commit del sottopasso — superato; 130 test core, 1
  ignored, 8 xtask, 3 CLI, 2 doctest e formattazione/test dei tre prototipi GUI.

Il primo gate aveva segnalato nella ricevuta P08.02a un percorso assoluto del
Cargo home locale. Sostituito con `<cargo-home>`, `python3
scripts/check_publishable.py` è passato e il gate completo è stato ripetuto con
esito positivo. In seguito il confronto con la baseline ha rilevato che i valori
configurati dovevano restare decimali esatti; aggiunta `rust_decimal`, aggiornati
i lockfile e ripetuti test mirati, Clippy, gate completo e hook.

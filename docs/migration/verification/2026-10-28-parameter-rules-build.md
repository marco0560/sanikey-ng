# P08.02e — Conversione e materializzazione delle osservazioni

Data: 2026-10-28. Commit funzionale `e89cb86`. Baseline Python fissata:
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

## Ambito

`build_parameter_slices` applica le politiche per unità mancanti, confronta le
unità senza distinzione di maiuscole/minuscole, usa solo conversioni affini
configurate e controlla i limiti sul valore convertito. Il parsing e
l'aritmetica di conversione mantengono `rust_decimal::Decimal` fino alla
materializzazione del valore `f64` richiesto dal modello osservazioni. I punti
conservano riferimenti al documento, riga, intervalli, valore/unità originali e
digest/versione della regola; le serie sono ordinate e riportano la regola che
le ha generate.

Valori oltre il dominio di `Decimal`/`f64` sono rifiutati con motivi espliciti
(`REJECTED_DECIMAL_PRECISION`, `REJECTED_CONVERSION_OVERFLOW`,
`REJECTED_FLOAT_RANGE`); non vengono convertiti in zero o infinito. Report,
merge, segni vitali e riconoscimento delle celle “valore in massa/percentuale”
restano fuori da questo sottopasso. La capability `parameters` resta pending.

## Verifiche

- Baseline: `uv run pytest tests/test_parameter_rules.py -q` — 7 passed.
- Rust mirato: `CARGO_NET_OFFLINE=true cargo test --offline -p sanikey-core parameter_rules -- --nocapture` — 8 passed.
- Clippy: `CARGO_NET_OFFLINE=true cargo clippy --offline -p sanikey-core --all-targets -- -D warnings` — exit 0.
- Formattazione: `cargo fmt --all -- --check` — exit 0.
- Gate: `CARGO_NET_OFFLINE=true cargo xtask check` — exit 0; 135 test core,
  1 ignored, 8 xtask, 3 CLI, 2 doctest, smoke CLI, pubblicabilità (232 file)
  e rustdoc superati.

Una prima compilazione mirata ha rilevato un helper `normalize_unit` duplicato
e il tipo opzionale mancante sul risultato di conversione. Corretti entrambi;
test mirati, Clippy e gate completi successivi sono passati.

## Prossimo sottopasso

P08.02f porterà report e aggregate delle serie osservazioni; segni vitali e
merge seguiranno in un sottopasso dedicato.

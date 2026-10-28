# P08.02b — Layout tabellari e valori annotati

Data: 2026-10-28. Baseline Python fissata: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.
Commit funzionale: `51c0900` (`feat(core): riconosce layout tabellari dei parametri`).

## Ambito

Estesa la scoperta di P08.02 con righe estratte verticalmente (etichetta, valore,
unità), layout a celle (etichetta, unità, intervallo di riferimento, valore) e
valori esplicitamente introdotti da `valore`, con righe di metodo e celle vuote
intermedie. La provenienza conserva intervallo di caratteri e testo originale;
le righe serum/urine continuano a ricevere il contesto di sezione rilevato da
P08.02a.

Restano fuori celle con descrizioni concorrenti come `valore in massa` e
`valore percentuale`, regole e conversioni dei parametri, report e segni vitali.
Non è dichiarata la parità completa di P08.02 né della capability osservazioni.

## Verifiche

- Baseline: sottoinsieme di `tests/test_parameter_slices.py` relativo a celle
  impilate, unità/intervallo/valore, forme annotate e confine tra etichette —
  7 passed.
- Rust: `CARGO_NET_OFFLINE=true cargo test --offline -p sanikey-core parameter_slices -- --nocapture` — 12 passed.
- `CARGO_NET_OFFLINE=true cargo clippy --offline -p sanikey-core --all-targets -- -D warnings` — exit 0.
- Hook `pre-commit` sul commit funzionale — exit 0: 127 test core, 1 ignored,
  8 test xtask, 3 CLI, 2 doctest e test/formattazione dei tre prototipi GUI.
- Il gate esplicito `cargo xtask check` non è stato rieseguito a questo
  sottopasso intermedio.

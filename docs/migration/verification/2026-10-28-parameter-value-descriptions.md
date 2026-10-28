# P08.02i — Celle con descrizione del tipo di valore

Data: 2026-10-28. Commit funzionale `933c9d6`. Baseline Python fissata:
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

## Ambito

La scoperta Rust ora legge le celle verticali `valore in massa` e
`valore percentuale` per le etichette di emoglobina glicata riconosciute dalla
baseline. Se sono presenti entrambe, seleziona deterministicamente il valore
in massa, conserva l'unità e la provenienza, e calcola gli offset Unicode fino
alla cella dell'unità.

## Verifiche

- Baseline: `uv run pytest tests/test_parameter_slices.py -q` — 14 passed.
- Rust mirato: `cargo test -p sanikey-core parameter_slices::tests -- --nocapture`
  — 13 passed.
- Formattazione: `cargo fmt --all -- --check` — exit 0.
- Clippy: `cargo clippy -p sanikey-core --all-targets -- -D warnings` — exit 0.
- Gate: `cargo xtask check` — exit 0; 143 test core passati, 1 ignorato,
  8 xtask, 3 CLI, 2 doctest, smoke CLI, pubblicabilità (240 file) e rustdoc.
- Hook pre-commit — superato; 143 test core, 1 ignorato, 8 xtask, 3 CLI,
  2 doctest e formattazione/test dei tre prototipi GUI.
- `git diff --cached --check` — exit 0.

Durante il primo compile il nuovo helper usava riferimenti non corretti agli
indici e il test ometteva l'argomento facoltativo delle etichette accettate;
la correzione ha portato i 13 test mirati al verde. Clippy ha poi segnalato un
indice non usato, rimosso prima del preflight finale.

## Limiti e prossimo passo

Le verifiche sono sintetiche e locali a Linux; non dichiarano parità completa
della capability su altri sistemi. P08.02 è conclusa; il prossimo passo è
P08.03, riferimenti e fogli AIFA.

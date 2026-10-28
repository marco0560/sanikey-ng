# P08.02h — Segni vitali e merge

Data: 2026-10-28. Commit funzionale `bd340e1`. Baseline Python fissata:
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

## Ambito

Il modulo Rust `vital_signs` porta le espressioni esplicite per peso e
pressione della baseline. Ignora documenti mancanti o senza data, conserva
provenienza del testo, link e pagina quando disponibile, restituisce
diagnostiche per valori fuori intervallo e produce solo le serie con punti
validi. Il merge evita serie duplicate e lascia prevalere un'osservazione
curata nella stessa serie e data.

## Verifiche

- Baseline: `uv run pytest tests/test_vital_signs.py -q` — 2 passed.
- Rust mirato: `cargo test -p sanikey-core vital_signs::tests -- --nocapture` —
  4 passed.
- Formattazione: `cargo fmt --all -- --check` — exit 0.
- Clippy: `cargo clippy -p sanikey-core --all-targets -- -D warnings` — exit 0.
- Gate: `cargo xtask check` — exit 0; 142 test core passati, 1 ignorato,
  8 xtask, 3 CLI, 2 doctest, smoke CLI, pubblicabilità (238 file) e rustdoc.
- Hook pre-commit — superato; 142 test core, 1 ignorato, 8 xtask, 3 CLI,
  2 doctest e formattazione/test dei tre prototipi GUI.
- `git diff --cached --check` — exit 0.

Durante lo sviluppo il controllo di formattazione ha segnalato modifiche
rustfmt non ancora applicate; Clippy ha richiesto di raggruppare i dati di
provenienza in un contesto per riga; il primo compile dopo tale refactoring ha
richiesto correzioni esplicite dei lifetime. I controlli finali elencati sopra
sono stati ripetuti dopo le correzioni e sono passati.

## Limiti e prossimo sottopasso

Le prove usano solo documenti sintetici Linux. L'implementazione non dimostra
parità completa della capability sui sistemi non provati.

P08.02i porta le celle annotate che descrivono il tipo di valore; poi si
chiude la scheda P08.02.

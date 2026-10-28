# P08.03b — Persistenza della selezione AIFA

Data: 2026-10-28. Baseline Python fissata:
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

## Ambito

Introdotti un helper che materializza la conferma esplicita di un candidato
AIFA, il fingerprint SHA-256 dei quattro campi curati usati dalla ricerca e la
scrittura ordinata di riferimenti ed esclusioni in
`medication_leaflets.toml`. Il writer usa la serializzazione TOML per
preservare correttamente virgolette e caratteri dei valori; il loader dei
metadati rilegge il risultato e verifica i riferimenti incrociati.

## Verifiche

- Baseline: `uv run pytest tests/test_leaflets.py -q` — 12 passed.
- Rust mirato: `cargo test -p sanikey-core aifa::tests -- --nocapture` — 8
  passed; include fingerprint confrontato con la baseline, ordinamento,
  escaping TOML e rilettura col loader dei metadati.
- Formattazione, Clippy e gate: `cargo xtask check` — exit 0; 151 test core
  passati, 1 ignorato, 8 xtask, 3 CLI, 2 doctest, smoke CLI, pubblicabilità
  (243 file) e rustdoc.

## Limiti e prossimo sottopasso

Il fingerprint Rust applica `to_lowercase`; la baseline Python usa `casefold`,
che differisce per alcune mappature Unicode espansive. La prova conferma la
compatibilità per i caratteri usati nel caso sintetico, non l'identità per ogni
stringa Unicode. Nessun dato personale è stato usato e non è stata interrogata
l'API AIFA reale. La capability resta `pending`. Prossimo: P08.03c, cache,
download e verifica dei documenti FI/RCP.

# P08.03d — Lookup offline dei fogli AIFA

Data: 2026-10-28. Baseline Python fissata:
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

## Ambito

Introdotto `local_leaflet_documents`, che individua i percorsi FI e RCP sotto
la cache locale del farmaco senza creare un client né effettuare richieste di
rete. Se uno o entrambi i file mancano, il risultato espone solo quelli
presenti. I nomi e i percorsi corrispondono ai riferimenti locali del payload
baseline di `exports.py`.

## Verifiche

- Baseline: `uv run pytest tests/test_exports.py -q` — 8 passed.
- Rust mirato: `cargo test -p sanikey-core aifa::tests -- --nocapture` — 12
  passed; la cache sintetica restituisce entrambi i percorsi e il farmaco
  senza cache restituisce un risultato vuoto.
- Gate: `cargo xtask check` — exit 0; 155 test core passati, 1 ignorato,
  8 xtask, 3 CLI, 2 doctest, smoke CLI, pubblicabilità (245 file) e rustdoc.

## Limiti e stato della capability

La costruzione del payload/export Rust non è ancora implementata; questo test
verifica il lookup locale che l'export consumerà. Il servizio AIFA reale non è
stato contattato. P08.03 è completata a livello core, mentre la capability
resta `pending` fino alla qualifica del servizio e all'integrazione in P09.02.
Prossima scheda del piano: P09.01, SQLite e FTS5.

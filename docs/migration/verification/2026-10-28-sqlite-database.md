# P09.01 — SQLite e FTS5

Data: 2026-10-28. Baseline Python fissata:
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

## Ambito

Introdotti `database_path` e `build_database` in `sanikey-core::database` con
lo schema della baseline: documenti, indice FTS5, testi estratti, metadati
clinici, provenienza dei punti osservati e studi DICOM. Le chiavi esterne sono
attive nella transazione. Il database viene scritto in uno staging temporaneo
e sostituisce quello esistente solo dopo il commit riuscito.

## Verifiche

- Baseline: `uv run pytest tests/test_database.py -q` — 4 passed.
- Rust mirato: `cargo test -p sanikey-core database::tests -- --nocapture` — 2
  passed; verifica contenuti clinici, FTS5, DICOM, foreign key e conservazione
  dell'archivio precedente quando una terapia ha un riferimento invalido.
- Comparatore cross-language su fixture sintetica:
  `cargo xtask compare sqlite <baseline-db> <rust-db> --query Blood --query hemoglobin`
  — exit 0, nessuna differenza significativa nello schema SQL, nelle righe,
  nell'integrità, nelle chiavi esterne o nei risultati FTS.
- Primo `cargo xtask check`: Clippy ha segnalato `cloned_ref_to_slice_refs` in
  un test; il test è stato corretto usando `std::slice::from_ref`.
- Gate ripetuto: exit 0; 157 test core passati, 1 ignorato, 8 xtask, 3 CLI,
  2 doctest, smoke CLI, pubblicabilità (248 file) e rustdoc.

## Limiti e prossima scheda

Il confronto semantico è stato eseguito su Linux con una fixture sintetica che
copre documenti, testo estratto, farmaco, terapia, studio DICOM e ricerche FTS.
Non costituisce una prova multipiattaforma. Il builder è disponibile nel core;
l'orchestrazione della build e della cache incrementale resta a P09.04. La
capability è `implemented`, non `verified`. Prossimo: P09.02, export JSON e
JavaScript.

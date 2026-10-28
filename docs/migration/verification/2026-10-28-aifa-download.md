# P08.03c — Download dei fogli AIFA

Data: 2026-10-28. Baseline Python fissata:
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

## Ambito

Introdotti il probe FI/RCP, il download locale della coppia, la verifica della
firma PDF `%PDF-`, diagnostiche per documento e il manifest delle date UTC.
Il download aggiorna i file solo dopo che FI e RCP sono entrambi validi; se uno
dei due fallisce conserva i file e la data locale già presenti. Il loader del
manifest ignora file assenti o JSON malformato.

## Verifiche

- Baseline: `uv run pytest tests/test_leaflets.py -q` — 12 passed.
- Rust mirato: `cargo test -p sanikey-core aifa::tests -- --nocapture` — 11
  passed, con trasporto simulato e payload sintetici.
- Formattazione, Clippy e gate: `cargo xtask check` — exit 0; 154 test core
  passati, 1 ignorato, 8 xtask, 3 CLI, 2 doctest, smoke CLI, pubblicabilità
  (244 file) e rustdoc.

## Limiti e prossimo sottopasso

Il test controlla la firma iniziale del PDF, come la baseline, non la validità
strutturale del documento. Non è stata interrogata l'API AIFA reale e non è
stata qualificata la disponibilità dei suoi endpoint. L'integrazione con
frontend/export Rust non è ancora presente; la capability resta `pending`.
Prossimo: P08.03d, uso offline dei documenti locali.

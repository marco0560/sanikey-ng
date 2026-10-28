# P08.02g — Report diagnostici di parametri

Data: 2026-10-28. Commit funzionale `1778faa`. Baseline Python fissata:
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

## Ambito

`write_parameter_rejections` produce report JSON e testo separati per righe
accettate e rifiutate, conserva il legame con documento, pagina, riga,
contesto, valore e motivo, e può includere diagnostiche di estrattori collegati
(per esempio i rifiuti dei segni vitali). Il testo tabellare allinea le colonne
in modo deterministico. I report sono artefatti locali della build.

## Verifiche

- Baseline: `uv run pytest tests/test_build.py::test_parameter_slice_build_outputs_are_byte_stable -q` — 1 passed.
- Rust mirato: `CARGO_NET_OFFLINE=true cargo test --offline -p sanikey-core parameter_reports -- --nocapture` — 3 passed.
- Clippy: `CARGO_NET_OFFLINE=true cargo clippy --offline -p sanikey-core --all-targets -- -D warnings` — exit 0.
- Formattazione: `cargo fmt --all -- --check` — exit 0.
- Gate: `CARGO_NET_OFFLINE=true cargo xtask check` — exit 0; 138 test core,
  1 ignored, 8 xtask, 3 CLI, 2 doctest, smoke CLI, pubblicabilità (236 file)
  e rustdoc superati.
- Hook pre-commit sul commit funzionale — superato; 138 test core,
  1 ignorato, 8 xtask, 3 CLI, 2 doctest e formattazione/test dei tre prototipi
  GUI superati.

Una prima asserzione mirata si aspettava le intestazioni senza padding; il
rendering mantiene la larghezza allineata dei valori come il riferimento. Il
test è stato corretto e il run finale è passato.

## Prossimo sottopasso

P08.02h porta estrazione dei segni vitali e merge con le serie osservazioni.

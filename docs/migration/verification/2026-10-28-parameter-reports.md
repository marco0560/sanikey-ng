# P08.02f — Report candidati ed export delle slice

Data: 2026-10-28. Commit funzionale `8eda1f6`. Baseline Python fissata:
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

## Ambito

Il modulo `parameter_reports` scrive JSON dei candidati con i gruppi proposti,
un template TOML con regole sempre disabilitate e, quando è disponibile il
risultato delle regole, gli export JSON e JavaScript statico di serie, punti e
decisioni. I payload preservano date, unità, decisioni e provenienza.
L'istante di generazione del template è RFC 3339 locale, a secondi, secondo la
API Chrono verificata con Context7; gli export macchina sono byte-stabili per
lo stesso input.

I report diagnostici separati per esiti accettati/rifiutati e segni vitali/merge
restano da implementare. La capability `parameters` resta pending.

## Verifiche

- Baseline: `uv run pytest tests/test_parameter_reports.py -q` — 1 passed.
- Rust mirato: `CARGO_NET_OFFLINE=true cargo test --offline -p sanikey-core parameter_reports -- --nocapture` — 2 passed.
- Clippy: `CARGO_NET_OFFLINE=true cargo clippy --offline -p sanikey-core --all-targets -- -D warnings` — exit 0.
- Formattazione: `cargo fmt --all -- --check` — exit 0.
- Gate: `CARGO_NET_OFFLINE=true cargo xtask check` — exit 0; 137 test core,
  1 ignored, 8 xtask, 3 CLI, 2 doctest, smoke CLI, pubblicabilità (235 file)
  e rustdoc superati.
- Hook pre-commit sul commit funzionale — superato; 137 test core,
  1 ignorato, 8 xtask, 3 CLI, 2 doctest e formattazione/test dei tre prototipi
  GUI superati.

Durante lo sviluppo il primo TOML proposto non superava il parser per un escape
di virgolette in `unit_policy`; corretto. Clippy ha poi rilevato una tupla di
ritorno troppo complessa; introdotto `ParameterReportPaths`. I test, Clippy e
il gate completi finali sono passati.

## Prossimo sottopasso

P08.02g genererà i report JSON e testuali per gli esiti accettati e rifiutati,
collegando i motivi ai candidati e alla loro provenienza.

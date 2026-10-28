# P08.02d — Decisioni esplicite delle regole sui candidati

Data: 2026-10-28. Baseline Python fissata:
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`. Commit funzionale `ba88e05`.

## Ambito

`evaluate_parameter_candidates` valuta ogni candidato contro le sole regole
abilitate. Per ogni regola emette il primo esito deterministico: sinonimo esatto,
campione esplicito o predefinito, formato numerico, qualificatore, data,
categoria e contesto richiesto/escluso. Se più regole accettano lo stesso
candidato, tutte le accettazioni diventano `REJECTED_MULTIPLE_RULE_MATCHES`.
Le decisioni sono ordinate per ID candidato, ID regola e motivo.

Il sottopasso non verifica ancora unità, conversioni, intervalli numerici,
materializzazione dei punti/serie, report, segni vitali o merge. La capability
`parameters` e la parità complessiva restano pending.

## Verifiche

- Baseline: `uv run pytest tests/test_parameter_rules.py -q` — 7 passed.
- Rust mirato: `CARGO_NET_OFFLINE=true cargo test --offline -p sanikey-core parameter_rules -- --nocapture` — 6 passed.
- Clippy: `CARGO_NET_OFFLINE=true cargo clippy --offline -p sanikey-core --all-targets -- -D warnings` — exit 0.
- Formattazione: `cargo fmt --all -- --check` — exit 0.
- Gate: `CARGO_NET_OFFLINE=true cargo xtask check` — exit 0; 133 test core,
  1 ignored, 8 xtask, 3 CLI, 2 doctest, smoke CLI, pubblicabilità (230 file)
  e rustdoc superati.
- Hook pre-commit sul commit funzionale — superato; 133 test core,
  1 ignorato, 8 xtask, 3 CLI, 2 doctest e formattazione/test dei tre prototipi
  GUI superati.

Durante la stesura due esecuzioni mirate hanno fallito per fixture di test
incomplete: la regola consentiva qualificatori e non limitava le categorie.
Le fixture sono state corrette; il run mirato finale e il gate completo sono
passati.

## Prossimo sottopasso

P08.02e applicherà politiche delle unità e conversioni esplicite e creerà punti
e serie con la provenienza del candidato e della regola.

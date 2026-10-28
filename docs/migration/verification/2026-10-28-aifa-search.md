# P08.03a — Ricerca e classificazione candidati AIFA

Data: 2026-10-28. Commit funzionale `2afbe8d`. Baseline Python fissata:
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

## Ambito

Introdotto `AifaClient` sincrono con `ureq 2.12.1`, TLS rustls e timeout
connect/read/write di 30 secondi. La ricerca genera query commerciali e per
principio attivo, elimina duplicati normalizzati, estrae payload JSON AIFA,
deduplica e ordina i candidati, poi separa corrispondenze esatte da risultati
plausibili da sottoporre a revisione. La dipendenza e le API sono state
controllate con Context7 e la documentazione ufficiale del client
[ureq 2.12.1](https://github.com/algesten/ureq/blob/2.12.1/README.md).

## Verifiche

- Baseline: `uv run pytest tests/test_leaflets.py -q` — 12 passed.
- Rust mirato: `cargo test -p sanikey-core aifa::tests -- --nocapture` —
  6 passed con risposte JSON sintetiche e trasporto iniettato.
- Clippy e rustfmt — exit 0.
- Gate: `cargo xtask check` — exit 0; 149 test core passati, 1 ignorato,
  8 xtask, 3 CLI, 2 doctest, smoke CLI, pubblicabilità (243 file) e rustdoc.
- Hook pre-commit — superato; 149 test core, 1 ignorato, 8 xtask, 3 CLI,
  2 doctest e formattazione/test dei tre prototipi GUI.
- Lockfile dei tre workspace GUI isolati riallineati offline alla nuova
  dipendenza core; i relativi test del hook sono passati.

Il primo hook ha interrotto il commit perché i tre lockfile GUI non includevano
ancora la nuova dipendenza transitiva del core. Sono stati aggiornati con
`cargo check --offline --manifest-path <prototipo>/Cargo.toml`; il secondo
hook è passato senza bypass.

## Limiti e prossimo sottopasso

Il bind loopback è vietato dalla sandbox; le risposte sintetiche sono quindi
state iniettate direttamente come nei test monkeypatch della baseline. Non è
stata interrogata l'API AIFA reale, né qualificata la sua disponibilità. La
capability resta `pending` finché non saranno portati selezione persistita,
download e verifica FI/RCP. Prossimo: P08.03b, persistenza della selezione
esplicita e dell'impronta dei campi curati.

# Ricevuta P03.02 — Errori ed eventi delle operazioni

Data: 2026-10-26. La baseline verificata è il commit
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`. `errors.py` definisce le
categorie `ConfigError`, `PrivacyError` e `UsbError`; `progress.py` espone
avanzamento testuale solo su TTY. L'inventario non rileva un contratto CLI di
annullamento esplicito.

`sanikey-core` ora separa il codice macchina (`configuration`, `privacy`,
`usb`) dal messaggio italiano, espone fasi ed eventi tipizzati e offre un
token di annullamento cooperativo, osservato ai punti sicuri del chiamante.
Il runner registra `Started`, progressi ed esattamente un evento terminale
(`Succeeded`, `Failed` o `Cancelled`). Il comando reale `sanikey info` usa il
runner; gli altri comandi restano non implementati e falliscono. Nessun flag
CLI di annullamento è stato introdotto.

## Verifiche

- `cargo fmt --all -- --check`: superato.
- `cargo test --workspace --locked`: 9 unit test e 1 doctest superati.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: superato.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`:
  superato.
- `cargo run --package sanikey-cli --locked -- info`: exit 0 e output atteso.
- `git diff --check`: superato.

I test coprono successo e ordine eventi, fallimento con codice e messaggio
distinti, annullamento e assenza di successo dopo errore/annullamento. Il
comportamento di annullamento è interno al core: il contratto utente CLI resta
da definire solo se introdotto da una scheda successiva.

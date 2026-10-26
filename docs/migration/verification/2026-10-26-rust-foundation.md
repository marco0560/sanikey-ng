# Ricevuta P03.01 — Workspace Rust iniziale

Data: 2026-10-26. Toolchain disponibile nell’ambiente: `rustc 1.98.1`,
`cargo 1.98.1`, `rustfmt 1.9.0-stable`, Clippy `0.1.98`.

Il workspace ha i membri `sanikey-core`, `sanikey-cli` e `xtask`, senza
dipendenze esterne. `sanikey-core::version()` alimenta `sanikey --version` e
`sanikey info`; gli altri comandi non sono presenti e una richiesta di build
non supportata termina con errore. `cargo xtask verify-toolchain` confronta
rustc e Cargo con la versione fissata.

## Scelta e fonti della toolchain

La data di verifica indicava Rust 1.99.1 come ultima stable pubblicata; il
workspace è fissato a 1.98.1, già installata e disponibile sul runner locale,
così la scheda è verificabile senza introdurre un download/toolchain ulteriore.
Non ci sono dipendenze né API che richiedano una versione più recente. `rustup`
non è installato in questo ambiente; la versione esatta è stata verificata
direttamente e il file `rust-toolchain.toml` consente a rustup/CI di selezionare
la stessa release.

Fonti primarie consultate:

- [Annunci di rilascio Rust](https://blog.rust-lang.org/releases/) per lo
  stato delle stable pubblicate.
- [File toolchain rustup](https://github.com/rust-lang/rustup/blob/main/doc/user-guide/src/overrides.md)
  per pin, profilo e componenti del file `rust-toolchain.toml`.
- [Workspace Cargo](https://doc.rust-lang.org/cargo/reference/workspaces.html)
  e [resolver Cargo](https://doc.rust-lang.org/cargo/reference/resolver.html)
  per workspace virtuale, lockfile condiviso e resolver 3 nell’edizione 2024.

## Verifiche

- `cargo xtask verify-toolchain`: exit 0; rilevati rustc e Cargo 1.98.1.
- `cargo fmt --all -- --check`: exit 0.
- `cargo build --workspace --locked`: exit 0.
- `cargo test --workspace --locked`: exit 0; 5 unit test superati.
- `cargo test --doc --workspace --locked`: exit 0; 1 doctest superato.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`:
  exit 0.
- `cargo run --package sanikey-cli --locked -- info` e `--version`: exit 0,
  output atteso.

Il primo `cargo fmt --check` ha segnalato solo formattazione non conforme;
`cargo fmt` l’ha corretta e il controllo successivo è passato. Non è stata
installata una toolchain. Il gate completo `cargo xtask check`, la CI e i target
multipiattaforma restano a P03.03; nessuna capability della matrice è stata
marcata verificata.

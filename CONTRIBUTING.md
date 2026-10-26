# Contribuire

## Stato dell'ambiente

P03.01 ha introdotto workspace Cargo, `Cargo.lock`, toolchain fissata in
`rust-toolchain.toml` e `cargo xtask verify-toolchain`. Il gate completo
`cargo xtask check` sarà implementato in P03.03. L'esecutore previsto delle
schede di implementazione è GPT-6-Luna High.

## Ciclo di una scheda

1. Leggere piano, ledger, scheda e fonti della baseline.
2. Verificare dipendenze e stato Git; preservare modifiche non proprie.
3. Scrivere una modifica circoscritta con test che dimostrino il contratto.
4. Eseguire controlli pertinenti e aggiornare documentazione e parità.
5. Creare il commit atomico e registrare risultati e prossimo passo.

Il ledger è l'autorità sul completamento delle schede. Le schede definiscono
il lavoro, senza duplicare uno stato di esecuzione indipendente.

## Controlli documentali del bootstrap

Prerequisiti: Git e un lettore TOML conforme. Directory: radice del repository.

- Verificare contenuti candidati al commit e assenza di dati privati.
- Verificare collegamenti Markdown locali, indice delle schede e dipendenze.
- Analizzare baseline.toml e parity.toml e verificarne la coerenza col ledger.
- Eseguire `git diff --cached --check` prima del commit.
- Confermare che tutte le schede richieste dal piano siano presenti.

Il bootstrap può essere verificato con strumenti di sviluppo esterni.
Questi strumenti non diventano dipendenze del generatore distribuito.
In caso di errore, correggere il documento responsabile prima di pubblicare.

## Gate Rust da costruire in P03

Il comando `cargo xtask check` dovrà eseguire, con la toolchain fissata:

- formattazione in modalità controllo;
- Clippy su workspace e target concordati, con warning bloccanti;
- test dei target e test degli esempi rustdoc in passaggi distinti;
- generazione rustdoc con warning bloccanti;
- controlli documentali, dei contratti e dei contenuti pubblicabili.

I comandi Cargo dovranno usare Cargo.lock senza aggiornamenti impliciti.
Le combinazioni di feature saranno elencate e verificate singolarmente:
non assumere che tutte le feature di backend siano combinabili.

Prima della chiusura di P03.03, i controlli di base sono riproducibili con:

```bash
cargo xtask verify-toolchain
cargo fmt --all -- --check
cargo test --workspace --locked
cargo test --doc --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
```

Riferimenti:
[Cargo test](https://doc.rust-lang.org/cargo/commands/cargo-test.html) e
[Cargo doc](https://doc.rust-lang.org/cargo/commands/cargo-doc.html).

## Rust e documentazione

Usare tipi di dominio, errori espliciti e confini tra logica e I/O.
Documentare API pubbliche, errori, invarianti e uso significativo con rustdoc.
Gli esempi compilabili diventano doctest. Evitare gerarchie, async e
astrazioni introdotte senza un'esigenza dimostrata.
Il dominio non deve contenere unsafe; eventuali confini FFI richiedono
isolamento, motivazione e verifiche specifiche.

## Git

Ogni scheda conclusa produce un commit atomico. I tipi ammessi sono feat,
fix, docs, perf, refactor, test, chore e style. Gli scope iniziali sono
core, cli, desktop, migration, build, ci, docs, validation e release.
Il subject deve essere descrittivo, in forma imperativa e lungo al massimo
72 caratteri. Il bootstrap può usare un subject senza scope.

Non riscrivere storia pubblicata. Non generare release a ogni push.
La pubblicazione del repository sanikey-ng è approvata; il repository
Python e le decisioni G1–G4 hanno controlli distinti.

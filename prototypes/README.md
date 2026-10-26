# Prototipi GUI P04

Questi eseguibili isolati servono solo al confronto P04.06; non sono crate del
prodotto e non entrano nel workspace Cargo. Usano fixture sintetiche e non
leggono né scrivono cartelle reali salvo l'apertura del dialogo di selezione.

## Avvio

Dalla radice del repository, dopo la prima compilazione:

```bash
cargo run --release --offline --locked --manifest-path prototypes/gui-egui/Cargo.toml
```

```bash
cargo run --release --offline --locked --manifest-path prototypes/gui-slint/Cargo.toml
```

Per Slint lo stile è selezionato in compilazione. Per provare le due varianti
Fluent in modo ripetibile:

```bash
SLINT_STYLE=fluent-light cargo run --release --offline --manifest-path prototypes/gui-slint/Cargo.toml
```

```bash
SLINT_STYLE=fluent-dark cargo run --release --offline --manifest-path prototypes/gui-slint/Cargo.toml
```

Il lockfile separato di ciascun prototipo fissa le dipendenze; dopo averli
scaricati una prima volta, `--offline --locked` impedisce aggiornamenti e
richieste di rete. Il dialogo cartelle usa `rfd` 0.17.2 su entrambi i front-end.
Questi comandi non dimostrano l'avvio su Windows/macOS né la distribuzione su
sistemi puliti.

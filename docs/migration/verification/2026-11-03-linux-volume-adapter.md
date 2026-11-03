# Ricevuta P10.02 — Adattatore Linux dei volumi

Data: 2026-11-03. Implementato `sanikey_core::linux_volume`, dietro `cfg` Linux.
La funzione di ispezione richiede un target già esistente, legge lo spazio
filesystem con `rustix::fs::statvfs` e interroga `findmnt --json --output
TARGET,FSTYPE,UUID --target <target>`. Non crea directory né scrive sul target.
Il validatore applica l'UUID globale, l'UUID condiviso delle persone abilitate
solo sotto `/media` o `/run/media`, il vincolo exFAT e il margine minimo in MiB.
Come la baseline, l'assenza di `findmnt` è tollerata senza UUID globale o
richiesta exFAT; in quel caso l'UUID persona non viene confrontato.

La misura usa `f_bfree * f_frsize` (con fallback a `f_bsize`) per mantenere il
comportamento di `shutil.disk_usage(...).free`. Il binding `rustix` è safe e
mantiene `#![forbid(unsafe_code)]`; è una dipendenza di compilazione Linux,
senza requisito runtime aggiuntivo. `findmnt` è il requisito runtime opzionale
di util-linux secondo la configurazione. Riferimenti: [rustix 1.1.5
`statvfs`](https://docs.rs/rustix/1.1.5/rustix/fs/fn.statvfs.html) e
[P04.04](2026-10-26-volume-identity-feasibility.md).

## Verifiche

- `cargo test --offline -p sanikey-core linux_volume`: 7 test passati, 0 falliti.
  Fixture sintetiche verificano parsing JSON, UUID globale e persona, percorsi
  Linux, UUID errato, filesystem errato, soglia di spazio e assenza di metadati.
- Un test ha letto metadati e spazio della directory del checkout senza
  modificarla. Questa è una prova sul filesystem host Linux, non su una USB.
- `CARGO_NET_OFFLINE=true cargo check --manifest-path prototypes/gui-egui/Cargo.toml`:
  exit 0; lockfile aggiornato a `rustix 1.1.5`.
- `CARGO_NET_OFFLINE=true cargo check --manifest-path prototypes/gui-slint/Cargo.toml`:
  exit 0; lockfile aggiornato per la dipendenza transitiva.
- `CARGO_NET_OFFLINE=true cargo check --manifest-path prototypes/gui-protocol/Cargo.toml`:
  exit 0; lockfile aggiornato con la dipendenza Linux.
- `uv run codira index`: exit 0, 1 file indicizzato, 213 riusati, 0 errori;
  restano i 12 tipi non coperti già attesi per CSS, JS, SVG, template e parole.
- Il primo `cargo test -p sanikey-core linux_volume` ha tentato l'accesso
  all'indice crates.io e ha fallito per DNS non disponibile. Il test è stato
  ripetuto offline con il registry già presente e ha superato tutti i casi.
- Nessun target USB fisico o UUID reale è stato consultato. La qualifica su
  supporto qualificato resta separata; Windows e macOS sono fuori da P10.02.

`CARGO_NET_OFFLINE=true cargo xtask check`: exit 0. Formattazione e Clippy
superati; test workspace 189 core passati, 2 ignored, 8 xtask e 3 CLI passati;
2 doctest passati; smoke CLI, controlli pubblicabilità (276 file) e rustdoc
superati. Il commit funzionale `ccdadba` (`feat(core): add Linux volume adapter`)
ha superato gli hook pre-commit: diff staged, formattazione, test workspace e
formattazione/test dei tre prototipi GUI. Il protocollo GUI ha passato 1 test;
egui e Slint non hanno test unitari; i tre target sono comunque stati compilati.

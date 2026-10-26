# SaniKey NG

Migrazione di [SaniKey](https://github.com/marco0560/sanikey) verso un generatore di archivi
sanitari e chiavette USB scritto in Rust, con applicazione grafica per
Windows, Linux e macOS.

## Stato

Il repository contiene il workspace Rust iniziale: `sanikey-core`, la CLI
minima e `xtask`. La CLI supporta informazioni e versione; il generatore,
la GUI e il gate completo non sono ancora implementati. SaniKey Python resta
il generatore operativo.

Il campione di controllo è il commit Python
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`, creato includendo tutti i dodici file
precedentemente modificati. La validazione locale del campione ha superato
359 test con un test saltato e copertura complessiva del 92%.
P01.03 ha qualificato un checkout pulito Linux e ha verificato il fetch remoto
dello SHA. P02.01 ha inventariato il contratto e P02.02 ha fissato il corpus
sintetico; nessuna capability Rust della matrice di parità è ancora verificata.

## Riprendere il lavoro

Aprire [le istruzioni per Luna High](docs/migration/START-HERE.md), poi
consultare [il piano approvato](docs/migration/plan.md) e
[il registro di avanzamento](docs/migration/ledger.md).

## Documentazione

- [Contratto del prodotto](docs/product-contract.md).
- [Architettura prevista](docs/architecture.md).
- [Contribuire e verificare](CONTRIBUTING.md).
- [Distribuzione](docs/deployment.md).
- [Percorso di apprendimento Rust](docs/learning-rust.md).
- [Decisioni architetturali](docs/decisions/README.md).
- [Identità della baseline](docs/migration/baseline.toml).
- [Matrice iniziale della parità](docs/migration/parity.toml).
- [Inventario del contratto](docs/migration/contract-inventory.md).
- [Corpus sintetico e expected](docs/migration/corpus/README.md).
- [Indice delle schede](docs/migration/tasks/README.md).

La GUI serve alla preparazione dell'archivio e della chiavetta.
La consultazione clinica esportata resta statica, offline e apribile da
`index.html`, senza installazione sul computer del medico.

## Dati e licenza

Versionare esclusivamente codice, documentazione e fixture sintetiche.
Configurazioni reali, documenti clinici, build e log operativi devono restare
fuori dal repository pubblico.

Licenza [MIT](LICENSE). Le derivazioni dal progetto Python manterranno le
attribuzioni e gli avvisi delle dipendenze e degli asset incorporati.

# P04.06 — Prototipi GUI e confronto per G1

Data: 2026-10-26
Esito: entrambi i prototipi compilano su Linux; G1 resta in attesa del manutentore.

## Ambito provato

I prototipi sono crate indipendenti in `prototypes/`, esclusi dal workspace del
prodotto. La finestra, le stringhe italiane, le tre righe sintetiche e gli
scenari di avanzamento, errore e annullamento sono equivalenti. Un crate
condiviso genera la sequenza `OperationEvent` con `sanikey-core::run_operation`;
entrambi presentano gli stessi eventi a intervalli di 250 ms.

Questo temporizzatore è un mock UI: il core restituisce una sequenza conclusa
e il prototipo la riproduce. Non dimostra ancora l'integrazione con un'operazione
asincrona reale né il comportamento della GUI sotto lavoro effettivo.
Entrambe le finestre offrono campi di percorso e usano `rfd` 0.17.2 per
selezionare le cartelle. La documentazione di `rfd` marca la stabilità dell'API
come sperimentale; il dialogo non è stato aperto durante questa prova.

## Risultati sul sistema disponibile

Sistema: Linux x86_64, `rustc 1.98.1`. Le due dipendenze UI e `rfd` sono
versionate esattamente e hanno lockfile separati.

| Misura | egui/eframe | Slint |
| --- | ---: | ---: |
| Versioni UI | `egui`, `eframe`, `egui_extras` 0.37.0 | `slint` 1.18.1 |
| File dialog condiviso | `rfd` 0.17.2, MIT | `rfd` 0.17.2, MIT |
| Crate/version nodes, `cargo tree --edges normal` | 366 | 443 |
| Binario release Linux | 29.263.024 byte | 30.923.256 byte |
| Librerie dinamiche osservate con `ldd` | `libc`, `libm`, `libgcc_s` | `libc`, `libm`, `libgcc_s`, `fontconfig`, `freetype`, `expat`, `zlib`, `bzip2`, `libpng` |
| Licenza dichiarata dal crate principale | MIT OR Apache-2.0 | GPL-3.0-only OR licenze Slint royalty-free/software |

Conteggi e dimensioni si riferiscono a questi lockfile, feature predefinite e
build release Linux; non sono confronti normalizzati o pacchetti installabili.
`ldd` osserva solo il linkage sul sistema di sviluppo e non rileva ogni libreria
caricata dinamicamente dai renderer. Nessun pacchetto Windows/macOS o host pulito
è stato costruito.

### egui con eframe

- `eframe::App::ui` implementa il mock; `egui_extras::TableBuilder` presenta la
  tabella e `rfd` fornisce i dialoghi cartella.
- Il tema chiaro/scuro cambia a runtime. `eframe` abilita AccessKit dove è
  disponibile un adapter nativo; gli input hanno label collegate.
- Il grafo predefinito include `wgpu`, X11 e Wayland. I nodi del grafo e il
  binario Linux risultano minori in questa prova. Questo non dimostra il
  comportamento dei driver GPU o il confezionamento autonomo.
- Il ruolo semantico della tabella e l'intero albero AccessKit non sono stati
  verificati con un lettore di schermo.

### Slint

- Il markup dichiarativo include `StandardTableView`, `ProgressIndicator`,
  ruoli e label accessibili; la feature `accessibility` è predefinita.
- La palette segue il tema di sistema. Le varianti `fluent-light` e
  `fluent-dark` hanno superato `cargo check`, selezionate con `SLINT_STYLE` in
  compilazione; il cambio di stile non è un controllo runtime del prototipo.
- L'espressione di licenza del crate consente i termini GPLv3 o le licenze
  Slint indicate. La licenza Community richiede attribuzione. G1 deve accettare
  esplicitamente condizioni e attribuzione prima che Slint diventi dipendenza
  del prodotto.
- `ldd` rileva dipendenze di font/rendering esterne nell'eseguibile Linux.
  La loro presenza e il confezionamento devono essere risolti per ogni OS.

## Accessibilità, DPI e interazione

I due prototipi compilano, ma non sono stati azionati con mouse/tastiera né
provati con screen reader o a scale DPI differenti. Non dichiaro superata
nessuna verifica GUI manuale. I contratti pubblici indicano possibilità e
API, non attestano annunci corretti, ordine del focus, contrasto o usabilità.

egui offre tema runtime e integrazione AccessKit verificabile con `egui_kittest`;
Slint dà accesso dichiarativo a ruoli, label, tabella e progress indicator, ma
il tema va selezionato in compilazione. La tabella va verificata con screen
reader in entrambi i prototipi prima dell'uso da parte degli operatori.

## Verifiche eseguite

- `cargo fmt` sui tre crate isolati: superato.
- `cargo check --offline --locked` su egui e Slint: superato.
- `cargo test --offline --locked` sulle app: superato (nessun test GUI nei due
  binari); test del crate condiviso: 1 superato.
- `cargo clippy --all-targets --offline --locked -- -D warnings` su entrambe le
  app: superato.
- `cargo build --release --offline --locked` su entrambe le app: superato.
- `SLINT_STYLE=fluent-light cargo check --offline --locked` e variante
  `fluent-dark`: superati.
- `ldd` e `cargo tree --edges normal`: eseguiti solo su Linux.
- Una prima `cargo metadata --offline` non era risolvibile perché mancavano
  nell'indice locale crate per target non Linux; la rilevazione è stata
  completata con `cargo metadata --locked` e accesso al registry. Non è una
  qualifica di build Windows/macOS.
- Il gate `cargo xtask check` del workspace prodotto è separato dai prototipi.

## Fonti ufficiali consultate

- [egui 0.37.0 release](https://github.com/emilk/egui/releases/tag/0.37.0)
- [egui accessibilità](https://github.com/emilk/egui/blob/0.37.0/docs/accessibility.md)
- [egui architettura](https://github.com/emilk/egui/blob/0.37.0/ARCHITECTURE.md)
- [Slint 1.18.1 crate e dipendenze](https://docs.rs/crate/slint/1.18.1)
- [Slint StandardTableView](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/standardtableview/)
- [Slint ProgressIndicator](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/basic-widgets/progressindicator/)
- [Slint stili e scelta in compilazione](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/style/)
- [Slint licenze](https://slint.dev/faqs)
- [`rfd` 0.17.2, API e stato](https://docs.rs/rfd/0.17.2/rfd/)
- Context7: `/emilk/egui/0.37.0`, `/websites/slint_dev`; risoluzione di `rfd` non disponibile.

## Conclusione per G1

I dati non stabiliscono un vincitore: egui ha licenza permissiva e tema runtime;
Slint offre componenti e ruoli dichiarativi più espliciti, con costi di
distribuzione Linux e termini di licenza da accettare. Mancano soprattutto le
prove visuali e con screen reader sui sistemi target. La scelta finale è
riservata al manutentore. La capability `desktop` resta `pending`.

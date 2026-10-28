# Ricevuta P08.01 — Importazioni di osservazioni

Data: 2026-10-28. Baseline: commit Python fissato
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`. La SHA-256 del modulo estratto
dalla baseline coincide con quella del checkout isolato usato per i test.
Sono state usate esclusivamente fixture sintetiche.

## Comportamento portato

- Lettura CSV e fogli Calamine `.ods`, `.xls`, `.xlsb`, `.xlsm` e `.xlsx`, con
  selezione del foglio e gestione di intestazioni singole o multiple.
- Manifest con serie, mappature di colonne, estrazioni multiple, riporto di
  celle vuote configurate, note e valori composti tramite gruppi regex.
- Layout tabellare e matrice ripetuta, date ISO, `gg/mm/aaaa`, `gg-mm-aaaa`,
  policy `year_start` e `period_start`, valori numerici a virgola o punto,
  unità dichiarate nella serie e avvisi per duplicati nella stessa giornata.
- Hash SHA-256 di manifest e fonti; `ensure_observation_imports_current`
  rifiuta artefatti mancanti o fonti/manifest modificati.
- Gli artefatti sono scritti in `metadata/observations/imported/`, separati
  dai TOML curati. I file curati in `metadata/observations/` non vengono
  sostituiti.

La baseline non converte le unità: l'unità dichiarata resta metadato della
serie. La directory `imported/` è un output generato dedicato, non un percorso
di metadati curati.

## Verifiche

| Controllo | Esito |
| --- | --- |
| Test baseline Python `tests/test_observation_imports.py` allo SHA fissato | 7 passed |
| Test Rust `observation_imports` | 8 passed, inclusi XLSX sintetico, staleness e integrità dei TOML curati |
| `cargo clippy --offline -p sanikey-core --all-targets -- -D warnings` | Exit 0 |
| `cargo xtask check` | Exit 0; 115 core passed, 1 ignored, 8 xtask, 3 CLI, 2 doctest; pubblicabilità e rustdoc superati |
| Hook pre-commit | Exit 0; aggiunti test dei prototipi GUI: protocollo 1, egui 0, Slint 0 |

Cargo ha usato un cache temporaneo sotto `.Temp`, aggiornato prima con
Calamine 0.36.1, già qualificato in P04.02. Il primo hook ha individuato lock
file GUI che non comprendevano le nuove dipendenze del core; tutti e tre sono
stati aggiornati offline e il pre-commit completo è stato ripetuto con esito 0.
Il normale accesso a crates.io nel sandbox non risolveva il DNS; nessun hook è
stato saltato.

Codira non è stato usato per query: il rebuild dell'indice Rust registrato in
P07.07 terminava con SIGSEGV (exit 139), quindi non sono stati consultati dati
indicizzati obsoleti.

Commit funzionale: `bda9293` (`feat(core): porta importazioni di osservazioni`).
La capability `observations` è `implemented`, non `verified`: in questa scheda
il test Rust dei fogli di calcolo copre XLSX; XLS, XLSB, XLSM e ODS, nonché
Windows e macOS, non sono stati qualificati. Non è dimostrata la parità
completa del sottosistema né l'integrazione con frontend/database.

# Ricevuta P06.01 — Inventario e digest

Data: 2026-10-28. Baseline Python fissata a
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`; sorgenti di riferimento
`src/sanikey/documents.py` e `tests/test_documents.py`.

## Comportamento portato

- Scansione ricorsiva in sola lettura, ordinata per percorso relativo POSIX.
  Una root mancante o non-directory produce inventario vuoto; gli errori di
  attraversamento e lettura dei file sono restituiti con contesto sul percorso.
- Hash SHA-256 incrementale in blocchi, identità del documento uguale al digest,
  provenienza `source`, categoria dalla prima directory e data/titolo dal
  prefisso `YYYYMMDD`.
- Classificazione per estensione e firma DICOM; supportati PDF, testo, immagini,
  archivi, Office/fogli di calcolo, DICOM e tipo binario di fallback. I prefissi
  di directory di servizio non appaiono nella categoria.
- Pattern d'ingestione con `*`, `?`, classi, corrispondenza case-insensitive su
  percorso e nome file, esclusione prima dell'hash e precedenza include. Il
  prefisso `**/` ammette anche zero directory, come nella baseline.
- L'inventario completo conserva i duplicati; il raggruppamento li associa al
  digest e la vista deduplicata mantiene il primo percorso ordinato. I byte
  degli originali restano invariati.
- P06.01 espone i servizi di inventario nel core; non implementa estrazione o
  pipeline di build, che restano nelle schede successive.

## Verifiche

| Comando | Esito |
| --- | --- |
| `cargo test --offline -p sanikey-core inventory` | Exit 0 su Linux: 7 test mirati |
| `cargo xtask check` | Exit 0 su Linux: fmt, Clippy, 3 test CLI, 62 test core, 8 test xtask, 1 doctest, build, smoke CLI, 3 test della guardia Python, pubblicabilità (196 file) e rustdoc |
| `uv run codira index`; `uv run codira cov`; `uv run codira audit` | Index exit 0 (2 indicizzati, 145 riusati); copertura completa senza gap; audit exit 0 con quattro rilievi rustdoc preesistenti negli helper metadata/privacy, fuori ambito |
| `git diff --check` | Exit 0 |

Il primo tentativo con `cargo test -p sanikey-core inventory` ha cercato di
aggiornare l'indice crates.io per `globset`, non presente nel lockfile, ma la
rete non risolveva `index.crates.io`; il comando è stato interrotto prima del
build. La dipendenza non è stata aggiunta: i pattern necessari sono gestiti dal
matcher interno e SHA-256 usa `sha2`, già bloccato dal workspace.

La prima esecuzione di `cargo xtask check` si è fermata al lint
`clippy::manual_memcpy` nel matcher `?`; il ciclo è stato sostituito con
`copy_from_slice` e il gate completo è poi passato.

Il test del corpus `minimal` confronta i tre percorsi ordinati, due digest
registrati nel manifest e il duplicato con la baseline Python. Le prove sono
state eseguite solo su Linux; Windows e macOS non sono stati provati in questa
scheda. Sono state usate solo fixture sintetiche.

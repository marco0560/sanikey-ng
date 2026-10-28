# P06.02 — Pipeline TXT consultabile

Data: 2026-10-28. Sistema verificato: Linux.

## Perimetro

`sanikey-core::txt_pipeline::build_txt_pipeline` legge soltanto i documenti
`.txt` ammessi e deduplicati dall'inventario. Decodifica UTF-8 sostituendo i
byte non validi e normalizza CRLF/CR a LF, come la lettura testuale Python della
baseline. Crea un database con `documents`, `document_text` e FTS5; esporta i
metadati documentali e il testo in JSON; genera una pagina HTML italiana che
mostra i testi e collega copie locali dei file originali. I percorsi esportati
sono relativi e i nomi URL vengono percent-encoded.

Il test usa esclusivamente il corpus sintetico versionato. Confronta
identificatore, percorso, titolo, categoria, tipo, data e tag con
`expected/minimal-python.json`; verifica ricerca FTS5, contenuto, link e byte
originali. Un secondo caso verifica UTF-8 non valido, newline, escaping HTML e
encoding URL.

Questo non completa le capability `sqlite`, `exports` o `frontend`: i relativi
stati restano `pending`. Markdown, CSV, PDF e altri formati, il catalogo clinico
completo, la UI Python e la parità su Windows/macOS non sono implementati né
verificati in questa scheda. Non è stata eseguita una qualifica browser manuale.

## Dipendenza

`rusqlite 0.38.0` con feature `bundled` usa SQLite incorporato e abilita FTS5;
la stessa versione e feature erano già usate dal crate `xtask`. API di
connessione, transazione e query parametrizzate consultate sulla
[documentazione rusqlite](https://docs.rs/rusqlite/0.38.0/rusqlite/).
La serializzazione usa `serde_json 1.0.154`, già fissato nel lockfile per
`xtask`; riferimento: [documentazione serde_json](https://docs.rs/serde_json/1.0.154/serde_json/).

## Verifiche

- `cargo fmt --all`: exit 0.
- `cargo test --offline -p sanikey-core txt_pipeline`: 2 test superati.
- `cargo xtask check`: exit 0; Clippy, 3 test CLI, 64 test core, 8 test xtask,
  doctest, build, smoke CLI, controlli di pubblicabilità e rustdoc superati.
- `python3 scripts/check_publishable.py`: exit 0, 199 file pubblicabili;
  `git diff --check`: exit 0.
- I test dei tre workspace isolati GUI, eseguiti singolarmente con
  `--offline`, sono passati dopo aver aggiornato i lockfile per le nuove
  dipendenze transitive di `sanikey-core`.
- Hook pre-commit: exit 0; test workspace, doctest e test dei tre prototipi GUI
  superati.

Durante lo sviluppo, il primo test confrontava il testo del corpus con una
stringa che non era nel fixture; il test ha poi riletto il contenuto atteso
dall'originale. Il controllo successivo ha rilevato che la copia sul disco
usava il percorso URL percent-encoded; la copia ora usa il percorso relativo
originale mentre l'href resta codificato. Entrambe le correzioni sono coperte
dal test di pipeline.

Il primo gate Clippy ha segnalato un cast `u8` ridondante nel test, corretto
senza eccezioni; la seconda esecuzione completa è passata.

Codira non ha fornito orientamento indicizzato: `uv run codira index` è
terminato con exit 135; il rebuild completo differendo gli embeddings ha
restituito `BrokenProcessPool` (exit 2), e modalità `thread` e `off` sono
terminate con SIGSEGV (exit 139). `codira ctx` ha segnalato l'indice locale
illeggibile (`generation 31` in aggiornamento, ultima generazione riuscita 30).
Un rebuild con output pulito in `.Temp` ha riprodotto SIGSEGV (exit 139), quindi
il problema non dipende dal database locale. La configurazione Codira non è
stata modificata. Le fonti sono state lette direttamente dal commit baseline
fissato in `baseline.toml`.

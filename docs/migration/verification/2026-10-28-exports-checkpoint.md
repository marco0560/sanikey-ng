# P09.02 — Checkpoint export JSON e JavaScript

Stato: **in corso**, non dimostra la parità completa degli export.

## Ambito verificato

Il modulo Rust genera catalogo documenti, ricerca, timeline, sommario, script
JavaScript per il caricamento `file://`, ricerca avanzata, payload clinici,
link AIFA e DICOM, report tecnici e manifest viewer. Gli input di documenti e
studi con un `patient_id` diverso vengono esclusi.

## Verifiche

- Baseline fissata: estratto isolato di `fdad1d93e56496cc6cb9933b3089729175d5b5b3`;
  `uv run --project <checkout-ambiente-Python> --no-sync pytest
  tests/test_exports.py -q` con `PYTHONPATH=<baseline>/src` — 8 passed.
- Rust: `cargo fmt --all -- --check`, `cargo clippy -p sanikey-core
  --all-targets -- -D warnings` e `cargo test -p sanikey-core exports::tests
  -- --nocapture` — superati, 8 test. La prova aggiunta verifica la ricerca di
  procedure e osservazioni e l'esclusione dei file DICOM tecnici quando lo
  studio non dispone di un viewer.
- Fixture sintetica non vuota con documento, tag, problemi clinici, farmaco,
  terapia, riferimenti AIFA, studio DICOM, viewer offline e testo indicizzato:
  `cargo xtask compare frontend <baseline-web> <rust-web>` — nessuna
  differenza significativa.
- Confronti separati di ricerca e timeline JSON, report tecnico CSV/HTML e
  manifest viewer JSON — nessuna differenza significativa.
- Un primo test timeline fallì perché il fixture assegnava la stessa data ai
  due documenti; corretta la data nel test e ripetuta la suite con esito 7/7.
- Un primo tentativo di `uv run` nell'estrazione baseline non riuscì a
  scaricare i requisiti di build per DNS assente. Il test è stato poi eseguito
  con l'ambiente Python locale non sincronizzato e `PYTHONPATH` puntato
  all'estrazione esatta della baseline. Il repository Python è rimasto pulito.
- Codira: prima l'indice incrementale terminò con `BrokenProcessPool` e il
  tentativo con `--concurrency thread` con SIGSEGV (139). Dopo la correzione
  dell'analizzatore Rust segnalata dal manutentore, `uv run codira index` è
  terminato con exit 0 (`Indexed: 0`, `Reused: 201`, `Failed: 0`; 2241
  embeddings riusati). `uv run codira cov` segnala come unico file non coperto
  `.user-words`, che non ha un analyzer registrato; l'analyzer Rust v4 copre
  i file `.rs`. Le query Codira successive hanno usato la generazione valida.

## Blocco G2 e lavoro residuo

Il contratto Python renderizza CommonMark con HTML raw disabilitato nei
documenti Markdown e nella sintesi clinica. Il workspace Rust non contiene un
parser Markdown. È stata chiesta approvazione G2 per aggiungere
`pulldown-cmark` 0.13 e trasformare gli eventi HTML raw in testo escapato;
nessuna dipendenza è stata aggiunta in attesa della decisione.

Da fare dopo G2: implementare e confrontare la resa Markdown; ripetere i
confronti completi, `cargo xtask check`, aggiornare il ledger e chiudere la
scheda con un commit atomico. Fino ad allora la capability `exports` resta
`pending`.

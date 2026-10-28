# P09.02 — Chiusura export JSON e JavaScript

Stato: **completata**. La capability `exports` è implementata; la parità è
verificata sulle fixture sintetiche Linux descritte qui, non su piattaforme o
servizi reali non provati.

## Ambito

`sanikey-core::exports::generate_exports` produce catalogo documenti, ricerca,
timeline, sommario, script JavaScript per `file://`, ricerca avanzata, payload
clinici, report tecnici, collegamenti AIFA/FI/RCP e manifest per viewer DICOM.
I dati di persone diverse restano isolati. Gli export Markdown usano
CommonMark tramite `pulldown-cmark 0.13.5`: HTML raw viene escapato, schemi URL
non sicuri sono resi inerti e gli URL `data:` ammessi corrispondono alla
allowlist della baseline.

Il manutentore ha precisato che la precedente richiesta di terminare P09.02 non
costituiva approvazione. Il 2026-10-28 ha approvato esplicitamente la proposta
G2 di usare `pulldown-cmark` per la parità CommonMark. Nello stesso giorno ha
approvato la distinta proposta sull'identità USB, registrata in
[ADR-0003](../../decisions/0003-identita-usb.md). Le approvazioni non qualificano
il servizio AIFA reale né gli adattatori USB multipiattaforma.

## Verifiche

- Baseline fissata `fdad1d93e56496cc6cb9933b3089729175d5b5b3`, test export
  eseguiti da estrazione isolata: `tests/test_exports.py` — 8/8. Il checkout
  Python di riferimento non è stato modificato.
- Rust focalizzato: `cargo test -p sanikey-core exports::tests --offline --
  --nocapture` — 10/10; `cargo fmt --all -- --check` — superato;
  `cargo clippy -p sanikey-core --all-targets --offline -- -D warnings` —
  superato.
- `cargo xtask compare frontend <baseline-web> <rust-web>` — exit 0, nessuna
  differenza significativa sulla fixture sintetica con Markdown, HTML raw,
  link sicuri e non sicuri, immagini `data:`, FI/RCP locali, URL AIFA, DICOM e
  dati clinici. Confronti separati per ricerca/timeline JSON, report CSV/HTML
  e manifest del viewer DICOM — nessuna differenza significativa.
- `cargo xtask check` — exit 0: 167 test core passati, 1 ignorato; 8 test xtask;
  3 test CLI; 2 doctest; pubblicabilità verificata su 252 file e rustdoc
  superato. L'ultima modifica di codice ha aggiunto asserzioni ai test esistenti;
  il gate completo viene ripetuto prima del commit di chiusura.
- Hook pre-commit completati dopo l'aggiornamento offline dei tre lockfile dei
  prototipi GUI, rimasti indietro rispetto alla nuova dipendenza core.
- `uv run codira index` — exit 0 (`Indexed: 0`, `Reused: 201`, `Failed: 0`;
  2241 embeddings riusati). `uv run codira cov` segnala solo `.user-words`,
  che non ha analyzer registrato; i file Rust sono coperti dall'analyzer v4.

## Tentativi falliti e correzioni

- Il primo test Cargo offline non trovava `pulldown-cmark-escape`; il download
  Cargo online ha reso disponibile la dipendenza.
- I confronti iniziali hanno evidenziato una differenza di newline attorno
  all'HTML raw, un newline finale nel sommario JSON e un diverso trattamento
  delle immagini `data:`. Sono stati corretti rispettivamente il rendering,
  la serializzazione e la allowlist delle immagini, poi i confronti sono stati
  ripetuti.
- La prima esecuzione del pre-commit ha rilevato lockfile GUI obsoleti; sono
  stati aggiornati offline e il hook è stato ripetuto con esito positivo.
- Codira aveva fallito inizialmente con `BrokenProcessPool` e SIGSEGV
  (`--concurrency thread`, exit 139). Dopo la correzione dell'analizzatore Rust
  comunicata dal manutentore, l'indicizzazione è terminata correttamente.

## Commit e limiti

Commit funzionali: `28b53ca`, `fe8f578`, `5d75ac8`, `69d8ba7`; documentazione
P09.02: `caec79a`.

Non è stata effettuata una richiesta al servizio AIFA reale. Browser, sistemi
Windows/macOS e frontend completo sono fuori dalla qualifica di questa scheda;
il frontend offline prosegue in P09.03.

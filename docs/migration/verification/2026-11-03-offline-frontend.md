# P09.03 — Frontend offline

## Ambiente e baseline

- Baseline Python: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.
- La generazione Python è stata eseguita sul checkout estratto a quello SHA;
  `tests/test_frontend.py`: 6 passed.
- Gli hash SHA-256 di HTML, CSS, JavaScript, logo, Chart.js e licenza vendorizzati
  sono fissati dal test Rust `vendored_frontend_assets_match_the_pinned_python_baseline`.
- Browser: Chromium 151.0.7922.34, headless, Linux.
- I dati browser sono fixture sintetiche create nel test e rimosse a fine prova.

## Verifiche

Test mirato del generatore:

```bash
cargo test -p sanikey-core frontend::tests -- --nocapture
```

Esito: 4 passed. Verifica `cargo fmt --all -- --check` superata.

Prova browser per le pagine generate da `file://`:

```bash
SANIKEY_CHROMIUM_BIN="<percorso-Chromium-headless>" \
SANIKEY_TEST_TEMP_DIR="<directory-temporanea>" \
cargo test -p sanikey-core \
  frontend::tests::chromium_opens_offline_pages_and_exercises_chart_flows \
  -- --ignored --nocapture
```

Esito: 1 passed. La prova apre la pagina principale e quella dei grafici estesi
senza server o rete e controlla ricerca, link al documento e al grafico esteso,
valori mancanti, asse separato del polso, periodo temporale senza punti,
filtri senza risultati e smontaggio del grafico sostituito.

Il primo avvio nel sandbox si è fermato per i permessi di Crashpad e del sandbox
Linux di Chromium, prima del caricamento della pagina. Eseguito il medesimo test
con il permesso esterno approvato automaticamente; l'esecuzione ha superato
entrambe le pagine in 4,24 secondi. Un successivo test di avvio su pagina `data:`
ha confermato Chromium headless operativo; la prova P09.03 usa un timeout di
processo di 20 secondi.

## Risultato e limiti

Il generatore Rust scrive pagine e asset locali, conserva la licenza MIT di
Chart.js e copia l'eventuale immagine di sfondo configurata. I dati esportati e
la ricerca indicizzata restano prodotti dai moduli `exports`; l'orchestrazione
incrementale appartiene alla scheda P09.04.

La qualifica browser riguarda esclusivamente Linux e la versione Chromium sopra
indicata. Windows, macOS, browser diversi e supporti USB fisici non sono stati
provati. La prima esecuzione di `cargo xtask check` ha segnalato che questa
ricevuta non esisteva ancora; la ricevuta è stata aggiunta prima di ripetere il
gate.

## Esito del gate e commit

`cargo xtask check`: superato. Esiti: Clippy e rustfmt senza errori; core
171 passed e 2 ignored; CLI 3 passed; xtask 8 passed; doctest 2 passed;
pubblicabilità 268 file; build workspace e smoke test CLI superati; rustdoc
generato senza warning bloccanti.

Il primo passaggio del gate, avviato prima che la ricevuta fosse creata, ha
fallito solo per il collegamento locale mancante; il gate completo ripetuto dopo
la creazione della ricevuta è passato.

Commit atomico P09.03: da registrare nel ledger dopo la creazione.

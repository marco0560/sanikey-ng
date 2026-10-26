# Ricevuta P02.02 — Corpus sintetico e expected baseline

Data: 2026-10-26. Baseline di generazione: Python
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

Il corpus in `corpus/` contiene 17 input sintetici versionati con formato e
SHA-256, due casi negativi e 11 generatori/test per adattatori complessi con
SHA del sorgente baseline. Le fixture minime derivano dagli esempi pubblici
dichiarati fittizi; i negativi sono file TOML/CSV sintetici creati per questa
scheda. La matrice conserva tutte le capability in stato `pending`.

## Run della baseline

Comandi, eseguiti nel checkout qualificato con la configurazione del corpus:

- `uv run --locked sanikey validate-config <accounts.toml>`: exit 0; un
  paziente sintetico abilitato.
- `uv run --locked sanikey import-observations patient-a --config
  <accounts.toml>`: exit 0; una serie e un punto.
- `uv run --locked sanikey build-patient patient-a --mode full --config
  <accounts.toml> --no-progress`: exit 0; due record, un duplicato, zero
  derivati e zero istanze DICOM. Gli avvisi normalizzati sono contenuto
  duplicato e mancata estrazione testuale del CSV.
- `uv run --locked sanikey validate-config
  <negative/config-missing-version.toml>`: exit 1; `[global].config_version`
  obbligatorio.
- `uv run --locked sanikey import-observations patient-a --config
  <negative-run/accounts.toml>`: exit 1; data osservazione
  `giorno-sconosciuto` non valida.

Il punto atteso è `70.0 kg`, data `2026-01-02`, origine
`laboratory/peso-sintetico.csv`. L’expected JSON registra anche conteggi
SQLite, summary frontend, record esportato e categorie di avviso. Timestamp e
percorsi assoluti sono omessi per mantenere il confronto portabile; il report
di build locale completo resta sotto `.artifacts/migration/p02-02-build/`.

## Tentativi corretti e limiti

I primi tentativi hanno usato la sintassi posizionale errata per
`import-observations` (exit 2), una base relativa non corretta (import senza
manifest, zero serie) e una destinazione di scrittura in sola lettura. Sono
stati corretti usando `--config`, la radice dei percorsi definita dal loader e
una copia di lavoro sotto `.artifacts`; i run riportati sopra sono quelli
accettati. Nessun file del campione Python è stato modificato.

Le fixture per OCR, Office, archivi, DICOM e USB sono generate dai test
baseline referenziati nel manifest e nel [catalogo adattatori](../corpus/complex/README.md);
P02.02 non dichiara test su supporti reali né qualifica provider di sistema.
Il gate completo del campione qualificato in P01.03 aveva 359 test superati e
1 skip; in P02.02 non è stato rilanciato. Sono stati verificati SHA fixture e
generatori, TOML, JSON, coerenza dell’expected, link Markdown e `git diff
--check`.

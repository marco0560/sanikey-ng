# Ricevuta P02.01 — Inventario del contratto e dei test

Data: 2026-10-26. Baseline: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

L’inventario identifica 20 capability della matrice, 14 gruppi CLI, sorgenti
baseline, test di riferimento, formati, dati persistenti, default, errori e
ADR accettate. Le 20 capability restano tutte `pending`: qui è stata
registrata la copertura del contratto, non una verifica Rust.

## Esplorazione e controlli

- `uv run --locked codira caps --json`: exit 0; MCP Codira locale non
  disponibile, quindi è stato usato il CLI del checkout baseline.
- `uv run --locked codira index --path <baseline> --output-dir <indice>
  --defer-embeddings --explain`: exit 0; 124 file indicizzati, 0 falliti.
  Restano 20 avvisi di copertura per formati/configurazioni non serviti dagli
  analyzer attivi. L’indice ha guidato la scoperta simboli Python; sorgenti,
  test e documenti sono stati letti direttamente.
- `codira sym main --json` ha individuato l’entrypoint in
  `src/sanikey/__main__.py`; `codira symlist` ha orientato la lettura dei
  moduli implementati. Non è stato usato Codira per dedurre copertura Rust.
- I controlli documentali hanno analizzato TOML, link Markdown locali e
  `git diff --check`; nessun test è stato aggiunto o eseguito in questa scheda
  documentale.

## Rilievi risolti

- L’indice delle decisioni elenca 12 ADR successive; una tredicesima ADR con
  stato `Accettata` sulla visibilità degli artefatti esiste nel repository
  baseline ma manca dall’indice. È inclusa nell’inventario.
- La maggior parte delle DA numerate in `decisioni-iniziali.md` non ha stato.
  Le decisioni sono considerate requisiti solo quando confermate dalle fonti
  autorevoli correnti o da ADR accettate; i conflitti storici DA-010 e DA-086
  sono descritti nell’inventario.
- Non è emersa una differenza materiale irrisolta tra contratto prodotto,
  implementazione baseline e ADR accettate. G2 non viene aperto.
- Mancano test baseline di dispatch per `extract-text`,
  `import-observations` e `build-parameter-slices`; i test dei moduli restano
  collegati e i test CLI vengono richiesti nel lavoro Rust pertinente.
- I test frontend statici non dimostrano un flusso browser completo da
  `file://`; la relativa qualifica resta futura.

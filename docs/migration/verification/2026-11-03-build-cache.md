# Ricevuta P09.04 — Build e cache incrementali

Data: 2026-11-03
Commit funzionale: `b88d3bc` (`feat(build): add incremental patient builds`)
Baseline Python: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`

## Modifica

`sanikey-core::build` espone `build_patient` e `build_all`, con modalità
incrementale, completa e validation. La pipeline valida le regole prima di
creare l'output, esegue il preflight privacy e metadati per tutte le persone
abilitate, quindi collega staging, DICOM, estrazione, osservazioni, database,
export, frontend, manifest, report e checksum.

La cache JSON versionata contiene solo estrazioni testuali e la loro identità
documentale; risiede in `local_build/cache` per persona. Modalità completa,
validation, OCR forzato, cache corrotta e identità sorgente cambiata causano una
nuova estrazione. Le build successive mantengono i risultati già completati,
così `build_all` può riprendere da una persona successiva senza riprocessare
testo invariato. Gli originali restano immutati.

## Verifiche

- Baseline: `python -B -m pytest -p no:cacheprovider tests/test_build.py -q` —
  **25 passed**.
- Rust mirato: `cargo test -p sanikey-core build::tests` — **6 passed**.
- Gate completo: `cargo xtask check` — **exit 0**; 177 test core passati, 2
  ignorati, 8 test xtask, 3 test CLI, 2 doctest; controlli publishability,
  rustdoc e build superati.
- Hook pre-commit del commit funzionale — formattazione, suite workspace e test
  dei tre prototipi GUI superati.
- Fixture sintetiche: riuso, ripetizione stabile, invalidazione dopo modifica,
  equivalenza SQLite full/incremental, cache corrotta, bypass cache in modalità
  validation/OCR forzato, duplicati, ordine multi-persona, prevalidazione e
  ripresa dopo errore parziale.

## Limiti

Le prove sono Linux e usano fixture sintetiche. Il bypass OCR è verificato,
senza eseguire un nuovo OCR PDF in questa scheda. Non è stata eseguita una
richiesta al servizio AIFA reale. I comandi CLI di build restano da collegare;
questa scheda aggiunge le API core. La qualifica Windows/macOS e la parità
semantica completa restano da dimostrare.

## Prossima scheda

[P10.01 — Implementare copia e aggiornamento portabili](../tasks/P10.01-implementare-copia-e-aggiornamento-portabili.md).

# Ricevuta P10.01 — Copia USB portabile

Data: 2026-11-03

Commit funzionale: `4e3127e` (`feat(core): mirror USB image directories portably`)

## Modifica

`sanikey-core::usb_copy::mirror_directory` sincronizza una directory immagine
esistente con API filesystem Rust, senza dipendere da rsync. Prima di scrivere
verifica i percorsi, rifiuta sorgente/destinazione sovrapposte, link simbolici,
file speciali e collisioni tra file e directory. I file invariati sono riusati
tramite SHA-256; i file diversi vengono copiati in un temporaneo e verificati
prima del rename. Gli obsoleti vengono rimossi solo dopo il completamento delle
copie. L'operazione emette eventi di progresso, supporta l'annullamento fra
file e può essere rieseguita dopo un errore.

La prova end-to-end usa `export_txt_simulation` come immagine già pronta,
aggiorna una directory di destinazione e verifica manifest, checksum, file e
byte uguali. Gli originali sintetici restano invariati.

## Verifiche

- Baseline fissata: `python -B -m pytest -p no:cacheprovider tests/test_usb.py -q`
  — **20 passed, 1 skipped**.
- Rust mirato: `cargo test -p sanikey-core usb_copy::tests` — **5 passed**.
- Clippy core: `cargo clippy -p sanikey-core --all-targets --locked -- -D warnings`
  — exit 0.
- Gate completo: `cargo xtask check` — **exit 0**; 182 test core passati, 2
  ignorati, 8 test xtask, 3 CLI, 2 doctest, publishability 273 file, rustdoc e
  build superati.
- Hook pre-commit — suite workspace, formattazione e test dei tre prototipi GUI
  superati.
- Fixture sintetiche: immagine con manifest valido, riuso senza riscrittura,
  rimozione obsoleti, collisione prima di modifiche alla destinazione,
  recupero dopo correzione, overlap, link simbolici, progresso e annullamento.

## Limiti

Le verifiche sono Linux. La funzione sincronizza una directory immagine già
preparata: non assembla ancora l'export multi-persona, non valida UUID,
filesystem o spazio disponibile dei volumi fisici e non completa la
validazione USB. Le schede P10.02–P10.05 mantengono questi passaggi pending.

## Prossima scheda

[P10.02 — Implementare adattatore Linux dei volumi](../tasks/P10.02-implementare-adattatore-linux-dei-volumi.md).

# P06.03 — Esportazione simulata TXT

Data: 2026-10-28. Sistema verificato: Linux.

## Perimetro

`sanikey-core::simulated_export::export_txt_simulation` crea una nuova
directory con indice principale, archivio della persona, database, JSON,
frontend e soli documenti `.txt`. Costruisce l'albero in staging, lo valida e
lo finalizza con rename; rifiuta destinazioni esistenti e sovrapposizioni con
directory sorgente o di build. Non esegue rilevamento USB, mount, rsync o
scrittura su supporti fisici.

Il manifest versione 1 elenca le persone e lo SHA-256 di ogni file esportato,
escludendo il manifest stesso. `exported_at` è `null` e `copy_strategy` vale
`simulated`, così due esportazioni sintetiche identiche producono byte
identici. La validazione verifica schema, file richiesti, completezza della
mappa checksum, hash, link alla pagina della persona e link documentali locali
decodificati e contenuti nell'archivio.

Il test esegue due esportazioni su destinazioni distinte e confronta i manifest
byte per byte; poi altera una copia e verifica che la validazione fallisca.
Controlla inoltre link presenti, rifiuto di una destinazione esistente e
immutabilità di tutti i file sorgente. L'esportazione resta un campione a una
persona e TXT: la capability USB completa rimane `pending`.

## Stabilità SQLite

La prima ripetizione ha mostrato che riusare il database precedente cambiava i
byte SQLite e quindi il checksum, a parità di righe. La pipeline ora costruisce
un file nuovo a fianco della destinazione e lo sostituisce solo dopo commit; se
la sostituzione fallisce tenta il ripristino del file precedente. Questo
mantiene stabili i checksum e non elimina il database valido prima che quello
nuovo sia completo.

## Verifiche

- `cargo test --offline -p sanikey-core`: exit 0, 66 test e 1 doctest superati.
- `cargo xtask check`: exit 0; Clippy, 3 test CLI, 66 test core, 8 test xtask,
  doctest, build, smoke CLI, controlli di pubblicabilità e rustdoc superati.
- `python3 scripts/check_publishable.py`: exit 0, 202 file pubblicabili;
  `git diff --check`: exit 0.
- Hook pre-commit: exit 0; test workspace, doctest e test dei tre prototipi GUI
  superati.
- Nessun dispositivo o volume fisico coinvolto.

Il primo test di ripetibilità ha fallito confrontando manifest differenti per
byte. La causa è stata il riuso del database SQLite; dopo il cambio a file
temporaneo e sostituzione recuperabile, entrambe le esportazioni hanno
manifest identici e il test completo passa.

La baseline consultata è il commit fissato
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`; il checkout Python corrente è
avanzato e non è stato usato come fonte.

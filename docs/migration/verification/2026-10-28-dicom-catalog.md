# Ricevuta P07.06 — Catalogazione DICOM

Data: 2026-10-28. Prove Linux con fixture DICOM sintetiche; nessun dato
sanitario o media clinico.

## Fonti e implementazione

Baseline: commit `fdad1d93e56496cc6cb9933b3089729175d5b5b3`, file
`src/sanikey/dicom.py` e `tests/test_dicom.py`. La suite baseline completa è
stata eseguita in un export temporaneo immutabile: `20 passed in 0.48s`.

Il nucleo Rust usa DICOM-rs `dicom-object 0.10.0` per leggere metadati fino a
Pixel Data e `dicom-core 0.10.0` per i tag. Il catalogo raggruppa per Study
Instance UID, genera lo stesso identificatore SHA-256 della baseline, legge
record `STUDY` da DICOMDIR, rimuove duplicati e associa istanze staged al
contenitore origine. Un file non leggibile resta nell'inventario con un avviso
non bloccante e la sua provenienza. La catalogazione non decodifica Pixel Data.

## Verifiche

| Verifica | Esito |
| --- | --- |
| Baseline `tests/test_dicom.py`, commit fissato | 20 passed |
| `cargo check -p sanikey-core --offline` | exit 0 dopo aver corretto il riferimento al trait DICOM-rs |
| `cargo test -p sanikey-core dicom::tests --offline` | 5 passed |
| `cargo clippy -p sanikey-core --all-targets --offline -- -D warnings` | exit 0; un primo passaggio ha rilevato una condizione di parità manuale nel fixture, corretta |
| `cargo xtask check` | exit 0: 101 test core, 1 ignorato, 8 xtask, 3 CLI, 2 doctest; build e smoke CLI; pubblicabilità 218 file; rustdoc |
| Hook pre-commit | superato: workspace e tre workspace GUI isolati; protocollo GUI 1 test, egui/Slint compilati e testati |
| Codira index | SIGSEGV, exit 139; nessun indice precedente riutilizzato |

I test Rust coprono UID uguali e distinti, date, record DICOMDIR duplicati,
coalescenza DICOMDIR/istanze, provenienza da staging, istanze senza UID e file
danneggiati. Gli ID dei test sono elencati in `parity.toml`. Il commit
funzionale è `418baa3`; i tre lockfile dei prototipi GUI sono stati aggiornati
offline per includere DICOM-rs nel grafo del core.

## Limiti

La capability completa resta `pending`: media DICOMDIR in output, viewer,
anteprime e verifica di parità completa sono nel perimetro di P07.07 o di
qualifica. Non sono stati decodificati pixel né provati Windows, macOS, media
reali o viewer esterni. Non è stato usato un indice Codira obsoleto.

Riferimenti: [API DICOM-rs 0.10.0](https://docs.rs/dicom-object/0.10.0/dicom_object/),
[sorgenti ufficiali DICOM-rs](https://github.com/Enet4/dicom-rs).

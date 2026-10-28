# Ricevuta P07.07 — Media, viewer e anteprime DICOM

Data: 2026-10-28. Prove Linux con istanze, media e pagine HTML sintetiche;
nessun dato sanitario o media clinico.

## Fonti e implementazione

Baseline: commit `fdad1d93e56496cc6cb9933b3089729175d5b5b3`,
`src/sanikey/dicom.py`, `tests/test_dicom.py` e ADR di compatibilità del PC di
consultazione. La suite baseline DICOM è passata: `20 passed in 0.44s`.

Il core copia le istanze inalterate e un DICOMDIR leggibile con riferimenti
IMAGE risolvibili, mantenendo i percorsi relativi del supporto. Conserva il
subtree IHE PDI viewer con asset relativi, esclude launcher eseguibili e
ignora pagine che richiedono risorse HTTP(S), `fetch()` o XMLHttpRequest. Se il
DICOMDIR è assente o i suoi riferimenti non si risolvono, mantiene le istanze
e registra l'anomalia; non genera un DICOMDIR sostitutivo.

Le anteprime usano `dicom-pixeldata 0.10.0` con i decoder nativi e il supporto
immagini, convertono il primo frame a JPEG in scala di grigi e mostrano una
pagina offline dichiarata non diagnostica. Le istanze originali restano
inalterate quando la decodifica non riesce. JPEG-LS e JPEG 2000 non sono
abilitati; nessuna anteprima attesta adeguatezza diagnostica.

## Verifiche

| Verifica | Esito |
| --- | --- |
| Baseline `tests/test_dicom.py`, commit fissato | 20 passed |
| Test DICOM Rust mirati | 11 passed; includono decode 2×2, errore senza Pixel Data, byte originali e riferimenti DICOMDIR |
| `cargo clippy -p sanikey-core --all-targets --offline -- -D warnings` | exit 0 |
| DICOMDIR sintetico da pydicom `FileSet` | 1 record immagine, file referenziato esistente e magic DICOM verificati |
| `cargo xtask check` | exit 0: 107 test core, 1 ignorato, 8 xtask, 3 CLI, 2 doctest; build e smoke CLI; pubblicabilità 221 file; rustdoc |
| `cargo clippy -p sanikey-core --all-targets --offline -- -D warnings` | exit 0 |
| `uv run codira index --defer-embeddings --concurrency thread --jobs 2` | SIGSEGV, exit 139; indice non utilizzato |
| Hook pre-commit | superato: workspace, protocollo GUI (1 test), egui e Slint |

La pagina HTML delle anteprime usa solo nomi relativi inclusi nel media locale.
I test verificano la priorità IHE, i filtri configurati, l'assenza di URL
remoti nel viewer selezionato e l'esclusione dei launcher nativi. Il DICOMDIR
sintetico pydicom, SHA registrato nella cartella fixture, è stato riaperto con
`FileSet`: un record IMAGE e il file referenziato risultano presenti.

## Limiti

La capability `dicom` resta `pending`: generazione DICOMDIR senza un indice
valido d'origine, integrazione con la pipeline frontend/export e qualifica
multipiattaforma restano aperte. Sono stati provati solo fixture sintetici Explicit VR Little Endian;
RLE/JPEG Baseline sono stati decodificati nella verifica di fattibilità
P04.03, mentre JPEG-LS, JPEG 2000, media reali, viewer e Windows/macOS non sono
qualificati. Le anteprime sono consultazione orientativa, non diagnostica.

Commit funzionale: `7d24597`. I tre lockfile GUI isolati sono stati aggiornati
offline per includere i decoder e l'encoder JPEG.

Riferimenti ufficiali: [DICOM-rs pixeldata 0.10.0](https://docs.rs/dicom-pixeldata/0.10.0/dicom_pixeldata/),
[DICOM-rs object 0.10.0](https://docs.rs/dicom-object/0.10.0/dicom_object/),
[image 0.25.9](https://docs.rs/image/0.25.9/image/).

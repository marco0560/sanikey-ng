# Ricevuta P07.05 — Container e staging

Data: 2026-10-28. Implementazione e prove locali su Linux x86_64 con fixture
interamente sintetiche. Nessun dato clinico o file della baseline è stato
modificato.

## Contratto portato

- Selezione degli archivi ZIP, 7z, RAR, TAR.XZ, IMG e ISO per suffisso, come
  nella baseline; il parser verifica i byte e genera un warning per contenuti
  malformati. Le estensioni sono confrontate senza distinzione maiuscole/minuscole.
- Estrazione ZIP e TAR.XZ via librerie Rust; il parser 7z Rust usa 7-Zip come
  fallback per metodi non supportati; RAR via UnRAR; IMG/ISO via 7-Zip e
  fallback bsdtar. ZIP conserva i metodi Stored, DEFLATE, BZIP2 e LZMA della
  baseline. RAR, fallback 7z e immagini scrivono ogni membro attraverso stdout
  in un percorso già validato, così i programmi esterni non decidono dove estrarre.
- Rifiuto di percorsi assoluti, prefissi drive Windows e componenti `..`, con
  slash e backslash trattati come separatori. TAR ignora directory e membri non
  regolari; i link archivio non diventano link filesystem.
- Lock esclusivo per directory di build, deduplicazione dei container per
  SHA-256, ricorsione, filtri include/exclude, documenti derivati e manifest JSON
  schema 1 con hash, dimensioni, mtime e provenienza di tutti i file regolari.
- Nessuna quota di membri o byte è stata aggiunta: la baseline non ne definisce.
  Il percorso IMG/ISO mantiene il timeout baseline di 300 secondi per la fase di
  estrazione; UnRAR non aveva un timeout baseline.

La baseline riconosce i container dall'estensione, non dal magic. La scheda
menziona il riconoscimento del contenuto: il port mantiene la selezione reale
della baseline e lascia al parser il controllo del formato. Non è stata
introdotta una diversa policy di discovery.

## Verifiche

La suite baseline completa `tests/test_containers.py`, eseguita da un export
isolato dello SHA fissato `fdad1d93e56496cc6cb9933b3089729175d5b5b3`, ha dato
20 passed.

```bash
cargo test -p sanikey-core containers::tests -- --nocapture
cargo clippy -p sanikey-core --all-targets -- -D warnings
cargo xtask check
```

`cargo xtask check`: exit 0; fmt, Clippy, 96 test core passati e uno ignorato
perché richiede Pandoc/LibreOffice, 8 test xtask, 3 test CLI, 1 doctest,
build/smoke CLI, 3 test di pubblicabilità, 215 file pubblicabili e rustdoc.

Esito mirato: 14 test Rust passati e Clippy senza warning. I test coprono ZIP
con tutti i metodi Python baseline, TAR.XZ con stream XZ concatenati, 7z, RAR
con UnRAR locale, ricorsione ZIP,
ricorsione nel percorso IMG/ISO, filtri case-insensitive, manifest vuoto e
pieno, container ripetuti, archivi corrotti e traversal ZIP. Per esercitare il
percorso esterno IMG/ISO la fixture è un 7z sintetico con suffisso `.iso`, non
un'immagine ISO valida; anche la fixture nested usa quel formato sintetico.
Questi test verificano confinamento e flusso di estrazione, non la compatibilità
con immagini disco reali.

Il primo pre-commit si è fermato perché i tre lockfile isolati e tracciati dei
prototipi GUI non includevano le nuove dipendenze del core. Sono stati
rigenerati offline eseguendo i tre test workspace senza `--locked` e committati
in `a8d287c`; il pre-commit successivo ha superato protocollo, egui e Slint.

Codira è installato e dichiara l'analizzatore Rust. `uv run codira index
--defer-embeddings --concurrency thread --jobs 2` è terminato con SIGSEGV/exit
139 senza output; il controllo indicizzato non è stato disponibile. L'indice
non è stato consultato né considerato fresco.

## Limiti e parità

La capability `containers` è `implemented`, non `verified`. IMG/ISO reali, la
fallback bsdtar su un'immagine valida, le varianti codec 7z/RAR, zip bomb e
filesystem di Windows/macOS non sono state qualificate. I metodi ZIP baseline
sono coperti da fixture Rust e da un archivio LZMA sintetico prodotto con
`zipfile` Python. La suite automatica salta i test
dipendenti da 7-Zip/UnRAR se il programma non è installato. La distribuzione
continua a richiedere questi strumenti esterni per RAR e immagini.

## Fonti tecniche

- ZIP `8.6.0`: [`ZipFile::enclosed_name`](https://docs.rs/zip/8.6.0/zip/read/struct.ZipFile.html#method.enclosed_name).
- 7z `0.24.0`: [`decompress_file_with_extract_fn`](https://docs.rs/sevenz-rust2/0.24.0/sevenz_rust2/fn.decompress_file_with_extract_fn.html), con percorso validato dal helper ad alto livello.
- TAR `0.4`: [API del crate](https://docs.rs/tar/0.4.44/tar/).
- XZ `0.1.7`: [`XzDecoder::new_multi_decoder`](https://docs.rs/xz2/0.1.7/xz2/read/struct.XzDecoder.html#method.new_multi_decoder); il binding usa liblzma.
- Baseline e licenze/confezionamento: [ricevuta P04.02](2026-10-26-office-container-feasibility.md).

Commit implementazione: `13ac794`; lockfile GUI: `a8d287c`.

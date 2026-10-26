# Ricevuta P04.02 — Office e container

Data: 2026-10-26. Prove locali su Gentoo Linux x86_64, solo con file
sintetici. I prototipi e i derivati sono temporanei in `.Temp/sanikey-p04-02/`;
nessuna dipendenza o fixture è stata aggiunta al repository.

## Contratto della baseline

Baseline: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`. Pandoc estrae testo da
DOCX, ODT, RTF e HTML/HTM; LibreOffice converte `.doc` in testo. I fogli
`.ods`, `.xls`, `.xlsb`, `.xlsm` e `.xlsx` passano dal lettore Calamine. Per la
consultazione, Pandoc crea PDF per i formati moderni e LibreOffice per `.doc`
e i fogli. La disponibilità dei convertitori è esterna, con warning in caso
di assenza o errore.

Lo staging supporta ZIP, 7z, RAR, TAR.XZ, IMG e ISO. ZIP usa la libreria
standard; 7z usa `py7zr`; RAR usa `rarfile` e il binario UnRAR; TAR.XZ usa
`tarfile`/`lzma`; IMG/ISO usano 7-Zip e poi `bsdtar`. Il codice valida i
percorsi dei membri prima di scrivere, filtra i file tecnici, registra hash,
dimensioni e provenienza nel manifest e processa ricorsivamente i container
annidati.

## Esiti reali su Office

| Formato | Estrazione/lettura sintetica | PDF consultabile | Evidenza e limite |
| --- | --- | --- | --- |
| DOCX | Pandoc ha restituito il testo inserito. | Pandoc e LibreOffice hanno generato PDF da cui PyMuPDF ha estratto il testo. | Una pagina semplice; non prova fedeltà di stili, immagini o tabelle complesse. |
| ODT | Pandoc ha restituito il testo inserito. | Pandoc e LibreOffice hanno generato PDF con testo leggibile. | Una pagina semplice. |
| RTF | Pandoc ha restituito il testo di un RTF sintetico valido. | Pandoc e LibreOffice hanno generato PDF consultabili. | Il primo fixture RTF era un frammento non standalone prodotto da Pandoc e LibreOffice ne ha mostrato i control word; sostituito con RTF completo `\\rtf1`, poi entrambe le conversioni sono riuscite. |
| HTML | Pandoc ha restituito il testo. | Pandoc e LibreOffice hanno generato PDF consultabili. | Provato `.html`; `.htm` non provato separatamente. |
| DOC legacy | LibreOffice ha convertito un DOC sintetico generato da DOCX in testo e PDF; l'estrazione contiene un BOM UTF-8 iniziale. | PDF consultabile con PyMuPDF. | La prova usa DOCX creato da LibreOffice, non un corpus legacy indipendente. |
| XLSX | Calamine Python e Rust hanno restituito intestazioni e celle (`Parameter`, `Weight`, `70 kg`). | LibreOffice ha prodotto PDF con intestazioni e valori. | Fixture semplice generato da CSV. |
| ODS | Calamine Python e Rust hanno restituito il testo della cella. | LibreOffice ha prodotto PDF con intestazioni e valori. | Fixture semplice generato da CSV. |
| XLS | Calamine Python e Rust hanno letto un XLS prodotto da LibreOffice; contenuto coerente con XLSX/ODS. | Non provato separatamente. | Una sola fixture di due righe. |
| XLSM | Non provato. | Non provato. | Macro e compatibilità cartella non qualificate. |
| XLSB | Non provato. | Non provato. | Nessuna fixture binaria disponibile. |

Pandoc locale 3.11 ha prodotto testo da DOCX/ODT/RTF/HTML e PDF da tutti e
quattro. LibreOffice 25.2.7.2 ha prodotto PDF da DOCX, ODT, RTF, HTML, XLSX,
ODS e DOC; PyMuPDF ha letto il testo dei derivati. Per RTF, il risultato
finale è stato confermato con fixture completa. L'estrazione e il rendering
sono prove distinte: un PDF leggibile non dimostra fedeltà completa al
documento Office originale.

## Esiti reali su container e isolamento

| Formato | Baseline Python | Candidato Rust temporaneo | Limite |
| --- | --- | --- | --- |
| ZIP | Inventario, estrazione, ricorsione e rifiuto di membro `../` coperti dai test sintetici. | `zip 8.6.0` ha estratto `report.md` e rifiutato `../escape.txt` tramite `enclosed_name()`. | Nessun test di zip bomb o limiti di quota. |
| 7z | Inventario, estrazione e staging sintetici riusciti. | `sevenz-rust2 0.24.0` ha estratto il file dal campione creato con 7-Zip. | Non provate varianti di codec, cifratura o archivio solid. |
| TAR.XZ | Inventario e staging sintetici riusciti. | Non prototipato in Rust. | Candidato Rust: crate `tar` più decoder LZMA Rust o dipendenza liblzma; manca una prova end-to-end Rust. |
| RAR | Campione RAR sintetico estratto da staging con `rarfile`/UnRAR; zero warning, membro e contenuto attesi. | Nessun crate Rust selezionato. | Il fixture è stato creato con RAR 7.23 trial e l'estrazione locale usa UnRAR 7.23; non è una qualifica di distribuzione. |
| IMG / ISO | Non provati con immagini valide. | Non prototipati. | La baseline invoca 7-Zip e ripiega su bsdtar; nessuna equivalenza o supporto specifico dichiarato. |

I test d'integrazione selezionati sulla baseline hanno dato `12 passed` in
0,68 s per DOCX, ODT, RTF, HTML, ODS, inventari ZIP/7z/TAR.XZ, staging 7z e
TAR.XZ, ricorsione ZIP e rifiuto del percorso ZIP non sicuro. Il test XLSX
separato ha dato `1 passed` in 0,11 s. Il test DOC legacy della baseline usa
un processo simulato; la prova sopra ha invece invocato LibreOffice reale.

Il gate completo è stato eseguito dopo le modifiche documentali:

```bash
cargo xtask check
```

Exit 0: 9 unit test, 1 doctest, fmt, Clippy, build, smoke test CLI,
documentazione e controlli di pubblicabilità superati.

Comando dei test baseline dalla directory del checkout Python fissato:

```bash
BASELINE_ROOT=<checkout isolato al commit fissato>
PYTHON_REPO_ROOT=<checkout SaniKey Python con venv di sviluppo esistente>
TEMP_ROOT=<area .Temp autorizzata>
cd "$BASELINE_ROOT"
PYTHONDONTWRITEBYTECODE=1 \
PYTHONPATH="$BASELINE_ROOT/src" TMPDIR="$TEMP_ROOT" \
"$PYTHON_REPO_ROOT/.venv/bin/pytest" -p no:cacheprovider -q \
tests/test_documents.py::test_extract_text_reads_docx_documents \
tests/test_documents.py::test_extract_text_reads_pandoc_documents \
tests/test_documents.py::test_extract_text_reads_odt_documents \
tests/test_documents.py::test_extract_text_reads_ods_spreadsheets \
tests/test_documents.py::test_extract_text_lists_zip_archive_contents \
tests/test_documents.py::test_extract_text_lists_7z_archive_contents \
tests/test_documents.py::test_extract_text_lists_tar_xz_archive_contents \
tests/test_containers.py::test_stage_container_documents_extracts_7z_archive \
tests/test_containers.py::test_stage_container_documents_extracts_tar_xz_archive \
tests/test_containers.py::test_stage_container_documents_warns_for_unsafe_zip_member \
tests/test_containers.py::test_stage_container_documents_recurses_into_nested_zip
```

Il test XLSX separato usa `tests/test_documents.py::test_extract_text_reads_xlsx_workbooks`.
La directory baseline è rimasta pulita.

## Opzioni, licenze e dimensioni misurate

| Opzione | Licenza e dipendenze | Misura locale / packaging |
| --- | --- | --- |
| Pandoc 3.11 | GPL-2.0-or-later; processo esterno statico. | Binario Linux x86_64: 163.653.488 byte. Il PDF predefinito richiede un motore LaTeX: il solo `pdflatex` locale è 2.108.728 byte; `/usr/share/texmf-dist` misura 2.990.918.542 byte e `/var/lib/texmf` 140.863.990 byte. La conversione PDF dipende anche dai pacchetti TeX/font richiesti dal documento. |
| LibreOffice 25.2.7.2 | MPL-2.0, con componenti di terze parti sotto licenze diverse. | Albero `/usr/lib64/libreoffice`: 409.897.968 byte; non include tutte le dipendenze del sistema. Non è un singolo eseguibile autonomo. |
| Calamine 0.36.1 | MIT, crate Rust puro per XLS/XLSB/XLSX/XLSM/ODS. | Nel prototipo usa archivi ZIP; nessuna libreria nativa esterna osservata con `ldd`. La lettura di XLSB/XLSM è documentata ma non provata qui. |
| ZIP 8.6.0 | MIT; `enclosed_name()` scarta percorsi che escono dalla directory. | Nessuna libreria nativa esterna osservata. È comunque necessario limitare dimensione espansa, numero di membri e scritture concorrenti nel porting. |
| sevenz-rust2 0.24.0 | Apache-2.0, implementazione Rust; il changelog documenta la correzione della path traversal nella serie 0.21.1. | Nessuna libreria nativa esterna osservata nel prototipo. Verificare i codec richiesti e mantenere un limite di risorse. |
| 7-Zip / bsdtar / UnRAR | 7-Zip: LGPL con eccezioni e restrizione UnRAR; bsdtar appartiene a libarchive; UnRAR ha EULA distinta e restrizioni sul formato RAR. | Binari locali: 7zz 2.689.648 byte, bsdtar 80.032, unrar 449.864. `ldd`: 6 righe per 7zz, 10 per bsdtar e 6 per UnRAR. Dimensioni binario non equivalgono alla somma delle librerie. |
| Prototipo Rust Calamine + ZIP + 7z | 57 crate Rust nel grafo normale; licenze transitive non ancora auditate. | Eseguibile release di prova Linux x86_64: 2.331.344 byte; linkage dinamico osservato solo verso libc, libgcc_s e loader. È una misura del driver, non del prodotto. |

Fonti ufficiali: [licenza Pandoc](https://github.com/jgm/pandoc/blob/main/COPYRIGHT),
[PDF e motori Pandoc](https://pandoc.org/MANUAL.html#creating-a-pdf),
[Calamine 0.36.1](https://docs.rs/calamine/0.36.1/calamine/),
[sevenz-rust2 e licenza](https://github.com/hasenbanck/sevenz-rust2),
[correzioni di sicurezza sevenz-rust2](https://github.com/hasenbanck/sevenz-rust2/blob/main/CHANGELOG.md),
[API ZIP 8.6.0 `enclosed_name`](https://docs.rs/zip/8.6.0/zip/read/struct.ZipFile.html#method.enclosed_name),
[licenze LibreOffice](https://www.libreoffice.org/licenses/),
[licenza 7-Zip](https://www.7-zip.org/),
[EULA RAR/UnRAR](https://www.rarlab.com/license.htm).

## Tentativi corretti e limiti

- LibreOffice nel sandbox ha terminato con exit 1 e dconf non è riuscito a
  scrivere sotto `/run/user/1000`. Isolare XDG e profilo non ha sbloccato
  l'esecuzione; `strace` è stato negato dal sandbox per `ptrace`. La stessa
  conversione, con profilo/cache/output in `.Temp` e fuori sandbox, ha
  restituito exit 0. È un limite dell'ambiente di prova, non una diagnosi di
  difetto LibreOffice.
- La creazione iniziale del DOC ha usato un filtro con virgolette letterali
  e ha fallito; `doc:MS Word 97` ha prodotto il DOC atteso.
- La prima fixture RTF era un frammento, non un documento standalone; una
  fixture completa ha risolto il problema.
- Il prototipo Rust non applica ancora filtri clinici, manifest, ricorsione,
  timeout, annullamento o quote di spazio del contratto. I test ZIP non
  coprono le zip bomb. Nessun test IMG/ISO, XLSB/XLSM o altro sistema operativo.

Non serve aprire G2: i formati contrattuali non sono stati dichiarati
irrealizzabili e non è stata proposta una deroga. La capability `office` e la
capability `containers` restano `pending` fino al porting e alla parità dei
comportamenti completi.

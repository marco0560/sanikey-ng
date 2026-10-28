# P07.04 — Conversioni Office consultabili

Data: 2026-10-28. Sviluppo e verifica su Gentoo Linux x86_64, con fixture
sintetiche. Baseline Python fissata a `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

## Matrice per formato

| Formato | Convertitore | Verifica Rust P07.04 | Limite |
| --- | --- | --- | --- |
| DOCX | Pandoc | Instradamento unitario; conversione reale in Pandoc verificata in P04.02. | La prova reale Rust di questa scheda converte il DOCX con LibreOffice, non con Pandoc. |
| ODT | Pandoc | Instradamento unitario. | PDF reale Pandoc verificato in P04.02; nessuna nuova fixture Rust. |
| RTF | Pandoc | Conversione reale a PDF, testo estratto e documento corrotto verificati. | Un solo documento sintetico semplice. |
| DOC | LibreOffice | Instradamento unitario; conversione reale baseline verificata in P04.02. | La prova Rust usa DOCX sintetico con il ramo LibreOffice. |
| ODS, XLSX | LibreOffice | Instradamento unitario. | PDF reale LibreOffice verificato in P04.02; nessuna nuova fixture Rust. |
| XLS | LibreOffice | Instradamento unitario. | PDF non verificato separatamente in P04.02. |
| XLSB, XLSM | LibreOffice | Instradamento unitario. | Formati e PDF non qualificati in P04.02 né in P07.04. |
| HTM, HTML | Link originale | Restano nel ramo consultabile diretto di P07.03. | Nessuna conversione PDF, come nella baseline renderer per sorgenti apribili. |

La baseline usa Pandoc per DOCX/ODT/RTF e LibreOffice per DOC e fogli di
calcolo. La conversione non dichiara equivalenza semantica: il PDF è un
derivato e il frontend espone anche l'originale. La ricerca usa il testo
estraibile dal PDF derivato; non sostituisce il contenuto e la struttura del
file Office originale.

## Implementazione

`sanikey-core::office` seleziona il convertitore dall'estensione, invoca il
processo con argomenti separati, isola il profilo temporaneo LibreOffice e
interrompe il processo dopo 120 secondi. Il risultato viene accettato solo se
`lopdf` apre il PDF e trova almeno una pagina. Stderr non viene propagato nel
JSON o nell'HTML. Assenza, errore, timeout e PDF malformato producono warning;
la pipeline conserva e linka l'originale e lascia `href` nullo se manca il
derivato.

Le opzioni CLI sono confermate dalla documentazione ufficiale
[Pandoc — creating a PDF](https://pandoc.org/MANUAL.html#creating-a-pdf) e
[LibreOffice — parametri di avvio](https://github.com/libreoffice/core/blob/master/sysui/desktop/man/libreoffice.1).
La generazione PDF Pandoc dipende dal motore PDF installato; non viene
confezionato dal repository. LibreOffice resta una dipendenza runtime ampia.

## Verifiche

- Test mirati `office::tests`: 6 passati e 1 ignored; test
  `consultation_pipeline_`: 3 passati, incluso il mantenimento byte-per-byte
  dell'originale con derivato e con convertitore assente.

```bash
CARGO_HOME=<cache Cargo in .Temp> \
CARGO_TARGET_DIR=<target Cargo in .Temp> \
CARGO_NET_OFFLINE=true \
cargo test --offline -p sanikey-core office::tests

CARGO_HOME=<cache Cargo in .Temp> \
CARGO_TARGET_DIR=<target Cargo in .Temp> \
CARGO_NET_OFFLINE=true \
cargo test --offline -p sanikey-core consultation_pipeline_
```

- Il test reale ignored `installed_converters_render_synthetic_office_text_to_pdf`
  è stato lanciato esplicitamente: 1 passato. Pandoc ha convertito RTF in PDF
  e DOCX; LibreOffice ha convertito il DOCX in PDF. `lopdf` ha estratto il
  testo sintetico atteso. Un DOCX corrotto ha generato warning, nessun PDF e
  byte originali invariati. File e profili erano sotto `.Temp`; il comando è
  stato eseguito fuori sandbox per il limite dconf documentato sotto.

```bash
mkdir -p <TEMP_ROOT>/sanikey-p07-04-tests
TMPDIR=<TEMP_ROOT>/sanikey-p07-04-tests \
CARGO_HOME=<cache Cargo in .Temp> \
CARGO_TARGET_DIR=<target Cargo in .Temp> \
CARGO_NET_OFFLINE=true \
cargo test --offline -p sanikey-core --lib -- \
  --ignored installed_converters_render_synthetic_office_text_to_pdf --nocapture
```

- Il test processo verifica il timeout con terminazione del figlio.
- Il tentativo LibreOffice nel sandbox ha dato exit 1 perché dconf non può
  scrivere in `/run/user/1000`; lo stesso test sintetico eseguito fuori
  sandbox, con profilo e file in `.Temp`, è terminato con exit 0.
- Codira `caps --json` ha restituito exit 0. L'aggiornamento dell'indice con
  `uv run codira index` è terminato con exit 2 (`BrokenProcessPool`); non sono
  state usate query su un indice stantio.
```bash
CARGO_HOME=<cache Cargo in .Temp> \
CARGO_TARGET_DIR=<target Cargo in .Temp> \
CARGO_NET_OFFLINE=true \
cargo xtask check
```

Esito gate: exit 0; fmt, Clippy, 3 test CLI, 82 test core, 8 test xtask,
1 doctest, build, smoke CLI, 3 controlli di pubblicabilità, 211 file
pubblicabili e rustdoc superati. Il test reale esterno è intenzionalmente
ignored nel gate ordinario ed è stato eseguito separatamente fuori sandbox.
Hook pre-commit e commit-msg superati; 83 test core raccolti, 8 xtask, 3 CLI,
1 doctest, 1 test protocollo GUI, formattazione e test egui/Slint (0 test
unitari ciascuno). Commit implementativo: `e2e9586`.

## Parità e limiti

La capability `office` è implementata ma la parità resta pending. XLSB/XLSM,
fedeltà di layout complessi, distribuzione dei convertitori, Windows/macOS e
prove browser non sono qualificati. La capability di estrazione strutturata
dei fogli di calcolo resta distinta dalla conversione consultabile di questa
scheda.

# Inventario del contratto osservabile

Baseline: commit `fdad1d93e56496cc6cb9933b3089729175d5b5b3`, secondo
[baseline.toml](baseline.toml). Questo inventario identifica il comportamento
presente nel codice e i test di riferimento; non prova parità Rust. Le 20
capacità omonime in [parity.toml](parity.toml) sono gli identificativi stabili
per la verifica di sottosistemi.

## Superficie CLI

`src/sanikey/cli.py:build_parser` registra i comandi e il test d’integrazione
CLI è `tests/test_cli.py`; le prove versione sono in `tests/test_version.py`.
I comandi e il loro contratto primario sono:

| ID | Comando | Contratto / verifica mirata |
| --- | --- | --- |
| CLI-01 | `info`, `--version` | Identità macchina e versione installata; `test_info_subcommand_runs`, `test_short_version_flag_runs` |
| CLI-02 | `validate-config` | Configurazione, metadati e privacy; `test_validate_config_subcommand_runs`, `test_validate_config_rejects_invalid_metadata` |
| CLI-03 | `list-patients` | Elenco degli abilitati, con `--all` per includere i disabilitati; `test_list_patients_subcommand_runs` |
| CLI-04 | `scan-documents` | Inventario, avvisi, preflight, staging opzionale, output testo/CSV; `test_scan_documents_subcommand_runs` e test `test_scan_documents_*` |
| CLI-05 | `document-integrity` | Snapshot `before`, `after`, `check`; modifica delle fonti fallisce il controllo; `test_document_integrity_*` |
| CLI-06 | `extract-text` | Estrazione e avvisi per paziente; test funzionali `tests/test_documents.py`. Nel campione non è presente un test d’invocazione CLI dedicato. |
| CLI-07 | `import-observations` | Import tabellare esplicito, facoltativo per paziente; `tests/test_observation_imports.py` |
| CLI-08 | `discover-parameters`, `build-parameter-slices` | Proposte da cache estratta e slice curate; `tests/test_parameter_slices.py`, `tests/test_parameter_rules.py` |
| CLI-09 | `process-dicom` | Catalogo e staging opzionale; `tests/test_dicom.py` e test `test_process_dicom_*` |
| CLI-10 | `build-database` | SQLite per paziente; `test_build_database_subcommand_runs`, `tests/test_database.py` |
| CLI-11 | `build-patient`, `build-all`, `update-archive` | Build singola, multipaziente o incrementale; `test_build_patient_subcommand_runs`, `test_direct_runner_error_branches_and_update_archive`, `tests/test_build.py` |
| CLI-12 | `resolve-medication-leaflets` | Ricerca e conferma AIFA, scelta operatore nei casi ambigui; `tests/test_leaflets.py` e test `test_resolve_medication_leaflets_*` |
| CLI-13 | `generate-exports`, `build-web` | JSON statico e frontend; `tests/test_exports.py`, `tests/test_frontend.py` e test CLI dedicati |
| CLI-14 | `export-usb`, `validate-usb`, `deploy-usb` | Copia, verifica e flusso combinato USB; `tests/test_usb.py` e test CLI dedicati |

Per i comandi configurabili, il default è `config/accounts.toml` relativo alla
directory corrente e `--repo-root` usa la directory corrente. La configurazione
si passa come argomento posizionale; `--config` è esposto quando un argomento
posizionale lo renderebbe ambiguo. `build-patient` e `build-all` usano
`incremental` per default e accettano `full` e `validation`. La progressione
interattiva può essere disattivata con `--no-progress`; scansione e catalogo
DICOM mettono in staging i container per default. I flag di errore del parser
sono localizzati in italiano; gli errori applicativi `SaniKeyError` producono
exit 1, quelli di sintassi argparse exit 2. `tests/test_cli.py` verifica output,
flag non validi e diagnostica italiana. Non emerge un contratto CLI di
annullamento esplicito: non va inventato nell’implementazione.

I test del campione verificano i moduli di estrazione e osservazioni e il
percorso cache di `discover-parameters`, ma non si è trovato un test diretto
di dispatch per `extract-text`, `import-observations` o
`build-parameter-slices`. P02 documenta il comportamento dei moduli; questi
tre collegamenti CLI restano casi da aggiungere al corpus/test Rust durante la
relativa implementazione.

## Requisiti, sorgenti e prove di parità

Ogni riga identifica un requisito da preservare, il codice che oggi lo
realizza e il test da usare per il confronto. Le verifiche Rust restano vuote
finché non esistono implementazione e prove Rust.

| ID | Funzione obbligatoria | Sorgente baseline | Verifica baseline |
| --- | --- | --- | --- |
| configuration | Configurazione TOML, precedenze globali/persona, percorsi relativi, default e rifiuto di valori o chiavi ignoti | `src/sanikey/config.py` | `tests/test_config.py` |
| metadata | Metadati curati, riferimenti, identificativi e convalida tra entità | `src/sanikey/metadata.py`, `src/sanikey/models.py` | `tests/test_metadata.py` |
| privacy | Separazione dati privati, configurazione locale e vincoli sulle fonti | `src/sanikey/privacy.py`, `scripts/privacy_guard.py` | `tests/test_config.py`, `tests/test_validation.py`; `scripts/privacy_guard.py` |
| inventory | Inventario deterministico, esclusioni/inclusioni, duplicati, classificazione e provenienza | `src/sanikey/documents.py`, `src/sanikey/inspection.py` | `tests/test_documents.py`, `tests/test_inspection.py` |
| pdf | Estrazione PDF digitale, classificazione scansione, OCRmyPDF, fallback e avvisi | `src/sanikey/documents.py` | `tests/test_documents.py` |
| office | Estrazione e rendering consultabile dei formati Office supportati, con errori espliciti | `src/sanikey/documents.py`, `src/sanikey/rendering.py` | `tests/test_documents.py`, `tests/test_rendering.py` |
| containers | Ispezione/staging confinato, manifest, filtri, ricorsione e contenitori non sicuri/danneggiati | `src/sanikey/containers.py` | `tests/test_containers.py` |
| dicom | Catalogazione studi e supporti, viewer compatibili, copie e anteprime non diagnostiche | `src/sanikey/dicom.py` | `tests/test_dicom.py` |
| observations | Import da manifesto esplicito, normalizzazione e invalidazione se cambiano fonti | `src/sanikey/observation_imports.py` | `tests/test_observation_imports.py` |
| parameters | Candidati e slice deterministiche con unità, provenance, esclusioni e rifiuti | `src/sanikey/parameter_slices.py`, `src/sanikey/parameter_rules.py`, `src/sanikey/parameter_reports.py` | `tests/test_parameter_slices.py`, `tests/test_parameter_rules.py`, `tests/test_parameter_reports.py` |
| vital_signs | Merge e preservazione di valori, unità, date e provenienza dei segni vitali | `src/sanikey/vital_signs.py` | `tests/test_vital_signs.py` |
| aifa | Ricerca, conferma, esclusione esplicita e download dei fogli AIFA | `src/sanikey/leaflets.py`, `src/sanikey/leaflet_tui.py` | `tests/test_leaflets.py`, `tests/test_cli.py` |
| sqlite | Schema per paziente, vincoli, righe e ricerca FTS5 | `src/sanikey/database.py` | `tests/test_database.py` |
| exports | Payload JSON/JavaScript, riferimenti relativi e dati clinici ricercabili | `src/sanikey/exports.py` | `tests/test_exports.py` |
| frontend | Consultazione statica locale, ricerca, timeline, link e rendering offline | `src/sanikey/frontend.py`, `src/sanikey/assets/` | `tests/test_frontend.py`, `tests/test_exports.py` |
| build | Pipeline completa/incrementale, cache, report e isolamento dei pazienti | `src/sanikey/build.py` | `tests/test_build.py`, `tests/test_cli.py` |
| usb | Layout, mirror, hash, spazio/volume e rifiuto di export incompleti | `src/sanikey/usb.py` | `tests/test_usb.py` |
| cli | Comandi, exit status, default, parsing e messaggi operativi italiani | `src/sanikey/cli.py` | `tests/test_cli.py`, `tests/test_version.py` |
| desktop | GUI del generatore Rust | Nessuna sorgente baseline equivalente | Prove di usabilità/accessibilità previste in P11.04 |
| packaging | Pacchetti Rust installabili per i tre sistemi operativi | Nessuna sorgente baseline equivalente | Build e prove d’installazione previste in P12 |

Le funzioni trasversali `integrity`, `progress`, `errors` e `markdown` sono
esercitate da `tests/test_integrity.py`, `tests/test_progress.py`, i test
CLI/configurazione e `tests/test_markdown.py`; accompagnano le capacità sopra
e non costituiscono nuovi requisiti autonomi nel contratto prodotto.

## Formati, stato persistente e regole operative

- `config/accounts.toml` dichiara versione, persone, percorsi sorgente,
  metadati e build locale; globali e override persona sono validati. La
  configurazione privata e i dati restano locali.
- I metadati curati sono TOML: problemi, farmaci, fogli illustrativi, terapie,
  procedure, osservazioni, tag documentali, riepilogo clinico e timeline.
  `observation_imports.toml` associa esplicitamente CSV e fogli Excel/ODS; le
  serie normalizzate e il relativo `import_state.toml` persistono sotto
  `metadata/observations/`. `parameters.toml` abilita le regole longitudinali.
- La baseline classifica `.txt` e `.md`; `.pdf`; immagini `.jpg`, `.jpeg`,
  `.png`; Office `.docx`, `.htm`, `.html`, `.odt`, `.rtf`, `.doc`; fogli
  `.ods`, `.xls`, `.xlsb`, `.xlsm`, `.xlsx`; archivi `.zip`, `.7z`, `.rar`,
  `.tar.xz`; supporti `.dcm`, `.img`, `.iso`. I PDF possono usare OCR;
  `.jpg`, `.jpeg` e `.png` non ricevono OCR diretto. Le sorgenti DICOM restano
  conservate; le anteprime non sono diagnostiche. La capacità di estrazione,
  rendering o staging è specifica del formato e gli errori/strumenti mancanti
  generano avvisi o fallimenti secondo i test per formato.
- Gli artefatti rigenerabili includono cache di estrazione JSON, manifest e
  report JSON, SQLite per paziente, export JSON/JavaScript e frontend statico.
  La destinazione USB include `SANIKEY-MANIFEST.json` con hash verificabili.
  Gli originali sono fonte autorevole e non vengono modificati dalla build.
- I collegamenti esportati sono relativi e il frontend deve funzionare da
  `file://`, senza servizio locale o rete. Per la chiavetta la copia preferisce
  rsync quando disponibile, verifica integrità e destinazione, e non formatta
  supporti.

La tabella capability di `parity.toml` copre questi requisiti raggruppati; il
[corpus sintetico](corpus/README.md) fissa input e expected iniziali per date,
unità, duplicati, provenienza ed errori.
Gli artefatti d’esempio presenti nei test non attestano prove su dati reali,
supporti USB fisici o sistemi Windows/macOS.
I test del frontend verificano gli artefatti e i riferimenti generati; non
costituiscono una prova browser completa da `file://`.

## Decisioni accettate e coerenza

Le 12 ADR successive elencate in `docs/decisions/index.md` hanno stato
esplicito `Accettata`; anche `adr-consultation-artifact-visibility.md` nella
directory baseline è marcata `Accettata`, ma manca dall’indice. Le decisioni
precisano il contratto in questi punti:

- [Frontend offline](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/decisions/adr-frontend-consultation-ui.md), [ricerca avanzata](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/decisions/adr-advanced-offline-search.md), [federazione clinica](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/decisions/adr-federated-clinical-search-ui.md), [visibilità](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/decisions/adr-consultation-artifact-visibility.md) e [compatibilità del computer ospite](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/decisions/adr-consultation-host-compatibility.md) richiedono una consultazione locale da `file://`, con asset statici, ricerca clinica e documenti apribili.
- [Import osservazioni](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/decisions/adr-observation-imports.md) e [slice parametri](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/decisions/adr-longitudinal-parameter-slices.md) prescrivono mapping espliciti, provenienza e nessuna inferenza clinica automatica.
- [Pattern d’ingestione](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/decisions/adr-explicit-ingestion-patterns.md) e [policy OCR immagini](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/decisions/adr-source-image-ocr-policy.md) fissano priorità dei filtri e separano OCR PDF da immagini sorgente.
- [Hardening USB](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/decisions/adr-usb-consultation-hardening.md) e [UUID globale](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/decisions/adr-usb-uuid-global-default.md) richiedono controlli target, copia verificata e UUID globale autorevole.
- [Fogli AIFA](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/decisions/adr-medication-leaflets.md) richiede conferma operatore nei casi ambigui, copie locali e blocco dell’export quando manca un documento richiesto.
- [Lingua italiana](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/decisions/adr-italian-interface-language.md) mantiene in italiano messaggi, help e superfici rivolte alle persone, lasciando in inglese chiavi e nomi macchina.

`decisioni-iniziali.md` elenca DA-001–DA-159, ma la maggior parte delle righe
ha il campo Status vuoto. Le decisioni con collegamento a un’ADR successiva
fanno fede tramite quella ADR; le altre valgono solo se confermate dal
contratto prodotto o da codice e test correnti. Per esempio, DA-086 sul viewer
del supporto originale è superata dalla decisione accettata sulla
compatibilità del computer ospite: si conservano i DICOM, si usa un viewer
HTML statico compatibile quando disponibile e si producono anteprime dichiarate
non diagnostiche. Le destinazioni USB sono argomenti CLI e non il percorso
fisso menzionato in DA-010. Sono riferimenti storici, non requisiti da
reintrodurre. L’indice omette l’ADR accettata sulla visibilità degli artefatti;
la si include qui perché attinente ai requisiti di export e consultazione.
Queste differenze sono risolte dalle fonti autorevoli definite dal contratto
prodotto e non richiedono modifica materiale né apertura di G2.

Non emerge una differenza materiale tra il contratto prodotto, il codice e le
ADR attive. La federazione descritta dall’ADR riguarda dati e sezioni locali
nel frontend statico: non introduce cloud o sincronizzazione tra repository.
Le funzioni nominate in documenti storici ma escluse dal contratto (AI clinica,
FHIR/HL7, mobile/PWA, viewer DICOM generale e formattazione USB) restano fuori
dal perimetro. GUI desktop e packaging multipiattaforma appartengono al piano
Rust, non sono capacità da dichiarare già presenti nel campione.

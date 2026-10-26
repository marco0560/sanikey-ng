# Schede eseguibili

Esecutore previsto: GPT-6-Luna High. Le schede non duplicano lo stato del
[ledger](../ledger.md). Seguire i prerequisiti di ogni scheda: la GUI di
prodotto richiede G1, mentre le attività indipendenti del core possono
proseguire. Una scheda troppo ampia va suddivisa in sottoschede nel ledger
prima di modificarne il codice, preservando contratto e criteri.

I prerequisiti prevalgono sull'ordine numerico: P03 costruisce il workspace
prima di P02.03, che introduce il confronto nel tooling Rust.

## Ordine iniziale

L'utente ha richiesto P01.01 prima dell'implementazione del piano.
Il commit preliminare è già stato eseguito: non ripeterlo.
La preparazione termina dopo P00 e la registrazione P01.02;
la sessione Luna riparte da P01.03.

## Indice

| Scheda | Attività | Dipendenze |
| --- | --- | --- |
| [P00.01](P00.01-verificare-destinazioni-e-istruzioni.md) | Verificare destinazioni e istruzioni | P01.01 |
| [P00.02](P00.02-versionare-il-piano-e-i-documenti-iniziali.md) | Versionare il piano e i documenti iniziali | P00.01 |
| [P00.03](P00.03-creare-e-pubblicare-sanikey-ng.md) | Creare e pubblicare sanikey-ng | P00.02 |
| [P01.01](P01.01-committare-preliminarmente-tutte-le-modifiche-python.md) | Committare preliminarmente tutte le modifiche Python | Autorizzazione iniziale |
| [P01.02](P01.02-registrare-identita-ed-evidenze-della-baseline.md) | Registrare identità ed evidenze della baseline | P01.01 |
| [P01.03](P01.03-qualificare-un-checkout-isolato-del-campione.md) | Qualificare un checkout isolato del campione | P01.02 |
| [P02.01](P02.01-inventariare-i-contratti-e-i-test-esistenti.md) | Inventariare i contratti e i test esistenti | P01.03 |
| [P02.02](P02.02-preparare-il-corpus-sintetico-iniziale.md) | Preparare il corpus sintetico iniziale | P02.01 |
| [P02.03](P02.03-costruire-il-confronto-e-provarne-la-sensibilita.md) | Costruire il confronto e provarne la sensibilità | P02.02,P03.03 |
| [P03.01](P03.01-creare-workspace-e-toolchain-rust.md) | Creare workspace e toolchain Rust | P00.03 |
| [P03.02](P03.02-definire-errori-ed-eventi-delle-operazioni.md) | Definire errori ed eventi delle operazioni | P03.01 |
| [P03.03](P03.03-implementare-il-gate-e-la-ci-multipiattaforma.md) | Implementare il gate e la CI multipiattaforma | P03.02 |
| [P03.04](P03.04-provare-presto-il-pacchetto-minimo.md) | Provare presto il pacchetto minimo | P03.03 |
| [P04.01](P04.01-verificare-fattibilita-pdf-e-ocr.md) | Verificare fattibilità PDF e OCR | P03.04 |
| [P04.02](P04.02-verificare-conversioni-office-e-container.md) | Verificare conversioni Office e container | P03.04 |
| [P04.03](P04.03-verificare-dicom-e-codec.md) | Verificare DICOM e codec | P03.04 |
| [P04.04](P04.04-verificare-identificazione-dei-volumi.md) | Verificare identificazione dei volumi | P03.04 |
| [P04.05](P04.05-selezionare-due-toolkit-per-il-confronto.md) | Selezionare due toolkit per il confronto | P03.04 |
| [P04.06](P04.06-realizzare-i-prototipi-e-presentare-g1.md) | Realizzare i prototipi e presentare G1 | P04.05 |
| [P05.01](P05.01-portare-configurazione-e-validazione.md) | Portare configurazione e validazione | P02.03,P03.03 |
| [P05.02](P05.02-portare-metadati-e-isolamento-dei-dati.md) | Portare metadati e isolamento dei dati | P05.01 |
| [P05.03](P05.03-gestire-profili-e-importazione-dei-percorsi-storici.md) | Gestire profili e importazione dei percorsi storici | P05.02 |
| [P06.01](P06.01-portare-inventario-e-digest.md) | Portare inventario e digest | P05.03 |
| [P06.02](P06.02-completare-il-nucleo-txt-fino-al-frontend.md) | Completare il nucleo TXT fino al frontend | P06.01 |
| [P06.03](P06.03-esportare-e-validare-il-sottoinsieme-simulato.md) | Esportare e validare il sottoinsieme simulato | P06.02 |
| [P07.01](P07.01-portare-il-pdf-digitale.md) | Portare il PDF digitale | P06.03,P04.01 |
| [P07.02](P07.02-portare-l-ocr-pdf.md) | Portare l'OCR PDF | P07.01 |
| [P07.03](P07.03-portare-immagini-e-rappresentazioni-semplici.md) | Portare immagini e rappresentazioni semplici | P06.03 |
| [P07.04](P07.04-portare-conversioni-office.md) | Portare conversioni Office | P06.03,P04.02 |
| [P07.05](P07.05-portare-container-e-staging.md) | Portare container e staging | P06.03,P04.02 |
| [P07.06](P07.06-portare-catalogazione-dicom.md) | Portare catalogazione DICOM | P07.05,P04.03 |
| [P07.07](P07.07-portare-media-viewer-e-anteprime-dicom.md) | Portare media, viewer e anteprime DICOM | P07.06 |
| [P08.01](P08.01-portare-importazioni-di-osservazioni.md) | Portare importazioni di osservazioni | P05.02,P04.02 |
| [P08.02](P08.02-portare-parametri-slice-e-segni-vitali.md) | Portare parametri, slice e segni vitali | P07.01,P08.01 |
| [P08.03](P08.03-portare-riferimenti-e-fogli-aifa.md) | Portare riferimenti e fogli AIFA | P05.02 |
| [P09.01](P09.01-completare-sqlite-e-fts5.md) | Completare SQLite e FTS5 | P06.02,P08.01,P08.02,P08.03,P07.07 |
| [P09.02](P09.02-completare-export-json-e-javascript.md) | Completare export JSON e JavaScript | P09.01 |
| [P09.03](P09.03-completare-il-frontend-offline.md) | Completare il frontend offline | P09.02 |
| [P09.04](P09.04-completare-build-e-cache-incrementale.md) | Completare build e cache incrementale | P09.03 |
| [P10.01](P10.01-implementare-copia-e-aggiornamento-portabili.md) | Implementare copia e aggiornamento portabili | P09.04 |
| [P10.02](P10.02-implementare-adattatore-linux-dei-volumi.md) | Implementare adattatore Linux dei volumi | P10.01,P04.04 |
| [P10.03](P10.03-implementare-adattatore-windows-dei-volumi.md) | Implementare adattatore Windows dei volumi | P10.01,P04.04 |
| [P10.04](P10.04-implementare-adattatore-macos-dei-volumi.md) | Implementare adattatore macOS dei volumi | P10.01,P04.04 |
| [P10.05](P10.05-completare-validazione-e-recupero-usb.md) | Completare validazione e recupero USB | P10.02,P10.03,P10.04 |
| [P11.01](P11.01-creare-la-gui-e-i-profili.md) | Creare la GUI e i profili | G1,P05.03,P03.02 |
| [P11.02](P11.02-collegare-controlli-e-generazione.md) | Collegare controlli e generazione | P11.01,P09.04 |
| [P11.03](P11.03-collegare-export-e-risultato.md) | Collegare export e risultato | P11.02,P10.05 |
| [P11.04](P11.04-qualificare-usabilita-e-accessibilita.md) | Qualificare usabilità e accessibilità | P11.03 |
| [P12.01](P12.01-costruire-i-pacchetti-completi.md) | Costruire i pacchetti completi | P11.04 |
| [P12.02](P12.02-provare-installazione-e-aggiornamento-su-sistemi-puliti.md) | Provare installazione e aggiornamento su sistemi puliti | P12.01 |
| [P12.03](P12.03-preparare-una-prerelease-verificata.md) | Preparare una prerelease verificata | P12.02 |
| [P13.01](P13.01-presentare-la-transizione-operativa.md) | Presentare la transizione operativa | P12.03 |
| [P13.02](P13.02-presentare-il-phase-out-python.md) | Presentare il phase-out Python | P13.01,G3 |

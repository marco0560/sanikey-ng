# Piano approvato di SaniKey NG

## Approvazione e modifica P01

Il manutentore ha approvato repository locale e pubblico, baseline verificata
comprensiva delle modifiche correnti e confronto dei toolkit prima di G1.
Ha poi approvato il piano con questa modifica:

> Preliminarmente all'implementazione del piano viene effettuato un commit
> dei file correntemente modificati su sanikey, e il commit di riferimento
> sarà quello che include tutte queste modifiche.

Pertanto P01.01 precede P00 ed è già conclusa col commit
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`.
P01 non crea un altro snapshot né sostituisce il riferimento con l'ultimo
commit upstream. P01.02 registra l'identità; P01.03 ne completa la
qualificazione da checkout isolato.

L'utente aprirà una sessione Luna High nel nuovo repository. La sessione
preparatoria deve completare bootstrap e documentazione, avvertire il
manutentore e fermarsi prima dell'implementazione Rust.

## Decisioni fissate

| Aspetto | Decisione |
| --- | --- |
| Nome | sanikey-ng |
| Proprietario GitHub | marco0560 |
| Repository | Separato, locale e pubblico |
| Implementazione | Rust |
| Esecutore previsto | GPT-6-Luna High |
| Piattaforme | Windows, Linux e macOS; pacchetti separati |
| Baseline | Commit preliminare con tutti i dodici file modificati |
| GUI | Due prototipi confrontabili e approvazione G1 |
| Consultazione USB | Frontend statico offline, indipendente dall'app generatrice |
| Phase-out | Dopo qualifica e approvazioni G3 e G4 |

La posizione locale concordata è nella directory Software/Rust/sanikey-ng
del manutentore. I percorsi personali assoluti non sono versionati.
La nuova sessione deve confermare il proprio cwd prima di ogni attività.

## Metodo per Luna

Eseguire una scheda per volta con fonti, ambito, verifiche e arresti espliciti.
Non tradurre meccanicamente i moduli Python. Usare strutture Rust idiomatiche
che preservano il contratto osservabile e documentare le scelte reali.
Ogni scheda conclusa aggiorna ledger, parità, documentazione e commit.
Non dichiarare una verifica eseguita se manca il sistema o l'hardware.

Conservare sorgenti, decisioni, corpus ed evidenze necessarie in collocazioni
durevoli. Usare directory temporanee solo per materiale sacrificabile.
I test pubblici e la CI usano esclusivamente dati sintetici.

## Checkpoint

- G1: il manutentore sceglie il toolkit dopo il confronto P04.
- G2: il manutentore risolve differenze che cambiano requisiti, compatibilità
  o accettabilità delle dipendenze. Nessuna riduzione implicita.
- G3: il manutentore approva Rust come generatore operativo principale.
- G4: il manutentore approva la dismissione operativa del Python.

Pubblicare sanikey-ng non autorizza push o release del repository Python.
La pubblicazione di una ref del campione, se necessaria alla CI, richiede
un'indicazione specifica. La preparazione locale del corpus può usare il
commit locale senza fingere disponibilità remota.

## Sequenza e criteri

## P00 — Repository e documentazione iniziale

- [P00.01 — Verificare destinazioni e istruzioni](tasks/P00.01-verificare-destinazioni-e-istruzioni.md).
  Accettazione: Destinazioni controllate; account GitHub marco0560 verificato; istruzioni applicabili lette.

- [P00.02 — Versionare il piano e i documenti iniziali](tasks/P00.02-versionare-il-piano-e-i-documenti-iniziali.md).
  Accettazione: Tutti i link locali e i file TOML sono coerenti; nessun requisito futuro è dichiarato implementato.

- [P00.03 — Creare e pubblicare sanikey-ng](tasks/P00.03-creare-e-pubblicare-sanikey-ng.md).
  Accettazione: Il clone pubblico espone piano e schede; working tree pulito e remoto allineato.

## P01 — Baseline Python già committata

- [P01.01 — Committare preliminarmente tutte le modifiche Python](tasks/P01.01-committare-preliminarmente-tutte-le-modifiche-python.md).
  Accettazione: Commit fdad1d93e56496cc6cb9933b3089729175d5b5b3; gate e hook superati; checkout operativo pulito.

- [P01.02 — Registrare identità ed evidenze della baseline](tasks/P01.02-registrare-identita-ed-evidenze-della-baseline.md).
  Accettazione: Manifest e ricevuta identificano esattamente il commit preliminare; limiti e verifiche pendenti sono espliciti.

- [P01.03 — Qualificare un checkout isolato del campione](tasks/P01.03-qualificare-un-checkout-isolato-del-campione.md).
  Accettazione: Checkout pulito qualificato oppure blocco riproducibile documentato. Disponibilità remota registrata senza dichiararla verificata per deduzione.

## P02 — Contratto e corpus di controllo

- [P02.01 — Inventariare i contratti e i test esistenti](tasks/P02.01-inventariare-i-contratti-e-i-test-esistenti.md).
  Accettazione: Ogni funzione obbligatoria ha identificativo, fonte e verifica prevista. Le ambiguità materiali aprono G2.

- [P02.02 — Preparare il corpus sintetico iniziale](tasks/P02.02-preparare-il-corpus-sintetico-iniziale.md).
  Accettazione: Corpus ricostruibile con errori, duplicati, date/unità e provenienza attesa; output di riferimento generati dalla baseline fissata.

- [P02.03 — Costruire il confronto e provarne la sensibilità](tasks/P02.03-costruire-il-confronto-e-provarne-la-sensibilita.md).
  Accettazione: Le mutazioni deliberatamente introdotte sono rilevate; ogni differenza significativa produce un esito macchina non riuscito.

## P03 — Fondazioni Rust e CI

- [P03.01 — Creare workspace e toolchain Rust](tasks/P03.01-creare-workspace-e-toolchain-rust.md).
  Accettazione: Build e test minimi riproducibili; directory e dipendenze rispettano l'architettura; primo esempio rustdoc eseguito.

- [P03.02 — Definire errori ed eventi delle operazioni](tasks/P03.02-definire-errori-ed-eventi-delle-operazioni.md).
  Accettazione: Test di successi, errori e annullamento; eventi ordinati e nessun successo dopo un fallimento.

- [P03.03 — Implementare il gate e la CI multipiattaforma](tasks/P03.03-implementare-il-gate-e-la-ci-multipiattaforma.md).
  Accettazione: Esecuzione locale e job sui tre OS verificati tramite esiti effettivi. La mancanza di runner resta un limite pendente.

- [P03.04 — Provare presto il pacchetto minimo](tasks/P03.04-provare-presto-il-pacchetto-minimo.md).
  Accettazione: Ricevute per le piattaforme effettivamente provate; problemi di linkage individuati prima del porting.

## P04 — Prove di fattibilità e GUI

- [P04.01 — Verificare fattibilità PDF e OCR](tasks/P04.01-verificare-fattibilita-pdf-e-ocr.md).
  Accettazione: Report con risultati reali, alternative e limiti. Aprire G2 se un requisito obbligatorio non è copribile alle condizioni approvate.

- [P04.02 — Verificare conversioni Office e container](tasks/P04.02-verificare-conversioni-office-e-container.md).
  Accettazione: Matrice di fattibilità, requisiti di packaging e casi non coperti documentati.

- [P04.03 — Verificare DICOM e codec](tasks/P04.03-verificare-dicom-e-codec.md).
  Accettazione: Risultati separati per catalogazione, media e codec; limiti e alternative pronti per G2 se necessario.

- [P04.04 — Verificare identificazione dei volumi](tasks/P04.04-verificare-identificazione-dei-volumi.md).
  Accettazione: Contratto degli adattatori per OS e prove effettivamente eseguite registrati; nessuna equivalenza UUID presunta.

- [P04.05 — Selezionare due toolkit per il confronto](tasks/P04.05-selezionare-due-toolkit-per-il-confronto.md).
  Accettazione: Due candidati motivati e protocollo identico: cartelle, tabella, progressi, errore, annullamento, DPI, temi, tastiera e accessibilità.

- [P04.06 — Realizzare i prototipi e presentare G1](tasks/P04.06-realizzare-i-prototipi-e-presentare-g1.md).
  Accettazione: Report basato su prove, limiti espliciti e richiesta di scelta G1 al manutentore; nessuna selezione implicita.

## P05 — Configurazione e metadati

- [P05.01 — Portare configurazione e validazione](tasks/P05.01-portare-configurazione-e-validazione.md).
  Accettazione: Fixture valide e invalide concordano col contratto; precedenze e diagnosi italiane verificate.

- [P05.02 — Portare metadati e isolamento dei dati](tasks/P05.02-portare-metadati-e-isolamento-dei-dati.md).
  Accettazione: Metadati invalidi bloccano il lavoro prima dell'elaborazione; test d'isolamento e protezione dei dati superati.

- [P05.03 — Gestire profili e importazione dei percorsi storici](tasks/P05.03-gestire-profili-e-importazione-dei-percorsi-storici.md).
  Accettazione: Test di cambio cwd, spazi, Unicode, percorsi relativi e profili riaperti su ciascun OS.

## P06 — Prima pipeline completa TXT

- [P06.01 — Portare inventario e digest](tasks/P06.01-portare-inventario-e-digest.md).
  Accettazione: Inventari canonici e digest equivalenti sui casi sintetici; originali immutati.

- [P06.02 — Completare il nucleo TXT fino al frontend](tasks/P06.02-completare-il-nucleo-txt-fino-al-frontend.md).
  Accettazione: Pipeline TXT eseguibile con dati e link verificati; risultati confrontati con la baseline.

- [P06.03 — Esportare e validare il sottoinsieme simulato](tasks/P06.03-esportare-e-validare-il-sottoinsieme-simulato.md).
  Accettazione: Primo ciclo completo sui sistemi disponibili, errori di integrità rilevati e seconda esecuzione corretta.

## P07 — Ingestione documentale

- [P07.01 — Portare il PDF digitale](tasks/P07.01-portare-il-pdf-digitale.md).
  Accettazione: Corpus PDF, errori e effetti su ricerca/parametri verificati; dipendenze di distribuzione registrate.

- [P07.02 — Portare l'OCR PDF](tasks/P07.02-portare-l-ocr-pdf.md).
  Accettazione: Casi sintetici OCR e fallimento verificati; risultato e dipendenze conformi alla decisione P04/G2.

- [P07.03 — Portare immagini e rappresentazioni semplici](tasks/P07.03-portare-immagini-e-rappresentazioni-semplici.md).
  Accettazione: Originali identici, link validi e nessuna dipendenza OCR richiesta dalle sole immagini.

- [P07.04 — Portare conversioni Office](tasks/P07.04-portare-conversioni-office.md).
  Accettazione: Originale preservato, derivato consultabile o esito motivato; matrice per formato aggiornata.

- [P07.05 — Portare container e staging](tasks/P07.05-portare-container-e-staging.md).
  Accettazione: Membri confinati nello staging, nessuna scrittura esterna e risultati equivalenti sui casi normali e anomali.

- [P07.06 — Portare catalogazione DICOM](tasks/P07.06-portare-catalogazione-dicom.md).
  Accettazione: Inventari di studi equivalenti e provenienza integra, inclusi media danneggiati.

- [P07.07 — Portare media, viewer e anteprime DICOM](tasks/P07.07-portare-media-viewer-e-anteprime-dicom.md).
  Accettazione: Originali conservati anche se le anteprime falliscono; link offline e sintassi di trasferimento concordate verificati.

## P08 — Regole cliniche e AIFA

- [P08.01 — Portare importazioni di osservazioni](tasks/P08.01-portare-importazioni-di-osservazioni.md).
  Accettazione: Output equivalenti per i casi sintetici e importazioni obsolete rifiutate secondo contratto.

- [P08.02 — Portare parametri, slice e segni vitali](tasks/P08.02-portare-parametri-slice-e-segni-vitali.md).
  Accettazione: Valori, campioni, unità, date, scarti e provenienza equivalenti; numeri assenti non diventano zeri.

- [P08.03 — Portare riferimenti e fogli AIFA](tasks/P08.03-portare-riferimenti-e-fogli-aifa.md).
  Accettazione: Ambiguità gestite, download falliti diagnosticati e uso offline dei documenti locali verificato.

## P09 — Database, frontend e build

- [P09.01 — Completare SQLite e FTS5](tasks/P09.01-completare-sqlite-e-fts5.md).
  Accettazione: Righe, vincoli e query attese equivalenti; nessuna build parziale presentata come completa.

- [P09.02 — Completare export JSON e JavaScript](tasks/P09.02-completare-export-json-e-javascript.md).
  Accettazione: Comparatori strutturali e test di collegamento superati; pazienti isolati.

- [P09.03 — Completare il frontend offline](tasks/P09.03-completare-il-frontend-offline.md).
  Accettazione: Test browser dei flussi e dei link, inclusi intervalli senza dati, valori mancanti, polso e smontaggio dei grafici.

- [P09.04 — Completare build e cache incrementale](tasks/P09.04-completare-build-e-cache-incrementale.md).
  Accettazione: Build completa/incrementale equivalenti; ripetizioni e modifiche dei sorgenti invalidano i dati appropriati.

## P10 — Export USB multipiattaforma

- [P10.01 — Implementare copia e aggiornamento portabili](tasks/P10.01-implementare-copia-e-aggiornamento-portabili.md).
  Accettazione: Directory simulate identiche al manifest; errori e ripresa verificati senza perdere originali.

- [P10.02 — Implementare adattatore Linux dei volumi](tasks/P10.02-implementare-adattatore-linux-dei-volumi.md).
  Accettazione: Test dell'adattatore e prove su target qualificato documentati separatamente.

- [P10.03 — Implementare adattatore Windows dei volumi](tasks/P10.03-implementare-adattatore-windows-dei-volumi.md).
  Accettazione: Test del sistema e delle differenze di identificazione verificati su Windows reale o VM idonea.

- [P10.04 — Implementare adattatore macOS dei volumi](tasks/P10.04-implementare-adattatore-macos-dei-volumi.md).
  Accettazione: Test e prove sul sistema qualificati, con comportamento su rimozione del volume.

- [P10.05 — Completare validazione e recupero USB](tasks/P10.05-completare-validazione-e-recupero-usb.md).
  Accettazione: Copia/validazione/distacco correttamente distinti; nessun esito positivo dopo copia incompleta.

## P11 — Applicazione grafica

- [P11.01 — Creare la GUI e i profili](tasks/P11.01-creare-la-gui-e-i-profili.md).
  Accettazione: Flusso profili verificato; accesso da tastiera e stati invalidi espliciti.

- [P11.02 — Collegare controlli e generazione](tasks/P11.02-collegare-controlli-e-generazione.md).
  Accettazione: UI reattiva e test degli stati di successo, errore, annullamento e ripresa.

- [P11.03 — Collegare export e risultato](tasks/P11.03-collegare-export-e-risultato.md).
  Accettazione: Un operatore completa il flusso ordinario senza terminale; destinazioni ambigue o errate bloccate.

- [P11.04 — Qualificare usabilità e accessibilità](tasks/P11.04-qualificare-usabilita-e-accessibilita.md).
  Accettazione: Ricevute di test GUI e limiti espliciti, senza chiamare end-to-end una sola prova del core.

## P12 — Pacchetti e qualificazione

- [P12.01 — Costruire i pacchetti completi](tasks/P12.01-costruire-i-pacchetti-completi.md).
  Accettazione: Artefatti, hash, dipendenze e procedure di build pubblicabili; nessun Python runtime nascosto.

- [P12.02 — Provare installazione e aggiornamento su sistemi puliti](tasks/P12.02-provare-installazione-e-aggiornamento-su-sistemi-puliti.md).
  Accettazione: Ricevute delle prove complete e corpus di parità superato o limiti bloccanti dichiarati.

- [P12.03 — Preparare una prerelease verificata](tasks/P12.03-preparare-una-prerelease-verificata.md).
  Accettazione: Prerelease con perimetro qualificato e recupero documentato; eventuale pubblicazione secondo autorizzazione specifica.

## P13 — Transizione e phase-out

- [P13.01 — Presentare la transizione operativa](tasks/P13.01-presentare-la-transizione-operativa.md).
  Accettazione: Requisiti obbligatori verificati e rapporto concreto per approvazione G3.

- [P13.02 — Presentare il phase-out Python](tasks/P13.02-presentare-il-phase-out-python.md).
  Accettazione: Approvazione G4 registrata prima della dismissione; riferimenti storici e recupero mantenuti.

## Criteri trasversali della parità

- Inventario completo dei comportamenti implementati, non delle sole idee.
- Originali identici per SHA-256 e nessuna alterazione delle fonti.
- Equivalenza di valori, date, unità, riferimenti e provenienza.
- SQLite confrontato per struttura, dati, vincoli e ricerche.
- JSON normalizzato solo secondo regole dichiarate.
- Estrazione testo valutata per formato e impatto sulle funzioni derivate.
- Frontend offline verificato con ricerca, timeline, terapia, documenti,
  grafici e DICOM, su file:// e con risorse locali.
- Build completa/incrementale e copie interrotte provate in modo ripetibile.
- Pacchetti verificati su sistemi puliti per i target dichiarati.
- Differenze rispetto al campione spiegate; bug noti non copiati per conformità.

La baseline Python è qualificata inizialmente su Linux. Produrre lì gli
expected e confrontare Rust sui tre OS; testare separatamente percorsi,
filesystem, identificativi dei volumi e interazione grafica.

## Ripresa

Il [ledger](ledger.md) deve indicare ultima scheda conclusa, commit,
verifiche, blocchi e prossima attività. Il prompt di avvio in
[START-HERE.md](START-HERE.md) consente una nuova sessione senza trasferire
l'intera conversazione. Nessuna fase può saltare un checkpoint per
risparmiare tempo o far risultare verde la CI.

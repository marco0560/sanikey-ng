# Contratto del prodotto

## Obiettivo

Sostituire gradualmente il generatore Python con un'implementazione Rust
qualificata, dotata di GUI e pacchetti per Windows, Linux e macOS.
La riduzione delle dipendenze esterne e l'apprendimento Rust sono obiettivi
espliciti. I limiti tecnici non autorizzano riduzioni implicite di funzionalità.

## Autorità e perimetro

La baseline è identificata da [baseline.toml](migration/baseline.toml).
Per ogni requisito usare codice e configurazione, test, quindi documentazione
e ADR attive; registrare le incongruenze. Le vecchie specifiche e i piani
rimossi possono fornire contesto storico ma non nuovi requisiti obbligatori.

P02 costruirà l'inventario completo del comportamento attuale. Nessun elemento
della matrice iniziale di parità è già verificato in Rust.

## Invarianti da preservare

- Originali, configurazione e metadati curati sono autorevoli e protetti.
- Ogni paziente mantiene isolamento logico, archivio e frontend indipendenti.
- I metadati curati prevalgono sulle informazioni derivate.
- Valori, unità, date e provenienza non si alterano nelle normalizzazioni.
- La build e la distribuzione sono operazioni distinte e verificabili.
- Il frontend esportato funziona offline da file:// senza servizi locali.
- La chiavetta non richiede installazioni sul computer di consultazione.
- `usb_uuid` identifica il filesystem o volume del target secondo la baseline;
  un eventuale ID di deploy è distinto ([ADR-0003](decisions/0003-identita-usb.md)).
- I riferimenti ai file esportati sono relativi e validabili.
- Gli originali DICOM restano disponibili; le anteprime non sono diagnostiche.
- Le immagini sorgente non ricevono OCR diretto; l'OCR PDF è una funzione distinta.
- Il campione Python resta operativo fino all'approvazione della transizione.

## Compatibilità e differenze consentite

Preservare i significati di configurazione e metadati e documentare
l'importazione delle configurazioni storiche senza dipendere dal checkout.
Le differenze di UI del generatore e di confezionamento sono attese.
Le differenze di dati o comportamento richiedono analisi e accettazione;
una differenza dovuta a un bug Python non si risolve copiando il bug.

Confrontare SQLite per schema, righe, vincoli e ricerche, non per byte.
Confrontare JSON strutturalmente e originali per SHA-256.
I criteri per estrazione testo saranno definiti per formato, senza rimuovere
differenze capaci di cambiare la ricerca o l'estrazione dei parametri.

## Esclusioni della prima migrazione

Cloud, AI clinica, mobile/PWA, FHIR/HL7 e un viewer DICOM generale non entrano
nel contratto solo perché compaiono in documenti storici.
La formattazione dei dispositivi resta fuori dalla prima GUI.
La consultazione clinica statica non viene riprogettata insieme al generatore.

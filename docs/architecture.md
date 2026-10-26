# Architettura prevista

Stato: piano approvato; il workspace P03.01 contiene i crate iniziali
`sanikey-core`, `sanikey-cli` e `xtask`. Al momento il core espone solo la
versione e la CLI implementa informazioni/versione. Le pipeline SaniKey e la
GUI restano da implementare; il crate desktop attende G1.

## Componenti

- sanikey-core: tipi di dominio, contratti, orchestrazione e servizi headless.
- sanikey-cli: adattatore CLI per automazione, diagnosi e confronti.
- sanikey-desktop: adattatore grafico, da introdurre dopo G1.
- xtask: verifiche e strumenti del repository, esclusi dal prodotto.

Organizzare inizialmente il core in moduli, evitando un crate per ogni file
Python. Gli adattatori di sistema, conversione e rete hanno confini espliciti.
Nuove separazioni in crate richiedono un bisogno concreto e una ADR.

## Flusso

Configurazione e profili → validazione → scansione e metadati →
estrazione e rappresentazioni consultabili → osservazioni e riferimenti →
database e indici → frontend statico → immagine esportabile →
copia USB e validazione.

GUI e CLI chiamano gli stessi servizi. L'avanzamento è costituito da eventi
tipizzati; il testo italiano è una rappresentazione per l'utente.
Le operazioni lunghe devono mantenere reattiva la GUI e dichiarare i propri
punti sicuri di annullamento e ripresa.

## Portabilità

L'app installata non assume una directory corrente, una root Git o un PATH
arricchito dai file di avvio della shell. I profili definiscono basi esplicite.
Le operazioni su volumi usano adattatori per piattaforma verificati.
Il target di copia deve essere validato prima di modificare file.

## Dipendenze e distribuzione

Preferire dipendenze Rust incorporabili quando rispettano il contratto.
Per PDF/OCR, Office e DICOM verificare fattibilità, codec, licenze e
confezionamento prima del porting esteso.
Una libreria C/C++ incorporata va inventariata anche quando non richiede
installazione separata. Autocontenuto non significa necessariamente statico.

Il frontend USB resta HTML/CSS/JavaScript e asset locali. Il passaggio a Rust
riguarda il generatore e non richiede la conversione del frontend in Rust.

## Confini del campione

Il confronto Python è uno strumento di sviluppo. Non introdurre dipendenze
Python nel runtime Rust per dichiarare completo un adattatore mancante.
La baseline Python è qualificata inizialmente solo nell'ambiente Linux
registrato; i risultati di riferimento possono essere usati su altri sistemi,
con test separati delle differenze di filesystem e dispositivi.

# Distribuzione e dati locali

## Obiettivi

Pacchetti separati per Windows, Linux e macOS, costruiti e provati su ciascuna
piattaforma. OS minimi e architetture saranno fissati con le prove P04 e P12.
P03.04 ha provato la sola CLI minima su Linux in un'installazione temporanea;
nessun pacchetto di prodotto è disponibile.

## Classificazione delle dipendenze

Per ogni funzione registrare: dipendenza di compilazione, libreria incorporata,
asset vendorizzato, eseguibile incluso oppure requisito runtime esterno.
Misurare dimensioni, avvio e comportamento senza rete su sistemi puliti.
Non dedurre l'autonomia del pacchetto dal solo successo sul computer di sviluppo.

P04 valuterà PDF/OCR, Office, archivi, DICOM e accesso ai volumi.
I casi non coperti devono mantenere stato pendente o aprire G2;
non diventano facoltativi senza una decisione esplicita.

## Aree locali

Le directory local-data, config, exports, logs, .artifacts, target e dist
sono ignorate. Nessun dato sanitario va inserito nei contenuti pubblici.
Usare dati sintetici anche in screenshot, documenti dimostrativi e CI.

- local-data/reference/: checkout di controllo e dati privati di sviluppo.
- .artifacts/migration/: evidenze durevoli locali, escluse da Git.
- docs/migration/verification/: sole ricevute pubblicabili e prive di dati privati.
- config/: impostazioni locali; nessun percorso personale negli esempi pubblici.

Le directory temporanee della sessione sono quelle previste dalle istruzioni
globali del manutentore. Non usarle per baseline, report o evidenze da conservare.

## Qualificazione dei pacchetti

Verificare fuori dal checkout: assenza di Git e Python, PATH ridotto,
percorsi Unicode, cartelle senza permessi di scrittura, rete assente,
convertitori assenti e rimozione del supporto USB.
Verificare installazione, aggiornamento e ritorno alla versione precedente.
Firma e notarizzazione richiedono credenziali dedicate e non possono essere
dichiarate completate senza evidenze.

## Stato della baseline Python

Il commit di riferimento è locale al momento del bootstrap.
La sua pubblicazione sul repository Python non è stata eseguita né autorizzata
implicitamente dalla pubblicazione di sanikey-ng.
P01.03 deve verificare la recuperabilità e registrare l'eventuale richiesta
specifica per pubblicare una ref immutabile prima di usarla nella CI remota.

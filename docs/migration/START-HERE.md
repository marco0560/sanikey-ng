# Avvio di una sessione Luna High

## Stato da cui ripartire

La preparazione ha committato tutte le modifiche Python nel riferimento
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`, con gate superato.
Ha predisposto repository e documentazione per il piano approvato.
Il workspace Rust include CLI info/versione, gate CI, comparatori e pipeline
di consultazione per TXT, PDF, Markdown, HTML, immagini JPEG/PNG e conversioni
Office. P07.05 ha portato container e staging con fixture sintetiche Linux;
la capability è implementata ma IMG/ISO reali, alcuni codec e Windows/macOS
restano pending. Anche parità Office e browser restano pending. P08.01 ha
portato le importazioni di osservazioni con fixture sintetiche Linux; fogli
non qualificati, Windows/macOS e integrazione frontend/database restano pending.
P07.06 ha portato catalogazione
DICOM e P07.07 media, viewer HTML offline e anteprime sintetiche non
diagnostiche. La capability DICOM resta pending per generazione DICOMDIR senza
indice sorgente, integrazione frontend e qualifiche mancanti. Il prodotto GUI
è pianificato per P11.
Il corpus sintetico P02.02 è pronto con expected fissati alla baseline.

Il [ledger](ledger.md) riporta lo stato effettivo della pubblicazione.
P08.02 è completata: scoperta candidati, layout tabellari annotati e impilati,
regole curate, conversioni, punti e serie, report diagnostici, segni vitali e
celle descritte “valore in massa/percentuale”. P08.03 è completata a livello
core: ricerca e classificazione AIFA, conferma esplicita, persistenza TOML,
download FI/RCP in coppia e lookup filesystem-only dei PDF per uso offline. La
capability resta pending finché il servizio reale non viene qualificato; P09.02
collega ora FI/RCP locali e URL AIFA agli export. P09.01 ha portato schema
completo, builder transazionale, provenienza, DICOM e FTS5 equivalente al
campione. P09.02 è completata: export JSON/JavaScript, dati clinici e DICOM,
ricerca, report tecnici e Markdown CommonMark sono confrontati su fixture
sintetiche. Raw HTML è escapato e i link non sicuri restano testo inerte; vedi
la [ricevuta P09.02](verification/2026-10-28-exports-checkpoint.md).
P09.03 ha completato il generatore e gli asset del frontend offline. I flussi
principali sono stati provati con Chromium su Linux tramite `file://`; la
qualifica browser su Windows/macOS resta da eseguire. P09.04 ha aggiunto le API
core per build singole e multi-persona, cache derivata, report e ripresa dopo
esiti parziali ([ricevuta](verification/2026-11-03-build-cache.md)); i comandi
CLI di build restano da collegare. P10.01 ha implementato la sincronizzazione
portabile di un'immagine esistente, con riuso per digest, gestione degli
obsoleti, avanzamento e ripresa; non assembla ancora l'immagine USB completa.
P10.02 ha aggiunto l'adattatore Linux in sola lettura per spazio, mount, UUID e
filesystem; i test usano fixture sintetiche e una lettura del checkout host,
senza target USB fisici ([ricevuta P10.02](verification/2026-11-03-linux-volume-adapter.md)).
P10.03 ha implementato il confine Win32 e la verifica del Volume GUID approvata
il 2026-11-03, ma la scheda resta incompleta: manca una compilazione ed
esecuzione su Windows reale o VM ([ricevuta P10.03](verification/2026-11-03-windows-volume-adapter.md)).
Il manutentore ha chiesto il 2026-11-03 di completare prima le verifiche
disponibili su Linux e passare ai test Windows solo dopo. P10.03 resta pendente
per la qualifica Windows. P10.04 ha aggiunto l'adattatore macOS e i controlli
sintetici eseguibili su Linux; compilazione e qualifica macOS restano pendenti.
P10.05 aggiunge la copia verificata del sottoinsieme TXT e il controllo Linux
prima/dopo sul volume; la parità USB completa e la qualifica di rimozione
fisica restano pending ([ricevuta P10.05](verification/2026-11-03-usb-validated-copy.md)).
P11.01 ha introdotto la GUI desktop con importazione profili e selezione
session-only. P11.02 collega preflight, revisione duplicati, build in thread,
progresso, errori, annullamento tra persone e ripresa incrementale. P11.03
assembla e valida l'immagine multi-persona, collega la sincronizzazione alla
cartella `SaniKey` su volume Linux e presenta conferma e report; gate e fixture
sintetiche passano ([ricevuta](verification/2026-11-03-desktop-usb-flow.md)).
La finestra si avvia sul compositor Plasma dell'host. Le prove headless Linux
coprono Tab/focus, temi chiaro/scuro, scaling 1×/2× e viewport stretto. Una
prova nativa a 460×360 ha rilevato il taglio del pulsante profilo; le righe
sono state rese a capo e il test verifica che la label resti nel rettangolo di
clip. La nuova schermata non è stata ricatturata in modo affidabile dopo la
correzione; tastiera fisica, errori interattivi e tecnologie assistive restano
pending. La copia USB nel desktop è Linux-only; Windows e macOS restano da
qualificare. Il manutentore proseguirà con i test Windows.
G2 ha approvato la semantica dell'identità USB; la qualifica degli adattatori
multipiattaforma è ancora da fare ([ADR-0003](../decisions/0003-identita-usb.md)).
P01.03 ha qualificato il campione in un checkout pulito e ha verificato il
fetch remoto dello SHA fissato; non è stata eseguita una push del Python.

## Prerequisiti della sessione

- Aprire il cwd sanikey-ng, verificare istruzioni applicabili e stato Git.
- Selezionare Luna High per l'esecuzione desiderata dal manutentore.
- Individuare il checkout SaniKey Python locale senza registrare percorsi
  personali in Git. Usare una configurazione locale ignorata o un parametro.
- Leggere piano, ledger, baseline e scheda prima di usare strumenti.
- Verificare Cargo, rustc, strumenti di documentazione e rete quando servono.
  Non installare una toolchain diversa senza leggere la scheda P03.
- Usare le credenziali solo attraverso gli ambienti SOPS previsti dal
  contratto globale; non leggere o stampare i valori decrittati.

## Prompt da incollare

```text
Lavora nel repository sanikey-ng come esecutore del piano già approvato.
Leggi AGENTS.md, docs/migration/plan.md, docs/migration/ledger.md,
docs/migration/baseline.toml e CONTRIBUTING.md.

Verifica lo stato reale del repository prima di riprendere.
La baseline Python è fdad1d93e56496cc6cb9933b3089729175d5b5b3:
include tutte le modifiche che erano pendenti ed è già stata committata.
Non creare un altro commit di congelamento e non sostituirla con upstream.

Riprendi dalla prossima scheda indicata nel ledger.
Esegui una scheda alla volta con test pertinenti, documentazione, registro
delle evidenze e commit atomico. Mantieni separati esito del processo,
parità funzionale e prove sui sistemi o supporti reali.
Per ogni API o dipendenza consulta le fonti ufficiali e Context7.
Non presumere capacità Codira Rust senza verificarle.

Prosegui autonomamente nelle attività approvate e indipendenti.
Fermati alle decisioni G1–G4 e ai cambiamenti materiali del contratto.
Non eseguire push o release del Python senza autorizzazione specifica.
Non pubblicare dati clinici, configurazioni private, log o percorsi personali.
Aggiorna il ledger e lascia la prossima scheda esatta a ogni interruzione.
```

## Recupero

Se un controllo fallisce, conservare comando, exit status ed evidenza
nell'area durevole prevista. Determinare se il problema riguarda ambiente
oppure prodotto prima di cambiare configurazioni o codice.
Se il contesto si esaurisce, riprendere dal ledger e dallo stato Git;
non riavviare il progetto e non ripetere commit già conclusi.

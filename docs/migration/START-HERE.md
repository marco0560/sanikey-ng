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
La prossima scheda è quella indicata nel [ledger](ledger.md): P08.02.
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

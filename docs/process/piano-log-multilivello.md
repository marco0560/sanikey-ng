# Piano approvato: log multilivello

Stato: piano approvato; implementazione e qualificazione ancora da eseguire.
Il perimetro comprende GUI desktop, core, CLI e adattatori di volume del
progetto. Il generatore installato deve continuare a funzionare senza checkout,
Git o Python. L'attivazione vale per la sola sessione e il log è disattivato
all'avvio, salvo richiesta esplicita.

## Livelli

Ogni livello comprende il precedente:

1. **Eventi**: avvio e chiusura, azioni nella finestra, comandi CLI, passaggi
   delle operazioni, durate ed errori classificati. Non registra dati sanitari,
   nomi, percorsi, argomenti privati o messaggi di errore non sanitizzati.
2. **Diagnostica**: aggiunge dettagli tecnici, identificativi temporanei di
   operazione, componenti, conteggi e stati; copia inoltre tutto il testo
   mostrato nella GUI, inclusi valori, messaggi ed errori. Può quindi contenere
   dati sanitari, percorsi e altre informazioni private. La copia deve seguire
   anche i cambiamenti del contenuto durante la sessione, identificando il
   contesto necessario a ricostruire ciò che è stato mostrato.
3. **Traccia completa**: aggiunge tutti i dati elaborati, inclusi quelli
   sanitari, e ingresso, uscita, argomenti e risultati rappresentabili delle
   funzioni del codice SaniKey comprese nel perimetro.

Le credenziali e i segreti sono esclusi da tutti i livelli, anche se visualizzati
per errore. Il livello eventi usa campi esplicitamente ammessi, senza
formattazione generica di strutture o errori che possa esporre dati privati.
Per dati binari, grandi, non rappresentabili o non disponibili al punto di
osservazione, l'implementazione deve documentare le eventuali omissioni e
segnalarle nel log; non deve presentare come completa una traccia incompleta.

## Attivazione e conservazione

- La GUI offre la scelta del livello per la sessione e mostra il percorso del
  file. Prima di attivare diagnostica o traccia completa avverte che il log
  può contenere dati sanitari e privati e richiede conferma esplicita.
- La CLI offre opzioni equivalenti per il singolo processo, descritte nel suo
  aiuto; i livelli contenenti dati privati richiedono attivazione esplicita.
- Una cartella locale predefinita, appropriata al sistema operativo, non dipende
  dal checkout. Ogni sessione usa un file distinto; i log non vengono inclusi
  nell'export USB né in Git.
- Limitare l'accesso ai file con le capacità della piattaforma e segnalare i
  limiti. Documentare posizione, conservazione, eliminazione e condivisione
  consapevole; non introdurre cancellazioni automatiche senza un criterio
  approvato.
- Segnalare all'operatore gli errori di apertura o scrittura. Rendere evidente
  quando la registrazione si interrompe o è limitata e il log è incompleto.

## Implementazione

1. Definire infrastruttura e formato comuni a GUI, CLI e core, con contesto di
   sessione e correlazione delle operazioni, anche nei thread della GUI.
   Confermare API e dipendenze con Context7 e documentazione ufficiale.
2. Inventariare le funzioni di produzione dei crate SaniKey e strumentarle
   progressivamente per ingresso, uscita, durata, argomenti e risultati nel
   livello completo. Censire metodi, codice generato e casi speciali; mantenere
   un inventario verificabile della copertura e delle eccezioni.
3. Registrare gli eventi dei flussi e la copia del testo GUI nei livelli
   previsti, senza perdere i cambiamenti visualizzati. Gestire i valori privati
   secondo il livello selezionato e l'esclusione dei segreti.
4. Tracciare i dati passati e ricevuti ai confini con librerie, FFI e programmi
   esterni. Non dichiarare osservate le chiamate interne di tali componenti.
5. Integrare le opzioni di sessione e i messaggi operativi in italiano nella
   GUI e nella CLI; documentare percorso e limiti multipiattaforma.

## Verifica e chiusura dell'implementazione

- Usare fixture sintetiche per eventi, durate, errori, correlazione tra thread,
  chiamate, argomenti, risultati e testo GUI, inclusi cambiamenti durante la
  sessione. Verificare la copertura contro l'inventario delle funzioni.
- Verificare che dati sanitari, percorsi e valori privati di prova non appaiano
  nel livello eventi, che il testo GUI appaia nella diagnostica e che i dati
  previsti appaiano nella traccia completa. Verificare l'esclusione dei segreti
  in tutti i livelli e la dichiarazione di ogni omissione o troncamento.
- Provare attivazione, disattivazione, sessioni distinte, errori di scrittura,
  permessi, dimensioni, prestazioni e confini verso processi esterni.
- Aggiornare documentazione operativa, contratto e ledger con risultati e
  limiti effettivi. Eseguire i test mirati e `cargo xtask check`; qualificare
  Windows e macOS separatamente, senza dedurne il funzionamento dai test Linux.

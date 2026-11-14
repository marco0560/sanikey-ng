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

L'adattatore volumi Linux usa `rustix` incluso nel binario e il comando
`findmnt` fornito da util-linux per leggere mount, filesystem e UUID. `findmnt`
può mancare solo se non sono richiesti UUID globale ed exFAT; la misura dello
spazio continua a usare l'API filesystem. L'adattatore è in sola lettura e
richiede una directory target esistente. La verifica su questo host non
qualifica supporti USB reali; la matrice Windows/macOS è ancora da realizzare.

L'adattatore Windows usa `windows-sys` per le API di `kernel32.dll`, senza
eseguibili esterni; le chiamate FFI sono isolate in `sanikey-windows-volume`.
Legge il Volume GUID path, il filesystem e lo spazio disponibile senza scrivere
sul target. L'adattatore non è stato compilato o eseguito su Windows in questa
sessione e richiede qualifica su un host reale o VM Windows.

## Aree locali

Le directory local-data, patients, generated, config, exports, logs,
.artifacts, target e dist sono ignorate. Nessun dato sanitario va inserito
nei contenuti pubblici.
Usare dati sintetici anche in screenshot, documenti dimostrativi e CI.

- local-data/reference/: checkout di controllo e dati privati di sviluppo.
- .artifacts/migration/: evidenze durevoli locali, escluse da Git.
- docs/migration/verification/: sole ricevute pubblicabili e prive di dati privati.
- config/: impostazioni locali; nessun percorso personale negli esempi pubblici.

## Log diagnostici in corso di implementazione

Il logging è disattivato per default. La CLI accetta opzioni prima del
comando, per esempio `sanikey --log-level events info`. I livelli `diagnostic`
e `trace` richiedono anche `--confirm-sensitive-logging`: possono contenere
dati sanitari, percorsi e altri dati privati, incluso testo non classificabile.
I segreti identificati sono esclusi; un segreto inserito in un campo generico
non è distinguibile con certezza da altri dati.

Nella CLI, parsing delle opzioni di registrazione, verifica del consenso e
apertura della sessione sono bootstrap precedenti al contesto di log. Gli
errori di sintassi delle opzioni terminano senza creare un log; se la directory
o la sessione non possono essere aperte, il comando prosegue senza registrazione
e segnala su stderr che il log è incompleto. Queste operazioni iniziali non
hanno span autonome e non possono essere ricostruite dal log della sessione.

Nella GUI la sezione «Registrazione log per questa sessione» permette di
selezionare il livello, visualizzare cartella e file attivo e disattivare il
log. Diagnostica e Traccia richiedono la conferma della registrazione di
contenuti privati. L'attivazione riguarda solo la sessione corrente; le azioni
precedenti non possono essere ricostruite. Durante un'operazione in corso non
è possibile cambiare o disattivare la registrazione.

La diagnostica registra un record `content_omitted` per ogni apertura di un
picker nativo del sistema (profilo, destinazione USB o campo cartella), perché
il testo interno di queste finestre non è osservabile dalla GUI SaniKey. Il
record indica solo la categoria del picker e il motivo tecnico, senza percorso
selezionato o altri dati privati. Il livello `events` non registra questo
dettaglio. Un errore di scrittura attiva l'avviso di registrazione incompleta.

Le tracce e gli eventi sono **incompleti** durante l'implementazione. Ogni
sessione scrive un record `coverage` con questo stato; l'inventario sintattico
è ottenibile con `cargo xtask log-inventory` dal checkout di sviluppo. Le
invocazioni di macro e le chiamate esterne richiedono verifiche separate.
Ogni riga JSON contiene un numero `sequence` crescente assegnato alla scrittura;
con thread concorrenti i timestamp di creazione possono risultare fuori ordine.
Un `parent_call_id` propagato a un worker indica causalità: la chiamata padre
può essere terminata prima dell'inizio della chiamata nel worker. Il log non
può chiudersi finché i collegamenti ai worker restano attivi.

La directory predefinita non dipende dalla directory corrente: su Linux è
`$XDG_STATE_HOME/sanikey/logs`, oppure `.local/state/sanikey/logs` sotto la
directory home dell'utente se
`XDG_STATE_HOME` è assente, vuota o relativa; su macOS è
`Library/Logs/sanikey/logs` sotto la directory home; su Windows è
`%LOCALAPPDATA%\sanikey\logs`.
La CLI consente una directory diversa con `--log-dir`: sceglierla fuori dalle
aree esportate e dalle directory versionate. Ogni sessione crea un file nuovo
`*.sanikey-log.jsonl`, che il repository ignora anche fuori da `logs/`.

Su Unix la creazione richiede permessi 0700 per nuove directory e 0600 per
nuovi file, ulteriormente limitati dalla umask. Una directory finale già
esistente viene rifiutata se è un link simbolico o è scrivibile da gruppo o
altri; i suoi permessi non vengono modificati. Le directory più aperte in sola
lettura possono comunque rendere visibili i nomi dei file. Il file viene creato
rispetto alla directory aperta e verificata, anche se il percorso viene sostituito
in seguito. Su Windows valgono le ACL
ereditate e la loro protezione deve essere verificata separatamente. Non è
prevista cancellazione automatica: conservare, condividere ed eliminare i log
come dati privati. Errori di creazione e scrittura sono segnalati; un log
interrotto non deve essere considerato completo. Le prove su Windows e macOS
restano da eseguire.
Se si chiude la GUI con un worker ancora attivo, il log riceve un evento
`shutdown_pending`; il supervisore tenta la chiusura dopo il termine del
worker. Se il processo termina prima, il record `finished` può mancare.

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

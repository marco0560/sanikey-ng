# Inventario delle tracce — verifica Linux del 2026-11-14

`cargo xtask log-inventory` genera un inventario sintattico con hash del
sorgente, nomi, scope e condizioni `cfg`; l'output va rigenerato dopo ogni
modifica. Nello snapshot verificato risultavano 3.219 elementi, inclusi macro e
closure, 97 siti `trace_current`, 482 invocazioni di macro e 149 chiamate a
funzioni riconosciute dal nome come possibili wrapper di traccia. Questi numeri
sono indizi, non una percentuale di funzioni effettivamente coperte.

Il confronto dei simboli senza indizi riconosciuti ha individuato le seguenti
eccezioni e aree da verificare separatamente:

- Il bootstrap CLI (`main`, `execute_with_progress_notice`,
  `parse_log_arguments`, `parse_log_level`) e l'avvio desktop precedono la
  creazione della sessione; gli eventi precedenti non sono retroattivi.
- Le funzioni del logger, i suoi proiettori e le routine di classificazione e
  redazione usate dal logger stesso non aprono una span ricorsiva.
- Gli adattatori di volume sono crate indipendenti: le operazioni native sono
  esposte al core tramite observer. Le funzioni di conversione e alcuni helper
  interni degli adattatori non hanno una span autonoma. Le osservazioni FFI
  riportano la durata misurata dell'API, ma i test sintetici Linux non
  qualificano l'esecuzione nativa Windows o macOS.
- Metodi `const`, implementazioni derivate, codice generato e token interni
  delle macro richiedono trattamento separato; l'inventario non espande le
  macro né valuta tutte le condizioni di compilazione. Le closure sono censite,
  ma non tutte sono correlate automaticamente a una span.
- Gli indizi riconoscono alcuni wrapper per nome. La presenza di un indizio
  non dimostra che la span venga eseguita o che argomenti e risultati siano
  completi; l'assenza non dimostra che una funzione non sia strumentata.

I test sintetici esercitano specifici flussi e confini, non tutte le funzioni
su tutte le piattaforme. La copertura complessiva resta **incompleta** finché
non sia verificata una mappatura funzione per funzione, incluse eccezioni e
condizioni di compilazione, e non siano eseguite le qualifiche native previste.

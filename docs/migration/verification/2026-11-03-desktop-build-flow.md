# P11.02 — Preflight e generazione desktop

Data: 2026-11-03. Implementazione e test automatizzati su Linux.

## Modifica

- Il preflight gira su un thread di lavoro e usa i servizi core per verificare privacy/metadati e scandire l'inventario delle persone selezionate.
- La GUI mostra conteggio documenti e duplicati con SHA-256 e percorsi; l'operatore li rivede prima di avviare la build.
- La generazione usa `build_patient` del core su un thread separato. I percorsi modificati restano di sessione; quelli relativi digitati sono risolti rispetto alla base del profilo.
- L'avanzamento è indeterminato durante la build della persona corrente e diventa conteggio determinato fra persone. La UI continua a ricevere input mentre il core lavora.
- L'annullamento viene osservato tra persone, dopo aver lasciato terminare quella corrente. Gli output delle persone già completate restano visibili; il comando di build usa la modalità incrementale predefinita per riprendere.
- Errori e terminali success, failure e cancellation hanno stati visibili distinti. Se il thread termina senza inviare un esito, la UI avvisa di controllare gli artefatti e consente una nuova build incrementale.

## Verifiche

- `cargo test -p sanikey-desktop --offline`: 9/9 test passano per profilo, preflight/duplicati, percorsi relativi, percorsi vuoti, successo, errore, annullamento ai confini, stato UI e ripresa con una voce cache.
- `cargo clippy -p sanikey-desktop --all-targets --locked --offline -- -D warnings`: exit 0.
- `CARGO_NET_OFFLINE=true cargo xtask check`: exit 0; Clippy, test workspace (199 core con 2 ignored, 3 CLI, 9 desktop, 1 Windows-volume, 2 macOS-volume, 8 xtask), 2 doctest, build workspace, smoke CLI, controlli pubblicabilità (291 file) e rustdoc superati.
- `uv run codira index`: 1 indicizzato, 226 riusati, 0 fallimenti. `uv run codira audit` non segnala il crate desktop; restano i 12 file senza parser già registrati.
- Tentativi intermedi: Clippy ha richiesto il collasso di un `if`; il primo test dell'errore usava metadati mancanti, che il core ammette, ed è stato corretto con un conflitto sul percorso build; il primo gate non trovava ancora questa ricevuta referenziata dal ledger. Tutte le correzioni sono state applicate e gate/test ripetuti con esito positivo.
- L'interazione grafica manuale non è stata possibile: Wayland non ha un compositor e X11 non ha un display raggiungibile (diagnostica registrata nella ricevuta P11.01).

## Limite di annullamento

Il core non offre un annullamento intrapersona per `build_patient`. La UI lo dichiara: la richiesta resta pendente fino al confine sicuro dopo la persona corrente. Ripetere la build incrementale riutilizza gli artefatti già completati e la cache di estrazione.

## Prossimo passo

P11.03 collega export e risultato, mantenendo distinta la build locale dalla copia e dalla validazione del supporto.

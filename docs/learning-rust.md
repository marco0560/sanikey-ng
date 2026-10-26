# Imparare Rust attraverso SaniKey NG

Questo documento raccoglierà esempi del codice effettivamente implementato.
Al bootstrap non contiene esempi presentati come già eseguibili.

Per ogni fase aggiungere una spiegazione breve di una scelta reale:

| Fase | Argomento da spiegare nel contesto |
| --- | --- |
| P03 | Workspace, Result, errori, test unitari e rustdoc |
| P05 | Ownership dei dati TOML, tipi di dominio e validazione |
| P06 | Iteratori, percorsi, digest e separazione dell'I/O |
| P07 | Adattatori, processi e confini FFI quando necessari |
| P08 | Numeri, unità, provenienza e trasformazioni esplicite |
| P09 | Transazioni, cache, serializzazione e determinismo |
| P10 | Gestione delle risorse, errori parziali e annullamento |
| P11 | Stato della GUI ed eventi delle operazioni |
| P12 | Feature, dipendenze native e distribuzione |

Evitare la traduzione riga per riga dal Python. Spiegare quando la scelta
Rust riduce uno stato invalido o rende un errore esplicito.
Gli esempi pubblici compilabili devono essere coperti da doctest.
Le note non devono duplicare interi moduli o introdurre astrazioni didattiche
senza una necessità nel prodotto.

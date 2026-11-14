# Misura sintetica del logger — 2026-11-14

La misura usa una fixture sintetica, senza rete, dati sanitari o USB. Un piccolo
programma Rust in release ha eseguito tre repliche di 500 iterazioni per livello.
Ogni iterazione crea un'operazione, registra fase e progresso e la termina;
Diagnostica aggiunge un testo visualizzato variabile di circa 128 caratteri,
Traccia aggiunge una span con JSON sintetico e un risultato numerico. Il timer
esclude la creazione iniziale della sessione e include la sua chiusura. Tutte
le repliche hanno prodotto lo stesso checksum sintetico, 2.136.250.

| Livello | Mediana per 500 iterazioni | Dimensione file | Byte per iterazione |
| --- | ---: | ---: | ---: |
| Disattivato | 0,004 ms | 0 B | 0 |
| Eventi | 142,181 ms | 440.926 B | 882 |
| Diagnostica | 174,817 ms | 610.820 B | 1.222 |
| Traccia | 275,079 ms | 929.806 B | 1.860 |

Il riferimento disattivato è sotto la risoluzione utile del timer. I livelli
registrano quantità diverse di dati: questi valori descrivono soltanto il
carico artificiale provato e non prevedono durata o dimensioni di una build
reale, della GUI o di file grandi. Il programma e i log temporanei della prova
sono stati creati sotto `.Temp` e non costituiscono evidenza durevole; questa
ricevuta conserva metodo e risultati, non pretende riproducibilità dei tempi.

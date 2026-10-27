# ADR-0002: decisione G1 sul toolkit GUI

- Stato: accettata dal manutentore (G1).
- Proposta: 2026-10-26; approvazione: 2026-10-27.
- Evidenza: [confronto P04.06](../migration/verification/2026-10-26-gui-toolkit-prototypes.md).

## Contesto

Il generatore richiede un'interfaccia Rust desktop su Windows, Linux e macOS.
GUI e CLI dovranno usare gli stessi servizi headless. La scelta deve tenere
conto di accessibilità, distribuzione, temi, tastiera e dialoghi cartella.
L'architettura non consente di dedurre parità o supporto da una semplice build.

## Alternative

### A — egui con eframe 0.37.0

Vantaggi osservati: licenza MIT OR Apache-2.0, tema runtime, integrazione
AccessKit e test del relativo albero con `egui_kittest`; in questa build Linux
ha prodotto il binario minore e il grafo più piccolo. Rischi: ruolo semantico
della tabella e accessibilità effettiva non provati; il renderer predefinito e
i driver dei sistemi target vanno qualificati.

### B — Slint 1.18.1

Vantaggi osservati: markup dichiarativo, tabella e indicatore di avanzamento
standard, proprietà di accessibilità e stile adattabile al sistema. Rischi:
licenza Community con attribuzione o altra licenza da selezionare; in questa
build Linux il binario e il grafo erano maggiori e `ldd` mostrava dipendenze di
font/rendering. Il tema chiaro/scuro si sceglie in compilazione, non con un
controllo runtime.

Entrambe usano `rfd` 0.17.2 per le cartelle; la documentazione del crate marca
la stabilità dell'API come sperimentale e i dialoghi non sono stati provati sui
tre OS. Tauri è rimasto fuori da questa coppia perché
introduce WebView e frontend web, una superficie di packaging e accessibilità
diversa dal confronto tra adattatori GUI Rust nativi.

## Decisione

Approvato egui/eframe 0.37.0 come toolkit GUI per il generatore. Slint 1.18.1
resta solo il secondo prototipo di confronto. Il manutentore ha approvato la
strada egui/eframe il 2026-10-27.

## Conseguenze e verifiche necessarie

Prima del rilascio del toolkit scelto, provare build, dialoghi, temi, tastiera,
accessibilità reale e confezionamento almeno sui sistemi disponibili. Una
decisione G1 non chiude la matrice di parità desktop; questa resta `pending`
fino a P11.

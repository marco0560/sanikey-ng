# P04.05 — Candidati GUI e protocollo di confronto

Data: 2026-10-26
Esito: due candidati fissati per P04.06; scelta finale G1 non presa.

## Candidati

| Candidato | Versioni fissate | Evidenza utile al confronto | Questioni da provare |
| --- | --- | --- | --- |
| egui con eframe | `egui = 0.37.0`, `eframe = 0.37.0`, `egui_extras = 0.37.0` | Interfaccia immediata Rust; `eframe` gestisce finestre e rendering desktop. AccessKit è abilitato da `eframe` sulle piattaforme con adapter disponibile; `egui_kittest` interroga l'albero accessibile. Le licenze dei crate sono MIT OR Apache-2.0. | La navigazione e gli annunci vanno provati con screen reader reali sui tre OS; il rendering predefinito WGPU, le feature minime, le dipendenze di sistema e le dimensioni dei pacchetti vanno misurati. Il tema sperimentale va valutato senza dipendere da API instabili. |
| Slint | `slint = 1.18.1` | UI dichiarativa compilata in codice Rust; il crate abilita per default la feature `accessibility`. Gli elementi espongono ruoli e nomi accessibili; `StandardTableView` e `ProgressIndicator` coprono direttamente due controlli richiesti. Le lunghezze logiche seguono il fattore DPI del dispositivo. | Provare tastiera e screen reader sulle piattaforme target, incluse le celle della tabella; misurare backend, renderer, dipendenze di sistema e pacchetto. La licenza Community richiede attribuzione; alternative e condizioni vanno valutate prima di una scelta G1. |

Le versioni sono pin esatti per rendere ripetibile la comparazione, non una
raccomandazione di aggiornamento automatico. egui 0.37.0 è la release corrente
consultata; Slint 1.18.1 è la release stabile corrente consultata. Non è stato
aggiunto alcun crate al workspace in P04.05.

## Protocollo identico per P04.06

I due prototipi presentano la stessa finestra, le stesse stringhe italiane,
fixture sintetiche e lo stesso flusso di eventi del contratto P03.02.

1. Selezionare una cartella sorgente e una destinazione, visualizzando i
   percorsi scelti e gli errori di selezione in modo equivalente.
2. Mostrare una tabella con intestazioni, righe, selezione, scorrimento e
   ordinamento comparabili.
3. Riprodurre gli stessi eventi di avanzamento deterministici, inclusi stato
   indeterminato e completamento; mostrare un errore terminale italiano con
   codice macchina separato.
4. Annullare da tastiera e da controllo visibile; verificare che lo stato
   terminale sia quello di annullamento e che la finestra resti reattiva.
5. Percorrere tutti i controlli in ordine con tastiera, senza mouse; ispezionare
   ruoli, nomi e stati nell'albero AccessKit/accessibilità e provare uno screen
   reader sui sistemi disponibili.
6. Ripetere con scala DPI 100%, 150% e 200%, e temi chiaro e scuro. Registrare
   ritagli, testo troncato, contrasto e differenze non equivalenti.
7. Costruire su ogni sistema disponibile e registrare dimensione, dipendenze
   runtime, risorse incluse, avvio e comportamento offline. Nessuna prova su OS
   o screen reader non effettivamente disponibile sarà dichiarata superata.

La sorgente degli eventi sarà identica e controllata dal prototipo, così il
confronto UI non misura velocità casuali del generatore. Dialoghi e relativi
adapter saranno inventariati separatamente; non si presume che i due toolkit
li forniscano con lo stesso backend o requisito di sistema.

## Perimetro ed esclusioni

La richiesta P04.05 è un confronto limitato a due toolkit nativi Rust; la
selezione non è la decisione G1. Tauri resta fuori da questa coppia: combina
frontend web e WebView di sistema, con WebView2 su Windows e WebKit/WKWebView
su Linux/macOS. Introdurrebbe runtime e superfici di accessibilità diversi,
rendendo il prototipo non direttamente confrontabile con i due adattatori UI
Rust nativi richiesti dall'architettura. Può essere riaperto solo con una
modifica esplicita dello scope.

Nessun esito di accessibilità o distribuzione multipiattaforma è dedotto dalle
feature dichiarate. La capability `desktop` rimane `pending` fino a prove
funzionali e non implica parità.

## Fonti consultate

- [egui 0.37.0 release](https://github.com/emilk/egui/releases/tag/0.37.0)
- [egui accessibilità](https://github.com/emilk/egui/blob/0.37.0/docs/accessibility.md)
- [egui architettura dei crate](https://github.com/emilk/egui/blob/0.37.0/ARCHITECTURE.md)
- [Slint 1.18.1 su docs.rs](https://docs.rs/crate/slint/1.18.1)
- [Slint accessibilità](https://docs.slint.dev/latest/docs/slint/reference/common/)
- [Slint StandardTableView](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/standardtableview/)
- [Slint ProgressIndicator](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/basic-widgets/progressindicator/)
- [Slint DPI](https://docs.slint.dev/latest/docs/slint/guide/development/debugging_techniques/)
- [Licenze Slint](https://slint.dev/faqs)
- [Tauri versioni WebView](https://tauri.app/reference/webview-versions/)
- Context7: `/emilk/egui/0.37.0` e `/websites/slint_dev`, query su accessibilità, tastiera, DPI, temi e controlli.

## Verifiche

- Esame documentale delle API e delle versioni indicato sopra; nessun prototipo
  compilato in questa scheda.
- Il protocollo è identico per i due candidati e copre cartelle, tabella,
  avanzamento, errore, annullamento, DPI, temi, tastiera e accessibilità.
- La scelta del toolkit resta al manutentore in G1, dopo P04.06.

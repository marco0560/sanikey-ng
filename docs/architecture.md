# Architettura prevista

Stato: piano approvato; il workspace P03.01 contiene i crate iniziali
`sanikey-core`, `sanikey-cli` e `xtask`. P03.02 aggiunge al core errori
categorizzati, eventi tipizzati e annullamento cooperativo; la CLI implementa
informazioni/versione. P06 fornisce una pipeline sintetica limitata ai file
`.txt`; la pipeline completa multi-formato e la GUI restano da realizzare. Il
crate desktop attende G1.

## Componenti

- sanikey-core: tipi di dominio, contratti, orchestrazione e servizi headless.
- sanikey-cli: adattatore CLI per automazione, diagnosi e confronti.
- sanikey-desktop: adattatore grafico, da introdurre dopo G1.
- xtask: verifiche e strumenti del repository, esclusi dal prodotto.

`sanikey-core::metadata::prepare_people_metadata` applica i controlli di
percorso runtime e carica tutti i metadati delle persone abilitate prima che
un servizio inizi l'elaborazione. La preparazione restituisce risultati solo
se tutte le persone superano la validazione; non invoca Git. `xtask check`
mantiene separata la scansione Git di directory locali e riferimenti host prima
della pubblicazione.

`sanikey-core::config::AccountsProfile` importa file account esistenti tramite
un percorso assoluto e ne conserva il contenuto originale. L'esportazione crea
solo un nuovo file nella stessa base dei percorsi relativi, rifiuta
sovrascritture e verifica byte e configurazione riaprendo la copia; non richiede
la directory corrente, Git o Python.

`sanikey-core::inventory` visita in sola lettura la directory sorgente,
applica precedenza include su exclude e crea record canonici con categoria,
tipo, data, SHA-256 e identità del documento. La scansione completa conserva i
duplicati per la diagnosi; la vista deduplicata mantiene la prima occorrenza
nell'ordine dei percorsi.

`sanikey-core::txt_pipeline` produce SQLite con FTS5, JSON e pagine HTML offline
con originali copiati e link relativi. `build_txt_pipeline` legge `.txt`;
`build_consultation_pipeline` cataloga anche PDF, Markdown, HTML e immagini
JPEG/PNG e converte i formati Office baseline in PDF. Le immagini sono
copiate e linkate con testo e warning vuoti, senza richiedere OCR. Markdown
conserva il testo ricercabile; HTML è linkato come originale. DOCX, ODT e RTF
usano Pandoc; DOC legacy e fogli di calcolo usano LibreOffice in modalità
headless. Il PDF consultabile è un derivato: l'originale Office resta copiato
e linkato separatamente. Se il convertitore manca, termina male, supera 120
secondi o produce un PDF invalido, il catalogo conserva l'originale e registra
un warning senza esporre stderr o contenuto privato. La prova completa dei
formati e il confezionamento multipiattaforma restano pending.

`sanikey-core::containers::stage_container_documents` riconosce i contenitori
dalle estensioni della baseline e verifica il formato con il parser o il
convertitore disponibile. Estrae ZIP e TAR.XZ con librerie Rust; 7z usa il
parser Rust e ripiega su 7-Zip per metodi non supportati. RAR e immagini IMG/ISO
usano UnRAR, 7-Zip o bsdtar quando presenti. Per questi
strumenti esterni l'app scrive i byte dei singoli membri nello staging dopo
aver validato il nome; non delega loro la scrittura di percorsi estratti. La
ricorsione, i filtri di ingestione, il lock per persona e il manifest v1
seguono il contratto Python. La parità di IMG/ISO validi, le varianti di codec
e i sistemi Windows/macOS restano da qualificare. La baseline non definisce
quote di dimensione o numero dei membri; il porting non ne aggiunge.

`build_pdf_pipeline` estrae testo digitale con `lopdf` e, quando è
insufficiente, renderizza ogni pagina PDF con PDFium e invia il raster PPM via
stdin a Tesseract. Usa la soglia di 40 caratteri non whitespace della
baseline, conserva intervalli Unicode per pagina e warning, e li registra nel
JSON e in `document_page_spans`. La forzatura OCR si applica solo se ogni
pagina è coperta per almeno il 90% da un'immagine. PDFium e Tesseract sono
runtime esterni: la libreria PDFium va accanto all'eseguibile o indicata con
`SANIKEY_PDFIUM_LIBRARY`, Tesseract è cercato nel `PATH` o indicato con
`SANIKEY_TESSERACT`; il modello predefinito inglese deve essere installato. Il
glossario medico italiano è incorporato e materializzato in un file temporaneo
protetto durante l'OCR. CSV, altri formati e metadati clinici completi restano
fuori dal perimetro; la qualifica multipiattaforma è ancora pending.

`sanikey-core::simulated_export` assembla questo sottoinsieme in una nuova
directory, crea un manifest deterministico con checksum, verifica l'integrità
e i link locali e finalizza l'albero tramite rename dallo staging. Non rileva né
scrive su supporti fisici e non costituisce la capability USB completa.

Organizzare inizialmente il core in moduli, evitando un crate per ogni file
Python. Gli adattatori di sistema, conversione e rete hanno confini espliciti.
Nuove separazioni in crate richiedono un bisogno concreto e una ADR.

## Flusso

Configurazione e profili → validazione → scansione e metadati →
estrazione e rappresentazioni consultabili → osservazioni e riferimenti →
database e indici → frontend statico → immagine esportabile →
copia USB e validazione.

GUI e CLI chiamano gli stessi servizi. L'avanzamento è costituito da eventi
tipizzati; il testo italiano è una rappresentazione per l'utente.
Le operazioni lunghe devono mantenere reattiva la GUI e dichiarare i propri
punti sicuri di annullamento e ripresa.

Il core mantiene separati i codici macchina dei fallimenti dai messaggi
italiani. Ogni operazione emette un solo evento terminale dopo gli eventi di
avanzamento; l'annullamento cooperativo viene osservato ai punti sicuri
dichiarati dall'operazione.

## Portabilità

L'app installata non assume una directory corrente, una root Git o un PATH
arricchito dai file di avvio della shell. I profili definiscono basi esplicite.
Le operazioni su volumi usano adattatori per piattaforma verificati.
Il target di copia deve essere validato prima di modificare file.

## Dipendenze e distribuzione

Preferire dipendenze Rust incorporabili quando rispettano il contratto.
Per PDF/OCR, Office e DICOM verificare fattibilità, codec, licenze e
confezionamento prima del porting esteso. Il parser PDF digitale usa `lopdf`
con feature predefinite disattivate. L'OCR usa `pdfium-render 0.9.4` con
feature predefinite e supporto immagini disattivati, più il binario Tesseract
e i suoi modelli installati a runtime. Il confezionamento dei binari nativi e
la qualifica Windows/macOS restano da verificare.
Una libreria C/C++ incorporata va inventariata anche quando non richiede
installazione separata. Autocontenuto non significa necessariamente statico.

Il frontend USB resta HTML/CSS/JavaScript e asset locali. Il passaggio a Rust
riguarda il generatore e non richiede la conversione del frontend in Rust.

## Confini del campione

Il confronto Python è uno strumento di sviluppo. Non introdurre dipendenze
Python nel runtime Rust per dichiarare completo un adattatore mancante.
La baseline Python è qualificata inizialmente solo nell'ambiente Linux
registrato; i risultati di riferimento possono essere usati su altri sistemi,
con test separati delle differenze di filesystem e dispositivi.

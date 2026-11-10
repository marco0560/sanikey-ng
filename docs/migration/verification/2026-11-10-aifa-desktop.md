# Revisione desktop dei riferimenti AIFA

Data: 2026-11-10. Baseline Python: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

La GUI consente di caricare i farmaci delle persone selezionate, cercare nel
catalogo AIFA, esaminare i candidati e confermare un riferimento o lo stato
non AIFA. La ricerca e la verifica FI/RCP sono eseguite fuori dal thread UI.
La conferma verifica entrambi i documenti prima di registrare il riferimento.
La GUI richiede una conferma esplicita anche per un unico candidato esatto;
la CLI della baseline può confermarlo automaticamente.

Gli aggiornamenti cooperanti dei riferimenti sono serializzati, rileggono i
metadati e rilevano conflitti sul farmaco; la sostituzione usa un file
temporaneo nella stessa directory. Il lock è advisory: processi che non lo
rispettano possono interferire. Il file è sincronizzato, non la directory:
la permanenza del rename dopo un crash improvviso non è garantita.

## Prova online limitata

Una richiesta pubblica alla ricerca AIFA per `paracetamolo` ha restituito
HTTP 200, JSON con dieci risultati e identificativi attesi. Per il risultato
pubblico PARACETAMOLO SELLA, codice SIS 3399 e AIC6 29811, richieste dirette
agli endpoint FI e RCP hanno restituito HTTP 200 e firma iniziale `%PDF-`.
È un controllo puntuale diretto degli endpoint, non una prova end-to-end della
GUI, una verifica strutturale dei PDF o una qualifica dell'intero catalogo.

## Verifiche e limiti

Test sintetici mirati coprono caricamento, controlli accessibili, conferma,
errore di verifica, aggiornamenti concorrenti e permessi Unix. La qualifica
interattiva della GUI e quella end-to-end del servizio reale restano pendenti.
La capability AIFA resta `pending` nella matrice di parità.

Il 2026-11-10 `cargo xtask check` è terminato con exit 0 su Linux; i controlli
mirati hanno incluso 16 test core AIFA e 26 test desktop, tutti superati.

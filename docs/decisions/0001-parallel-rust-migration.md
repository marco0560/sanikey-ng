# ADR-0001: migrazione parallela verificata

## Stato

Accettata dal manutentore il 2026-10-26 come parte del piano.

## Contesto

SaniKey Python è operativo. Il generatore deve evolvere verso pacchetti per
Windows, Linux e macOS con poche dipendenze esterne. Il progetto serve anche
ad apprendere programmazione, documentazione e test in Rust.

## Decisione

Sviluppare sanikey-ng in un repository separato pubblico.
Conservare Python come campione di controllo fino alla qualificazione Rust.
L'utente richiede di committare preliminarmente tutti i file Python modificati:
il riferimento è il commit risultante, non un commit precedente o un nuovo
snapshot scelto successivamente.

La baseline fissata è `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.
La selezione del toolkit richiede G1. Riduzioni del contratto richiedono G2.
Transizione operativa e phase-out richiedono rispettivamente G3 e G4.

## Conseguenze

Occorrono un corpus sintetico, confronti verificabili e una matrice della parità.
Il successo di compilazione e processo non dimostra equivalenza funzionale.
Le dipendenze documentali complesse richiedono prove di fattibilità iniziali.
Le nuove feature del Python dopo il commit non entrano automaticamente nel
riferimento: serve una decisione esplicita sull'aggiornamento della baseline.

## Verifica

Identità e risultati in [baseline.toml](../migration/baseline.toml) e nella
[ricevuta preliminare](../migration/verification/2026-10-26-python-baseline.md).
Stato operativo nel [ledger](../migration/ledger.md).

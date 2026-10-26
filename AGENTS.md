# AGENTS.md — SaniKey NG

## Fonti e struttura

- Leggere `docs/migration/START-HERE.md` e `docs/migration/ledger.md`.
- Il piano approvato è `docs/migration/plan.md`; le schede sono sotto
  `docs/migration/tasks/`.
- Il contratto funzionale è `docs/product-contract.md`; l'architettura è
  `docs/architecture.md`.
- La baseline Python è fissata da `docs/migration/baseline.toml`.
  Non sostituire il suo SHA con il ramo upstream corrente.
- Il workspace Rust sarà introdotto in P03. Al bootstrap esiste solo
  documentazione: non presumere che comandi o crate futuri siano disponibili.
- Il generatore installato deve funzionare senza checkout, Git o Python.
  Il campione Python è una dipendenza di sviluppo, esclusa dal prodotto.

## Esecuzione e stato

- Eseguire una scheda per volta rispettandone dipendenze e ambito.
- Registrare risultati, comandi, commit, blocchi e prossima scheda nel ledger.
- La matrice `docs/migration/parity.toml` descrive la parità funzionale.
  Una scheda conclusa non dimostra da sola la parità del relativo sottosistema.
- G1 richiede approvazione del toolkit; G2 risolve cambiamenti al contratto;
  G3 approva la transizione operativa; G4 approva il phase-out di Python.
- La creazione e pubblicazione del nuovo repository sono approvate.
  Non eseguire push, release o modifiche al repository Python di riferimento
  senza autorizzazione specifica.

## Dati, lingua e dipendenze

- Dati sanitari, configurazioni private e log non entrano in Git.
- Usare solo fixture sintetiche. Le aree private ignorate sono documentate
  in `docs/deployment.md`.
- UI, messaggi e documentazione operativa sono in italiano.
  Identificatori, chiavi macchina e nomi tecnici restano in inglese.
- Confermare API e dipendenze con documentazione ufficiale e Context7.
- Verificare la copertura Rust prima di richiedere Codira per questo repository;
  la sua disponibilità per Python non garantisce l'indicizzazione Rust.

## Validazione e commit

- Prima di P03 seguire i controlli documentali in `CONTRIBUTING.md`.
- Da P03 il gate sarà `cargo xtask check`, da implementare e verificare.
- Sono richiesti controlli pertinenti alla scheda e il gate completo alla
  chiusura di una fase coerente; non saltare hook né indebolire i controlli.
- Commit atomici Conventional Commits, secondo `CONTRIBUTING.md`.

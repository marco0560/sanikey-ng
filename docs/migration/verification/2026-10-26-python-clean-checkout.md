# Ricevuta P01.03 — Checkout pulito del campione Python

Data: 2026-10-26. Piattaforma: Linux.

## Identità e isolamento

- Repository campione: `https://github.com/marco0560/sanikey`.
- Commit qualificato: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.
- Checkout isolato: `local-data/reference/sanikey`, detached HEAD sul commit fissato.
- Il checkout operativo del campione era pulito prima e dopo la verifica.
- Il checkout isolato era pulito dopo il gate.

## Dipendenze e gate

Il primo sync offline è terminato con exit 1 perché il wheel `duckdb==1.5.3`
non era presente in cache. Il sync successivo, vincolato al lockfile, è
terminato con exit 0. Il gate `uv run --locked python
scripts/validate_repo.py` è terminato con exit 0:

- 359 test superati e 1 skip, coerenti con la ricevuta della baseline;
- copertura totale 92%;
- privacy check, controlli repository, hook e test completati senza errori.

Il report copertura generato nel checkout isolato è stato trattato come stato
locale ignorato; non è stato incluso in Git.

## Recuperabilità remota

`git ls-remote` filtrato per lo SHA non ha restituito una ref diretta.
La prova decisiva è un fetch dell'oggetto esatto in un repository bare
temporaneo separato: exit 0, seguito da `git cat-file -e` sul commit con exit
0. Lo SHA è quindi recuperabile dal repository remoto. Non è stata eseguita
una push del campione.

## Limiti

Questa qualifica riguarda un checkout pulito Linux e il gate del repository.
Non qualifica altri sistemi operativi, supporti USB fisici o parità funzionale
con un'implementazione Rust.

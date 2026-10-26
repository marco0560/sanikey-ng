# Ricevuta del commit preliminare Python

Data: 2026-10-26. Ambiente di validazione: Linux.

## Identità

- Repository: https://github.com/marco0560/sanikey.
- Parent: `175e00cabbb7043f9b0ff97b1abe5534b60ddfc9`.
- Commit: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.
- Subject: fix(generation): stabilize parameter charts and USB exports.
- Ambito: tutti i dodici file modificati prima dell'avvio del piano.
- Diff: 1142 inserimenti e 95 rimozioni.
- Working tree dopo il commit: pulito.

## Contenuto

Grafici standard ed estesi, navigazione temporale, stili e collegamenti ai
parametri; risultati di laboratorio annotati e distinzione sangue/urine;
copia rsync senza limite totale di durata e recupero dopo interruzione;
test e documentazione corrispondenti.

## Controlli eseguiti

| Controllo | Esito |
| --- | --- |
| Gate completo validate_repo.py | Exit 0; 359 passed, 1 skipped |
| Copertura complessiva arrotondata | 92% |
| Privacy guard | Superato |
| Codira index e audit | Superati; nessun finding docstring |
| Ruff, formattazione, mypy | Superati |
| Controlli non di codice e cheatsheet | Superati |
| MkDocs strict | Exit 0 |
| git diff --check e verifica dello staging | Superati |
| Hook del commit | Superati |

Il primo tentativo del gate si è fermato sul download degli ambienti
pre-commit per una restrizione DNS della sandbox. Il secondo tentativo,
con rete autorizzata, ha completato il gate. Nessun controllo è stato aggirato.

Il test del target fisico opzionale non qualifica una chiavetta reale.
Non sono stati eseguiti export sui dati clinici né prove USB fisiche.
Il risultato non dimostra portabilità della pipeline Python fuori da Linux.

## Evidenze locali e limiti

I log integrali e i relativi exit file sono conservati, fuori da Git,
sotto la directory relativa al repository Python
`local-data/migration/sanikey-ng-baseline-20261026/`.
Questa ricevuta contiene solo informazioni pubblicabili.

La verifica del checkout pulito e della disponibilità remota dello SHA è
ancora P01.03. Non è stata eseguita una push del repository Python.

# Registro di avanzamento

## Stato della preparazione

Data: 2026-10-26. Il piano è approvato con la modifica preliminare P01.
La sessione preparatoria deve fermarsi dopo repository e documentazione.
Il manutentore aprirà una nuova sessione Luna High per l'implementazione.

Baseline Python: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.
Stato Rust: non implementato. Stato GUI: toolkit non scelto.

| Scheda | Stato | Evidenza |
| --- | --- | --- |
| P01.01 | completata | Commit preliminare con tutti i dodici file; working tree pulito |
| P00.01 | completata | Destinazione locale disponibile; GitHub 404 e account marco0560 verificati |
| P00.02 | completata | 53 schede; TOML, link locali, grafo e invarianti verificati |
| P01.02 | completata | Manifest e ricevuta confrontati con lo SHA reale |
| P00.03 | completata | Repository pubblico creato; primo commit pubblicato e SHA remoto verificato |
| P01.03 | da_eseguire | Qualificazione da checkout isolato e verifica remoto |

Tutte le altre schede dell'[indice](tasks/README.md) sono da_eseguire.
La matrice di parità è iniziale e interamente pending.

## Risultati del campione

- Gate Python: exit 0, 359 passed, 1 skipped, copertura complessiva 92%.
- Documentazione MkDocs strict: exit 0.
- Hook pre-commit e commit-msg: superati.
- Nessuna prova USB fisica e nessuna qualifica Python Windows/macOS.
- Nessuna push del Python effettuata.
- Dettagli e limiti nella [ricevuta](verification/2026-10-26-python-baseline.md).

## Blocco esterno da verificare

Il commit Python è disponibile nel checkout locale.
La sua recuperabilità pubblica non è stata verificata e non va presunta.
P01.03 può qualificare il campione locale; l'eventuale pubblicazione del
riferimento Python necessita di autorizzazione specifica prima della CI remota.

## Checkpoint

| Checkpoint | Stato |
| --- | --- |
| G1 — toolkit GUI | In attesa del confronto P04 e del manutentore |
| G2 — cambiamenti materiali | Nessuna deroga richiesta o approvata |
| G3 — transizione operativa | Non raggiunto |
| G4 — phase-out Python | Non raggiunto |

## Prossima sessione

Leggere [START-HERE.md](START-HERE.md).
La preparazione è conclusa. Nessun processo di implementazione è stato avviato.
Il manutentore può aprire ora la sessione Luna High.

Riprendere da [P01.03](tasks/P01.03-qualificare-un-checkout-isolato-del-campione.md)
come prima attività della nuova sessione.
Successivamente rispettare il grafo: P03 precede il confronto Rust P02.03.

A ogni scheda aggiungere data, commit, comandi ed esiti effettivi, eventuali
skip con motivazione, evidenze, blocchi e successiva attività.
Non modificare retroattivamente questa cronologia per nascondere tentativi falliti.

## Consegna della preparazione

- Repository pubblico: https://github.com/marco0560/sanikey-ng.
- Primo commit: `9eeafcc4ff3d31271cae3bf72bc1d4d773d23bfd`.
- Pubblicazione iniziale su main verificata con lettura della ref GitHub.
- 73 file di bootstrap; 53 schede; 20 capacità con parità ancora pending.
- Controlli documentali e git diff --cached --check: superati.
- SaniKey Python: commit preliminare presente e working tree pulito.
- Implementazione Rust, GUI, corpus e comparatori: non avviati.
- Il toolkit rimane da scegliere in G1.
- Ripresa prevista: P01.03. Non ripetere P01.01.
- Dopo il commit di questa consegna, verificare allineamento main/remoto
  e fermare la sessione preparatoria, come richiesto dal manutentore.

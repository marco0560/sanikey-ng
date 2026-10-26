# Registro di avanzamento

## Stato della preparazione

Data: 2026-10-26. Il piano è approvato con la modifica preliminare P01.
La sessione di bootstrap si è fermata dopo repository e documentazione;
l'implementazione approvata è ripresa in seguito con P03.01.

Baseline Python: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.
Stato Rust: P03.04 completata; gate e CI hosted Linux/Windows/macOS superati;
prova CLI minima limitata a Linux; pipeline non implementata.
Stato GUI: toolkit non scelto.

| Scheda | Stato | Evidenza |
| --- | --- | --- |
| P01.01 | completata | Commit preliminare con tutti i dodici file; working tree pulito |
| P00.01 | completata | Destinazione locale disponibile; GitHub 404 e account marco0560 verificati |
| P00.02 | completata | 53 schede; TOML, link locali, grafo e invarianti verificati |
| P01.02 | completata | Manifest e ricevuta confrontati con lo SHA reale |
| P00.03 | completata | Repository pubblico creato; primo commit pubblicato e SHA remoto verificato |
| P01.03 | completata | 2026-10-26; checkout isolato sul commit fissato, sync locked e gate exit 0; fetch remoto diretto dello SHA riuscito; commit `cb5d088`; [ricevuta](verification/2026-10-26-python-clean-checkout.md) |
| P02.01 | completata | 2026-10-26; inventario di 20 capability, CLI, formati, persistenza, default, errori e ADR; TOML/link/diff check superati; nessuna ambiguità materiale; commit `d70e743`; [ricevuta](verification/2026-10-26-contract-inventory.md) |
| P02.02 | completata | 2026-10-26; 17 fixture sintetici con hash, 11 generatori/adattatori referenziati con hash; baseline positiva e due negativi eseguiti; expected normalizzati; commit `b1bf5c0`; [ricevuta](verification/2026-10-26-synthetic-corpus.md) |
| P03.01 | completata | 2026-10-26; workspace di tre crate, toolchain 1.98.1, CLI info/versione, xtask e doctest; build, Clippy, rustdoc, unit test e doctest superati; commit `597bdf7`; [ricevuta](verification/2026-10-26-rust-foundation.md) |
| P03.02 | completata | 2026-10-26; errori codificati, eventi ordinati, progresso e annullamento cooperativo; info passa dal runner; 9 unit test, doctest, Clippy e rustdoc superati; commit `1ed6545`; [ricevuta](verification/2026-10-26-operation-contract.md) |
| P03.03 | completata | 2026-10-26; `cargo xtask check` exit 0; run 40404493580 completato su Ubuntu/Windows/macOS; commit `6a671e1`; [ricevuta](verification/2026-10-26-rust-gate-ci.md) |
| P03.04 | completata | 2026-10-26; installazione CLI release fuori checkout, `PATH` vuoto senza Git/Python; info/versione exit 0, comando sconosciuto exit 2; ELF 474.392 byte con linkage dinamico Linux registrato; commit `4b25bc0`; [ricevuta](verification/2026-10-26-cli-minimum-linux.md) |
| P04.01 | completata | 2026-10-26; PyMuPDF baseline e `lopdf 0.40.0` concordano sulla fixture PDF digitale; OCRmyPDF su fixture solo immagine restituisce il testo atteso; gate e limiti Linux registrati; commit `e8b1db8`; [ricevuta](verification/2026-10-26-pdf-ocr-feasibility.md) |

Le schede non registrate sopra sono da_eseguire; l'indice completo è in
[tasks/README.md](tasks/README.md).
La matrice di parità è iniziale e interamente pending.

## Risultati del campione

- Gate Python: exit 0, 359 passed, 1 skipped, copertura complessiva 92%.
- Documentazione MkDocs strict: exit 0.
- Hook pre-commit e commit-msg: superati.
- Nessuna prova USB fisica e nessuna qualifica Python Windows/macOS.
- Nessuna push del Python effettuata.
- Dettagli e limiti nella [ricevuta](verification/2026-10-26-python-baseline.md).

## Recuperabilità remota della baseline

P01.03 ha verificato il fetch diretto dello SHA dal repository Python remoto.
La baseline è recuperabile senza pubblicare una nuova ref. La CI remota potrà
usare il commit esistente; ogni push futuro del Python resta soggetto ad
autorizzazione specifica.

## Checkpoint

| Checkpoint | Stato |
| --- | --- |
| G1 — toolkit GUI | In attesa del confronto P04 e del manutentore |
| G2 — cambiamenti materiali | Nessuna deroga richiesta o approvata |
| G3 — transizione operativa | Non raggiunto |
| G4 — phase-out Python | Non raggiunto |

## Prossima sessione

Leggere [START-HERE.md](START-HERE.md).
La preparazione è conclusa; P03.02 ha completato il modello iniziale delle
operazioni Rust con commit `1ed6545`.

Riprendere da [P04.02](tasks/P04.02-verificare-conversioni-office-e-container.md).
P02.03 dipende dal gate introdotto da P03.03.
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
- Al bootstrap Rust, corpus e comparatori non erano avviati; P03.01 ha creato
  il workspace minimo e P02.02 ha fissato il corpus. GUI e comparatori restano
  da realizzare.
- Il toolkit rimane da scegliere in G1.
- Ripresa iniziale prevista: P01.03; completata il 2026-10-26. La scheda
  successiva è P02.01, come registrato sopra.

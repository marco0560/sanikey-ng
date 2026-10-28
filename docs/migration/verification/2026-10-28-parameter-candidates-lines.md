# P08.02a — Scoperta generica dei candidati per riga

Data: 2026-10-28. Baseline Python fissata: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.
Commit funzionale: `6c11b88` (`feat(core): porta scoperta base dei parametri clinici`).

## Ambito

Portato il primo sottopasso di P08.02: parsing di righe generiche in candidati
clinici, conservazione del testo numerico originale e provenienza, raggruppamento
per etichetta normalizzata esatta, contesto di pagina e distinzione di sezione
serum/urine. Il sottopasso non implementa i layout tabellari annotati o impilati,
né regole, conversioni/report o segni vitali; P08.02 resta in corso e la parità
della capability osservazioni resta pending.

## Verifiche

- Sorgente baseline `parameter_slices.py` verificato nel checkout isolato fissato.
- Sottoinsieme corrispondente della baseline Python: 6 test superati.
- Rust: `CARGO_HOME=/home/marco/Personalia/Progetti/.Temp/sanikey-p04-02/cargo-home CARGO_NET_OFFLINE=true cargo test --offline -p sanikey-core parameter_slices -- --nocapture` — 7 test superati.
- `CARGO_HOME=/home/marco/Personalia/Progetti/.Temp/sanikey-p04-02/cargo-home CARGO_NET_OFFLINE=true cargo clippy --offline -p sanikey-core --all-targets -- -D warnings` — exit 0.
- Hook `pre-commit` durante il commit — exit 0; test workspace e prototipi GUI completati. Il gate esplicito `cargo xtask check` non è stato rieseguito per questo sottopasso.

Il parser mantiene il lessico numerico e interpreta i separatori supportati per
la scoperta iniziale; layout tabellari, ulteriori regole di interpretazione e
conversioni cliniche restano fuori da questa ricevuta. Non si afferma parità
funzionale complessiva né qualifica multipiattaforma.

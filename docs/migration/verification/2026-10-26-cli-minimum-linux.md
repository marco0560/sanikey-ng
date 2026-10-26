# Ricevuta P03.04 — CLI minima su Linux

Data: 2026-10-26. Prova locale su Gentoo Linux x86_64; non rappresenta una
qualifica di tutte le distribuzioni Linux.

## Installazione isolata

Directory corrente di prova: `<scratch>` sotto l'area temporanea prevista
dalle istruzioni locali, fuori dal checkout. Installazione:

```bash
cargo install --path crates/sanikey-cli --root <scratch>/install --locked --offline
```

La binaria installata è stata eseguita per percorso assoluto con ambiente
`env -i` e `PATH` impostato a una directory vuota. `git`, `python` e `python3`
non erano risolvibili. `sanikey info` e `sanikey --version` hanno restituito
exit 0 e output atteso. `sanikey not-implemented` ha restituito exit 2 con
diagnostica italiana.

## Dipendenze e dimensioni

- Dipendenze Cargo: solo i crate del workspace; nessun crate esterno.
- Binaria ELF x86_64 release: 474.392 byte, non stripped.
- Linkage dinamico rilevato con `ldd`: `libc.so.6`, `libgcc_s.so.1` e il
  loader ELF del sistema. Queste librerie non sono incluse nell'eseguibile.
- Prove Windows/macOS, altri ABI/distribuzioni, percorso Unicode, rete assente
  su sistema pulito e condizioni di scrittura non sono state svolte da questa
  scheda.

La prova dimostra solo l'avvio della CLI minima senza checkout, Git o Python
nel `PATH`. Non dimostra parità funzionale, installazione completa del
generatore o portabilità binaria generale.

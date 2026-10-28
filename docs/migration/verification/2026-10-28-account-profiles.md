# Ricevuta P05.03 — Profili e percorsi storici

Data: 2026-10-28. Baseline Python fissata a
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`. La fonte di configurazione di
riferimento è `docs/config-example/accounts.toml` del checkout baseline.

## Comportamento portato

- `AccountsProfile::import` richiede un percorso assoluto, valida il TOML
  storico attraverso il loader account esistente e conserva il contenuto
  originale. Una volta importato, il profilo mantiene percorsi assoluti e la
  base storica indipendentemente dalla directory corrente.
- `export_copy` accetta solo una destinazione assoluta sotto la stessa base,
  crea un file nuovo senza sovrascrivere, copia i byte senza normalizzare
  alias o percorsi e riapre il risultato. L'esportazione viene accettata solo
  se byte e configurazione validata corrispondono; un risultato non valido
  viene rimosso.
- La copia mantiene i riferimenti relativi a documenti, metadata, build,
  immagine e dizionario se resta nella base originale. Non riscrive il file
  importato e non dipende da Git o Python.
- Questa è una capacità aggiuntiva Rust prevista da P05.03; non modifica lo
  schema di configurazione Python né la forma dei file storici.

## Verifiche

| Comando | Esito |
| --- | --- |
| `cargo test -p sanikey-core profile` | Exit 0 su Linux: importazione legacy, cambio cwd, spazi/Unicode, riferimenti relativi, riapertura, rifiuto di overwrite e base diversa |
| `cargo xtask check` | Exit 0 su Linux: fmt, Clippy, 3 test CLI, 55 test core, 8 test xtask, 1 doctest, build, smoke CLI, 3 test della guardia Python, pubblicabilità (191 file) e rustdoc |
| `uv run codira index`; `uv run codira cov`; `uv run codira audit` | Index exit 0 (5 indicizzati, 139 riusati); copertura completa senza gap; audit exit 0 con quattro rilievi rustdoc preesistenti negli helper di test metadata/privacy, fuori ambito |
| `git diff --check` | Exit 0 |

Durante lo sviluppo, i primi controlli mirati hanno trovato una visibilità
Rust errata del helper della base e due asserzioni di test impostate male;
entrambi i problemi sono stati corretti prima della verifica finale. Il primo
audit dopo il commit ha inoltre trovato il modulo test profilo privo di
rustdoc; la documentazione è stata aggiunta e il controllo aggiornato mantiene
solo i quattro rilievi precedenti fuori ambito.
La prova è stata eseguita solo su Linux: i test sono portabili, ma Windows e
macOS non sono stati eseguiti in questa scheda. Non sono stati usati dati reali.

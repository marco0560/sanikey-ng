# Ricevuta P05.02 — Metadati e isolamento

Data: 2026-10-28. Baseline Python: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.
Il lavoro Rust usa il crate `sanikey-core`; i controlli Git restano strumenti
di sviluppo e non diventano dipendenze del runtime installato.

## Comportamento portato

- Modelli Rust per provenienza, originali, problemi clinici, farmaci, foglietti
  AIFA ed esclusioni, terapie, procedure, osservazioni, serie/punti longitudinali,
  eventi di cronologia e aggregato `CuratedMetadata`.
- Loader dei file TOML curati per problemi, farmaci, foglietti, terapie,
  procedure, osservazioni, serie e punti, eventi, tag documentali e sintesi.
  I file assenti producono collezioni vuote; le cartelle metadata assenti restano
  facoltative come nella baseline.
- Validazione dei campi obbligatori e dei tipi, generazione deterministica
  degli ID terapia mancanti, unicità degli ID e riferimenti a farmaci/serie.
  L'aggregato viene restituito solo dopo tutte le validazioni.
- `prepare_people_metadata` valida i percorsi runtime e prepara tutte le
  persone abilitate come risultato atomico: un errore non espone risultati
  parziali alla fase successiva. La pipeline di elaborazione non è ancora
  implementata e non invoca ancora questo servizio.
- Il controllo runtime usa solo filesystem e non avvia Git. La scansione Git
  dei contenuti candidati resta nel controllo dev-only `scripts/check_publishable.py`,
  invocato da `cargo xtask check`; rifiuta directory private, file sensibili e
  riferimenti a percorsi host-specific. `patients/` e `generated/` sono ora
  ignorate da Git e documentate tra le aree locali.
- Originali: il preflight legge solo configurazione e metadata; un test
  sintetico verifica che i byte di un documento originale restino invariati.

La baseline contiene otto test diretti in `tests/test_metadata.py`; tutti hanno
un test Rust corrispondente elencato in `parity.toml`. I cinque casi privacy
sono in `tests/test_config.py` (non esiste `tests/test_privacy.py`) e lo script
`scripts/privacy_guard.py` è l'entrypoint della scansione Git Python. I test
Rust separano invarianti runtime e isolamento; tre test Python standard library
coprono la guardia di pubblicazione.

Le capability `metadata` e `privacy` sono registrate come `implemented`, non
`verified`: la scheda e i test Linux non dimostrano da soli la parità completa
su tutte le piattaforme o della pipeline futura.

## Verifiche

| Comando | Esito |
| --- | --- |
| `cargo xtask check` | Exit 0 su Linux: fmt, Clippy, 3 test CLI, 53 test core, 8 test xtask, 1 doctest, build, smoke CLI, 3 test della guardia Python, pubblicabilità (188 file) e rustdoc |
| `uv run codira index`; `uv run codira cov`; `uv run codira audit` | Rust/Markdown indicizzati, copertura senza gap, audit senza rilievi |
| `git diff --check` | Exit 0 |

L'orientamento della baseline Python ha trovato indicizzati i moduli richiesti.
La copertura globale Codira Python non è completa per 20 file fuori ambito
(`.yml`, `.csv`, `.toml`, `.js`, `.user-words)); i simboli metadata,
models e privacy richiesti sono presenti nell'indice. La versione Codira
installata non implementa il sottocomando `status`.

Nessun dato reale è stato usato. Nessuna verifica multipiattaforma, build
completa, database o esportazione USB è dichiarata in questa scheda.
Il primo gate finale ha segnalato un'asserzione Rust non formattata; dopo
`cargo fmt --all`, la riesecuzione del gate completo è terminata con exit 0.

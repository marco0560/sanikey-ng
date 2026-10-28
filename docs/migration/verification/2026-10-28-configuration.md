# Ricevuta P05.01 — Configurazione e validazione

Data: 2026-10-28. Baseline Python: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.
Implementazione nel crate `sanikey-core`, senza dipendenze Python nel runtime.

## Comportamento portato

- Sezioni `[global]`, `[[person]]`, `ui`, `search`, `ingestion` e `usb`, con
  tipi pubblici Rust documentati e messaggi di errore italiani categorizzati
  come `configuration`.
- Default della baseline, alias `repository_version`, ereditarietà globale,
  override UI/ricerca e unione additiva dei pattern di ingestione.
- UUID USB globale usato come default persona; conflitti controllati solo per
  persone abilitate, come nella baseline.
- Percorso convenzionale `config/accounts.toml`, espansione `~` per l'utente
  corrente, base relativa alla root quando il file è in `config/`, percorsi
  assoluti preservati e asset opzionali verificati.
- Ordine delle chiavi dei dizionari preservato con `toml` `preserve_order` e
  `IndexMap`, perché il dizionario viene esportato come JSON.
- Rifiuto di chiavi sconosciute a root e in `[global]`, oltre ai campi chiusi
  delle sezioni. L'inventario del contratto e la scheda vietano di ignorarle;
  la baseline non aveva test per chiavi sconosciute a root/global.
- `background_opacity` rifiuta anche `NaN`, che non soddisfa il contratto
  numerico tra zero e uno, benché il controllo Python attuale lasci passare
  quel valore.

Le prove privacy collocate nello stesso file Python sono fuori da questa
scheda: la matrice le assegna alla capability `privacy`, sorgente
`privacy.py`, da trattare con P05.02. La capability complessiva
`configuration` resta `pending` perché la gestione dei profili è P05.03 e
questa scheda da sola non prova tutta la parità del sottosistema.

## Corrispondenza dei test di contratto

| Test Python `tests/test_config.py` | Test Rust `sanikey-core` |
| --- | --- |
| `test_load_accounts_accepts_valid_synthetic_config` | `test_load_accounts_accepts_valid_synthetic_config` |
| `test_load_accounts_parses_ui_defaults_and_person_override` | `test_load_accounts_parses_ui_defaults_and_person_override` |
| `test_load_accounts_parses_search_and_background_config` | `test_load_accounts_parses_search_and_background_config` |
| `test_load_accounts_parses_ingestion_and_usb_config` | `test_load_accounts_parses_ingestion_and_usb_config` |
| `test_load_accounts_rejects_unknown_person_field` | `test_load_accounts_rejects_unknown_person_field` |
| `test_parse_accounts_uses_global_usb_uuid_default` | `test_parse_accounts_uses_global_usb_uuid_default` |
| `test_parse_accounts_rejects_missing_usb_uuid_without_global` | `test_parse_accounts_rejects_missing_usb_uuid_without_global` |
| `test_load_accounts_rejects_invalid_ui_values` (6 casi) | `test_load_accounts_rejects_invalid_ui_values` (6 casi) |
| `test_load_accounts_rejects_unknown_ui_field` | `test_load_accounts_rejects_unknown_ui_field` |
| `test_default_accounts_path_uses_current_working_directory` | `test_default_accounts_path_uses_current_working_directory` |
| `test_load_accounts_rejects_missing_file` | `test_load_accounts_rejects_missing_file` |
| `test_load_accounts_rejects_invalid_toml` | `test_load_accounts_rejects_invalid_toml` |
| `test_load_accounts_resolves_relative_config_path` | `test_load_accounts_resolves_relative_config_path` |
| `test_parse_accounts_rejects_missing_real_paths` | `test_parse_accounts_rejects_missing_real_paths` |
| `test_parse_accounts_rejects_invalid_top_level_shapes` | `test_parse_accounts_rejects_invalid_top_level_shapes` |
| `test_parse_accounts_accepts_repository_version_alias` | `test_parse_accounts_accepts_repository_version_alias` |
| `test_parse_accounts_resolves_relative_paths_from_config_root` | `test_parse_accounts_resolves_relative_paths_from_config_root` |
| `test_parse_accounts_rejects_invalid_patient_id` | `test_parse_accounts_rejects_invalid_patient_id` |
| `test_parse_accounts_rejects_non_table_person_entry` | `test_parse_accounts_rejects_non_table_person_entry` |
| `test_parse_accounts_rejects_non_boolean_enabled` | `test_parse_accounts_rejects_non_boolean_enabled` |
| `test_parse_accounts_rejects_empty_string_fields` | `test_parse_accounts_rejects_empty_string_fields` |
| `test_parse_accounts_rejects_duplicate_patient_ids` | `test_parse_accounts_rejects_duplicate_patient_ids` |

Test Rust aggiuntivi coprono override del dizionario persona, default dei
campi, chiavi sconosciute in root/global e sezioni annidate, valori invalidi
di ricerca/ingestione/USB, asset mancanti, dizionari invalidi, UUID globali in
conflitto, UUID discordanti di sole persone disabilitate ed espansione `~`.

## Verifiche

| Comando | Esito |
| --- | --- |
| `cargo xtask check` | Exit 0 su Linux; fmt, Clippy, 48 test unitari workspace (37 core), 1 doctest, build, smoke CLI, pubblicabilità e rustdoc; 169 file pubblicabili |
| `uv run codira index`; `uv run codira cov`; `uv run codira audit` | Copertura completa di Rust/Markdown senza gap; audit senza rilievi |

`toml` 1.1.8 e `indexmap` 2.14.2 erano presenti nella cache Cargo e sono stati
risolti con Cargo offline; la toolchain fissata è Rust 1.98. Il tentativo di
aggiornare l'indice crates.io è fallito per DNS nel sandbox, quindi non è
stato usato come prova di disponibilità di rete. La documentazione API è stata
consultata tramite Context7; le dipendenze sono state compilate dal lockfile.
Il primo hook pre-commit si è fermato perché i lockfile indipendenti dei tre
prototipi GUI non includevano più le dipendenze transitivamente richieste dal
core. I tre lockfile sono stati aggiornati con `cargo check --offline` sui
manifesti dei prototipi. Il gate completo è stato poi ripetuto con gli hook
attivi e superato.
L'indice Codira Rust ha inizialmente terminato con SIGSEGV su più moduli grandi
pur compilabili; la suddivisione in moduli tematici più piccoli ha eliminato il
crash. Reindicizzazione, copertura Rust/Markdown e audit sono passati senza gap
né rilievi.
Non sono state eseguite verifiche hosted Windows/macOS in questa scheda.

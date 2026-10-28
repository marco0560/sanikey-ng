# Ricevuta P02.03 — Comparatori e controllo di sensibilità

Data: 2026-10-28. Il corpus confrontato resta quello sintetico fissato in
P02.02; baseline Python `fdad1d93e56496cc6cb9933b3089729175d5b5b3`.

## Contratto dei comparatori

Il runner locale è `cargo xtask compare <tipo> <riferimento> <candidato>`.
Le differenze o gli input non validi restituiscono un codice diverso da zero.
La directory va confrontata con una directory e un file con un file. I
confronti di directory rifiutano collegamenti simbolici e confrontano anche
l'insieme dei percorsi relativi.

| Tipo | Confronto | Normalizzazione ammessa |
| --- | --- | --- |
| `originals` | Nomi relativi e SHA-256 dei byte di ogni file; per un singolo file, SHA-256 | Nessuna normalizzazione dei byte o dei nomi |
| `sqlite` | SQL di tabelle, indici, viste e trigger; righe e valori SQLite tipizzati; `integrity_check`, `foreign_key_check` e query FTS. Le righe sono confrontate in streaming come multinsieme, quindi i duplicati sono conservati | Ordine fisico di tabelle e righe ignorato. Le tabelle ombra interne FTS sono escluse dal confronto schema/dati fisici; la tabella virtuale logica, i suoi rowid e le sue ricerche sono inclusi |
| `json` | Struttura JSON e valori; oggetti indipendenti dall'ordine delle chiavi | Spaziatura e ordine delle chiavi previsti dal formato JSON; nessun campo viene scartato e i tipi numerici/stringa restano distinti |
| `text` | Testo UTF-8 completo | `CRLF` convertito in `LF`; non vengono eliminati spazi, righe vuote o differenze Unicode |
| `frontend` | Stesso insieme di percorsi relativi; JSON strutturale, file testuali come `text`, altri file tramite SHA-256 | Solo le normalizzazioni applicabili al formato del singolo file |

Per ogni tabella FTS il confronto SQLite richiede almeno un `--query <testo>` e
confronta l'elenco ordinato per `rank`, poi `rowid`. Si registrano query
rappresentative del caso in verifica: ciò confronta quei risultati e non prova
la completezza generale della ricerca. Le definizioni SQL sono confrontate
testualmente; anche differenze di formattazione DDL richiedono verifica invece
di essere normalizzate automaticamente.

I timestamp generativi, i percorsi assoluti e i metadati filesystem specifici
della macchina restano omissioni dichiarate dal produttore del report, come
nel corpus P02.02. Il comparatore JSON non li rimuove: un nuovo campo non
previsto fallisce il confronto. Le date, i valori, le unità, i riferimenti e la
provenienza nei dati confrontati restano significativi.

Il controllo SQLite apre entrambi i database in sola lettura. Il frontend è
confrontato come albero di file; questo controllo non simula un browser e non
qualifica l'apertura `file://`, prevista dalla scheda frontend.

## Prove

I moduli `xtask/src/compare/` aggiungono test per ogni tipo. Le mutazioni
sintetiche cambiano byte e percorsi degli originali, valori JSON inclusi data,
unità e provenienza, contenuto testuale, dati e vincoli SQLite, risultati FTS
e file/dati frontend. Il comando SQLite è esercitato sul database sintetico
generato in P02.02. La suddivisione in moduli mantiene l'indicizzazione Codira
stabile: la prima versione concentrata in un solo file provocava un crash del
parser Rust; dopo la divisione l'indice si completa senza errori.

| Comando | Esito |
| --- | --- |
| `cargo test --package xtask --locked` | Exit 0; 8 test superati |
| `cargo xtask compare originals docs/migration/corpus/minimal/patient-a/documents docs/migration/corpus/minimal/patient-a/documents` | Exit 0 |
| `cargo xtask compare sqlite .artifacts/migration/p02-02-build/patient-a/database/medical_archive.db .artifacts/migration/p02-02-build/patient-a/database/medical_archive.db --query sintetico` | Exit 0; query FTS inclusa |
| `cargo xtask compare json docs/migration/corpus/expected/minimal-python.json docs/migration/corpus/expected/minimal-python.json` | Exit 0 |
| `cargo xtask compare text 'docs/migration/corpus/minimal/patient-a/documents/laboratory/20260102 Referto Sintetico.txt' 'docs/migration/corpus/minimal/patient-a/documents/laboratory/20260102 Referto Sintetico.txt'` | Exit 0 |
| `cargo xtask compare frontend .artifacts/migration/p02-02-build/patient-a/web .artifacts/migration/p02-02-build/patient-a/web` | Exit 0 |
| `cargo xtask compare json <expected> <mutated>` dopo `value: 70.0 -> 71.0` | Exit 1; differenza strutturale rilevata |
| `cargo xtask check` | Exit 0 su Linux dopo la suddivisione in moduli: fmt, Clippy, 16 test unitari, 1 doctest, build, smoke CLI, pubblicabilità e rustdoc |
| Cinque comandi `cargo xtask compare` (originals, SQLite, JSON, testo, frontend) | Exit 0 su fixture sintetiche; per SQLite inclusa la query FTS `sintetico` |
| `cargo xtask compare json <expected> <mutated>` dopo `value: 70.0 -> 71.0` | Exit 1; differenza strutturale rilevata |
| `uv run codira index`; `uv run codira cov`; `uv run codira audit` | 0 errori, 0 file falliti, copertura completa su 96 file, nessun gap e nessun problema rustdoc |
| `git diff --check`; `python3 scripts/check_publishable.py` | Exit 0; 144 file esaminati |

Il primo tentativo `cargo check --package xtask` non ha risolto
`index.crates.io` nel sandbox; con l'accesso di rete autorizzato sono state
scaricate le dipendenze e il codice compilato. I primi controlli hanno poi
rilevato errori di tipo, una formattazione non aggiornata e un lint
`filter_map_bool_then`; sono stati corretti. Il gate completo successivo è
terminato con exit 0. Anche il controllo mutation CLI sopra usa una copia
temporanea sotto `.Temp`; nessun expected versionato è stato riscritto.

Il riepilogo delle policy è in `[comparison]` di `parity.toml`; questa ricevuta
ne documenta il dettaglio e i limiti. La matrice delle capability resta in
stato `pending`: la disponibilità del comparatore non dimostra la parità di
alcuna capability Rust. Le verifiche sono state eseguite localmente su Linux;
la CI hosted multipiattaforma non è
stata rilanciata da questa scheda.

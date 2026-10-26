# Corpus sintetico di migrazione

Versione del corpus: 1. Origine primaria: esempi pubblici dichiarati fittizi
nel commit baseline Python
`fdad1d93e56496cc6cb9933b3089729175d5b5b3`. Il corpus è solo sviluppo/test;
non viene incluso nei pacchetti installati.

- `minimal/`: input leggibili e copiabili, configurazione, metadati curati,
  CSV con data e unità, testo datato e un duplicato a byte identici.
- `negative/`: input malformati con esito d’errore atteso.
- `complex/`: catalogo delle fixture adattatore generate dai test Python
  sintetici; separa formati che richiedono provider o parser specifici.
- `expected/`: proiezioni deterministiche degli output baseline usate nel
  confronto, senza database binari o dati macchina-specifici.
- `fixture-manifest.toml`: versione, provenienza, formati e SHA-256 degli input.

Per ripetere il run iniziale, usare il checkout baseline qualificato e la
configurazione in `config/accounts.toml`. La `local_build` punta a una directory
locale ignorata sotto `.artifacts/migration`; importazione e build scrivono
solo lì e nella copia di lavoro dei metadati, mai sugli originali.

Tutte le date e quantità sono sintetiche. La copia del referto non aggiunge un
documento differente: serve a verificare che i duplicati siano rilevati e non
esportati due volte. Gli hash attestano i byte dei fixture versionati, non
l’autenticità di documenti clinici.

# Esempi di Pazienti

Questa directory contiene una struttura paziente interamente sintetica. Copiare
solo i file necessari nella propria `metadata_directory` e sostituire valori,
identificativi e percorsi con quelli curati localmente.

`patient-a/metadata/` contiene un esempio per ogni metadato compilabile:

- `clinical_summary.toml`, `document_tags.toml`, `problems.toml`,
  `medications.toml`, `therapies.toml`, `procedures.toml`,
  `observations.toml` e `timeline_events.toml` sono letti direttamente dalla
  build;
- `observation_imports.toml` è un manifesto compilabile separato, mantenuto
  fuori dal paziente dimostrativo perché richiede di eseguire
  `import-observations`; `metadata/observations/` è invece generata dal comando
  e non va modificata manualmente;
- `parameters.toml` è un modello separato per discovery e regole longitudinali;
  predisporre prima il file comune come descritto nel
  [riferimento parametri](https://github.com/marco0560/sanikey/blob/fdad1d93e56496cc6cb9933b3089729175d5b5b3/docs/process/metadata-toml-reference.md#parameterstoml),
  poi copiarlo nella `metadata_directory` soltanto dopo aver configurato il termine
  corrispondente nel dizionario. Le regole restano disabilitate finché non sono
  state revisionate.

`medication_leaflets.toml` è scritto da `resolve-medication-leaflets`, quindi
non è un modello da copiare; l'esempio include solo uno stato sintetico
`non_aifa` necessario a rendere costruibile il paziente dimostrativo. Tutti gli
esempi sono fittizi e non clinici.

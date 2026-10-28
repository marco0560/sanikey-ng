# ADR-0003: semantica dell'identità USB

- Stato: accettata dal manutentore (G2).
- Proposta: 2026-10-28; approvazione: 2026-10-28.
- Evidenza: [fattibilità P04.04](../migration/verification/2026-10-26-volume-identity-feasibility.md).

## Contesto

La baseline usa `usb_uuid` per confrontare l'identificatore filesystem del
target fisico con quello osservato sul volume montato. La documentazione aveva
anche descritto lo stesso valore come possibile ID di deploy e lo esportava
nel manifest e in `usb-info`. Questi significati non sono intercambiabili.
Linux, Windows e macOS espongono identificatori diversi; non è stata dimostrata
una conversione comune tra UUID filesystem Linux, volume GUID Windows e
VolumeUUID macOS.

## Decisione

`usb_uuid` mantiene il significato della baseline: identifica il filesystem o
volume target usato dalla verifica d'identità. Non rappresenta un ID di deploy.
Gli export devono etichettarlo con questo significato e non presentarlo come
identificatore universale quando il dato proviene da un identificatore nativo
diverso.

Se in futuro il prodotto richiederà un ID stabile di deploy, sarà un campo
distinto, per esempio `deployment_id`, con una decisione e una verifica proprie.
Gli adattatori per Linux, Windows e macOS devono esporre chiaramente il tipo di
identità disponibile; non devono equiparare UUID filesystem, volume GUID,
seriale e VolumeUUID senza una regola verificata. La verifica multipiattaforma
resta da implementare e qualificare.

Il manutentore ha approvato questa proposta G2 il 2026-10-28. L'approvazione
risolve l'ambiguità contrattuale; non dichiara completata la capability USB né
la sua verifica su dispositivi reali o sui tre sistemi operativi.

## Conseguenze e verifiche necessarie

- Conservare il campo `usb_uuid` compatibile con il significato della baseline.
- Mantenere separati l'identità del target e l'eventuale ID di deploy in schema,
  manifest, diagnostica e UI.
- Prima della qualifica USB multipiattaforma, verificare gli adattatori nativi,
  i casi di identificatore non disponibile e il comportamento di mismatch.
- Non dichiarare supporto multipiattaforma sulla sola base di fixture Linux.

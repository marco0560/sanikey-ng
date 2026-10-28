# Ricevuta P04.04 — Identità e mount dei volumi

Data: 2026-10-26. Prova runtime solo su Linux x86_64 con una directory
temporanea in `.Temp`; nessun dispositivo USB reale è stato letto o scritto.
Le fonti Microsoft e Apple sono state consultate per documentare gli
adattatori possibili, senza dichiarare test Windows o macOS.

## Contratto della baseline

Baseline fissata: `fdad1d93e56496cc6cb9933b3089729175d5b5b3`. Prima di copiare
verso un target non coincidente con l'immagine USB generata, la baseline crea
il target se manca, misura byte richiesti e spazio libero, poi invoca
`findmnt --json --output TARGET,FSTYPE,UUID --target <path>`. Confronta
l'eventuale UUID configurato con `UUID` di `findmnt`, può imporre `exfat` e
riconosce come probabili target rimovibili Linux soltanto i percorsi sotto
`/media` o `/run/media`. Se `findmnt` non è disponibile, il controllo fallisce
solo quando è stato richiesto un UUID o exFAT.

`_validate_physical_target` crea il percorso target se manca. Per questo la
prova locale ha chiamato solo la lettura mount/spazio su un percorso temporaneo
già esistente e non ha invocato il validatore su un dispositivo fisico.

La configurazione non ha ancora una semantica uniforme documentata:
`UsbConfig.usb_uuid` è descritto nel codice come UUID filesystem per target
fisici; `docs/architecture.md` lo definisce come UUID filesystem **oppure** ID
di deploy. Lo stesso campo viene anche esportato nel manifest e in `usb-info`.
Questa ambiguità va risolta al checkpoint G2 prima di implementare una
validazione portabile; non ho convertito o equiparato identificatori diversi.

## Prove eseguite

La suite baseline `tests/test_usb.py`, con
`SANIKEY_USB_INTEGRATION_TARGET` rimosso dall'ambiente, ha dato `20 passed,
1 skipped` in 3,09 s. Lo skip è il test opzionale su target filesystem reale.
La suite copre la verifica del mismatch UUID con informazioni filesystem
simulate, ma non prova dispositivi o API di altri sistemi operativi.

Su una directory temporanea Linux già esistente, `findmnt` 2.42.3 ha
restituito le tre chiavi attese (`fstype`, `uuid`, `target`) e
`shutil.disk_usage()` ha letto spazio disponibile. I valori effettivi del
filesystem e dell'identificatore non sono stati stampati o conservati. Il
probe è in sola lettura rispetto al filesystem montato; la directory di prova
è stata creata e rimossa esclusivamente in `.Temp`.

Per ripetere la suite fissata, usare il checkout baseline indicato e il suo
venv di sviluppo. Il target di integrazione reale deve essere non impostato:

```bash
BASELINE_ROOT=<checkout al commit fdad1d93e56496cc6cb9933b3089729175d5b5b3>
PYTHON_REPO_ROOT=<checkout Python con venv di sviluppo>
TEMP_ROOT=<area temporanea autorizzata>
cd "$BASELINE_ROOT"
env -u SANIKEY_USB_INTEGRATION_TARGET \
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH="$BASELINE_ROOT/src" \
TMPDIR="$TEMP_ROOT" \
"$PYTHON_REPO_ROOT/.venv/bin/pytest" -p no:cacheprovider -q tests/test_usb.py
```

Esito atteso: `20 passed, 1 skipped`; lo skip conferma che non è stato
configurato un target fisico per il test opzionale.

## Adattatori per sistema operativo

| Sistema | Identità filesystem e mount | Spazio e filesystem | Prova |
| --- | --- | --- | --- |
| Linux | `findmnt` espone `UUID` e `TARGET`; il `UUID` è l'identificatore filesystem usato dalla baseline. | `FSTYPE`, `shutil.disk_usage` | Lettura eseguita su directory temporanea del filesystem host; nessuna USB reale. |
| Windows | `GetVolumeNameForVolumeMountPointW` restituisce un volume GUID path; `GetVolumeInformationW` restituisce anche filesystem e volume serial assegnato alla formattazione. La lettera unità può cambiare. Volume GUID, serial e UUID Linux sono identificatori distinti. | `GetVolumeInformationW` per filesystem; `GetDiskFreeSpaceExW` per spazio; `GetVolumePathNameW` per mount path. | Fonti Microsoft soltanto; nessun test runtime. |
| macOS | Disk Arbitration espone `VolumeUUID`, `VolumePath` e nome BSD del device. Apple avverte che non tutte le chiavi sono disponibili per tutti i dispositivi: il matching va provato su supporti diversi. | Disk Arbitration per metadati; API filesystem come `statvfs` o candidato `sysinfo` per capacità. | Fonti Apple soltanto; nessun test runtime. |

`sysinfo 0.39.6` documenta API comuni per nome, filesystem, mount point,
spazio totale/disponibile e flag rimovibile, e dichiara supporto Linux, Windows
e macOS. `Disk` non espone un UUID filesystem; da solo non sostituisce
`findmnt` o gli adattatori nativi per il confronto d'identità. Il suo MSRV
dichiarato è Rust 1.95, compatibile con la toolchain del repo 1.98.1; la
dipendenza è candidata e non è stata aggiunta o compilata.

Fonti ufficiali consultate: [util-linux `findmnt`](https://man7.org/linux/man-pages/man8/findmnt.8.html),
[Microsoft `GetVolumeInformationW`](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumeinformationw),
[Microsoft volume GUID path](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumenameforvolumemountpointw),
[Microsoft spazio libero](https://learn.microsoft.com/en-us/windows/desktop/api/fileapi/nf-fileapi-getdiskfreespaceexw),
[Microsoft mount path](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumepathnamew),
[Apple Disk Arbitration Volume UUID](https://developer.apple.com/documentation/diskarbitration/kdadiskdescriptionvolumeuuidkey),
[Apple Disk Arbitration e disponibilità delle chiavi](https://developer.apple.com/library/archive/documentation/DriversKernelHardware/Conceptual/DiskArbitrationProgGuide/ManipulatingDisks/ManipulatingDisks.html),
[sysinfo 0.39.6 `Disk`](https://docs.rs/sysinfo/0.39.6/sysinfo/struct.Disk.html),
[sistemi supportati da sysinfo](https://docs.rs/crate/sysinfo/0.39.6/source/README.md).
Context7 è stato consultato per sysinfo; l'API e il MSRV sono stati verificati
nelle fonti ufficiali versionate.

Il primo probe ha invocato il Python host senza il `PYTHONPATH` del checkout
baseline e ha terminato con `ModuleNotFoundError`. È stato ripetuto con il venv
e il `PYTHONPATH` fissati: lettura mount e spazio superata. Una verifica
separata dello stato CI hosted non è riuscita per errore di rete GitHub API;
non è stata dichiarata qualificazione multipiattaforma.

## Decisione G2 successiva

Il manutentore ha approvato il 2026-10-28 la semantica proposta in
[ADR-0003](../../decisions/0003-identita-usb.md): `usb_uuid` resta l'identità
filesystem/volume usata dalla baseline; un eventuale ID di deploy è separato.
Questa decisione risolve l'ambiguità documentale, ma non implementa né
qualifica gli adattatori USB per Windows/macOS o dispositivi reali.

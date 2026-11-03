# Ricevuta P10.03 — Adattatore Windows dei volumi

Data: 2026-11-03. Implementati il crate isolato `sanikey-windows-volume` per le
chiamate Win32 e `sanikey_core::windows_volume` per l'applicazione della
configurazione. L'FFI è limitato al modulo `ffi`; il crate vieta `unsafe` fuori
da quel confine e il core mantiene `#![forbid(unsafe_code)]`.

La specifica approvata il 2026-11-03 aggiunge l'ADR-0003: su Windows
`usb_uuid` contiene il Volume GUID path di
`GetVolumeNameForVolumeMountPointW`, confrontato letteralmente. Il GUID resta
un'identità Windows distinta dall'UUID Linux e dal seriale restituito da
`GetVolumeInformationW`. L'adattatore legge anche filesystem e spazio libero;
non crea directory né scrive sul target.

## Verifiche disponibili su Linux

- `cargo test -p sanikey-core windows_volume`: 3 test superati. Fixture
  sintetiche coprono precedenza della configurazione globale, valore condiviso
  delle persone abilitate, confronto letterale del GUID, mismatch, filesystem
  exFAT e spazio insufficiente.
- `cargo test -p sanikey-windows-volume`: 1 test superato per output UTF-16
  terminato correttamente e rifiuto di buffer/tracce malformate.
- `cargo clippy -p sanikey-windows-volume --all-targets -- -D warnings`: exit 0.
- `CARGO_NET_OFFLINE=true cargo xtask check`: exit 0. Clippy, build, smoke CLI,
  test workspace (192 core passati, 2 ignored, 8 xtask, 3 CLI, 1 Windows-volume
  crate), doctest (2), pubblicabilità (281 file) e rustdoc superati.
- I tre prototipi GUI sono stati ricontrollati con `cargo check`; i rispettivi
  lockfile includono il nuovo crate path e `windows-sys` dove richiesto.
- `uv run codira index`: exit 0, 7 indicizzati, 209 riusati, 0 errori; i 12
  tipi non supportati sono gli stessi già attesi.

La compilazione del target Windows non è riuscita perché questo host Linux non
ha installato `x86_64-pc-windows-gnu` (`can't find crate for core`); `rustup`,
Wine, QEMU e `virsh` non sono disponibili. Il ramo FFI Windows non è quindi
stato compilato né eseguito su Windows o VM. Nessun volume Windows o suo GUID
reale è stato letto. Il criterio di accettazione P10.03 richiede Windows reale
o VM idonea: la scheda resta incompleta finché tale qualifica non sarà eseguita.

Il commit funzionale `12d8f27` (`feat(core): add Windows volume adapter`) ha
superato gli hook pre-commit: diff staged, formattazione e test del workspace,
più formattazione/test dei tre prototipi GUI. Il protocollo GUI ha passato 1
test; egui e Slint non hanno test unitari ma sono stati compilati.

## Fonti API

Le chiamate FFI usano `windows-sys 0.61.2`, verificato nel sorgente Microsoft
del crate e con le firme Win32 documentate:
[Volume GUID path](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumenameforvolumemountpointw),
[filesystem](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumeinformationw),
[mount path](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumepathnamew),
[spazio libero](https://learn.microsoft.com/en-us/windows/desktop/api/fileapi/nf-fileapi-getdiskfreespaceexw).

Il primo test del crate FFI fallì per un import `std::io` limitato alla cfg
Windows mentre i test UTF-16 vengono eseguiti anche su Linux (`E0433`). L'import
è stato esteso a `cfg(any(windows, test))` e il test del crate è poi passato.

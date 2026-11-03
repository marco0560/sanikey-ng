# Ricevuta P10.04 — Adattatore macOS dei volumi

Data: 2026-11-03. Aggiunti `sanikey-macos-volume` e
`sanikey_core::macos_volume`. Il crate isola `statfs` dietro un modulo FFI
ristretto; legge filesystem, mount e spazio senza scrivere sul target, quindi
interroga `/usr/sbin/diskutil info -plist` per il VolumeUUID nativo.

Il parser conserva `VolumeUUID` come identità macOS distinta dall'UUID Linux e
dal Volume GUID Windows. Se macOS non espone un UUID persistente, il valore
resta assente: la verifica è ammessa solo quando la configurazione non richiede
un'identità. Il validatore confronta l'identità configurata letteralmente,
controlla exFAT e applica il margine minimo di spazio. Nessuna etichetta viene
usata per identificare il dispositivo.

## Verifiche su Linux

- `cargo test -p sanikey-macos-volume`: 2 test sintetici superati per estrazione
  del VolumeUUID da plist, valore assente e plist malformata.
- `cargo test -p sanikey-core macos_volume::tests`: 4 test sintetici per
  precedenza globale, confronto letterale, VolumeUUID mancante, filesystem e
  spazio insufficiente.
- `cargo xtask check`: exit 0; Clippy, test workspace (196 core, 2 ignored,
  8 xtask, 3 CLI, 2 macOS-volume, 1 Windows-volume), 2 doctest, CLI,
  pubblicabilità (286 file) e rustdoc superati.
- `cargo check` dei tre workspace GUI: exit 0; lockfile aggiornati.
- `uv run codira index`: exit 0, 1 indicizzato, 219 riusati, 0 errori; i 12
  tipi non supportati sono quelli già attesi.
- Il target `aarch64-apple-darwin` non è installato: `cargo check -p
  sanikey-macos-volume --target aarch64-apple-darwin --offline` termina con
  `can't find crate for core`. Non è disponibile un host macOS, quindi il crate
  FFI non è stato compilato né eseguito su macOS.

La qualifica runtime macOS, inclusa la rimozione del volume durante il flusso,
resta pendente. I test sintetici Linux non dimostrano la correttezza delle
strutture `statfs`, del formato effettivo del plist o del comportamento di
`diskutil` su un volume macOS.

## Fonti API

- [Apple Foundation: VolumeUUID persistente](https://developer.apple.com/documentation/foundation/urlresourcekey/volumeuuidstringkey)
  documenta che il valore può essere assente.
- [Manuale `diskutil(8)`](https://keith.github.io/xcode-man-pages/diskutil.8.html):
  `info -plist` emette una property list e accetta il mount point come device.
- [Context7: `libc` Apple `statfs`](https://github.com/rust-lang/libc/blob/main/_autodocs/api-reference/bsd.md)
  conferma la disponibilità di `statfs` sui target Apple; la definizione locale
  installata di `libc 0.2.191` fornisce i campi usati dal crate.

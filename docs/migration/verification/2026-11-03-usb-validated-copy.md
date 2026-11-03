# Ricevuta P10.05 — Copia USB verificata e ripresa

Data: 2026-11-03. Aggiunti `mirror_validated_txt_image` e il flusso Linux
`mirror_validated_txt_image_on_linux_volume`. Il primo convalida manifest,
checksum e link dell'immagine TXT prima e dopo la copia; un'immagine sorgente
invalida non modifica il target. Il target alterato viene riallineato dal
manifest valido e la validazione finale precede l'esito `Succeeded`.

Su Linux il flusso richiede una directory target già esistente, calcola la
dimensione dell'immagine, verifica prima del trasferimento spazio, mount,
filesystem e identità configurata; dopo la copia e la validazione rilegge il
volume e confronta mount, filesystem e UUID. Gli eventi mantengono separati
elaborazione e validazione finale. Annullamento e copia incompleta non
producono un esito positivo e la stessa immagine può essere rilanciata.

## Verifiche su Linux

- `cargo test -p sanikey-core usb_copy::tests`: 8 test superati, inclusi
  validazione prima/dopo, riparazione del target alterato, rifiuto di una
  sorgente corrotta senza modifiche al target, annullamento e preflight del
  target inesistente.
- `cargo xtask check`: gate Rust/documentale completo superato; numeri e file
  pubblicabili registrati nel ledger: 199 core passati, 2 ignored, 8 xtask,
  3 CLI, 2 test crate macOS-volume, 1 Windows-volume, 2 doctest; pubblicabilità
  verificata su 287 file e rustdoc superato.
- `uv run codira index`: exit 0, 4 indicizzati, 220 riusati, 0 errori; i 12
  tipi non supportati sono quelli già attesi.
- I target dei test sono directory sintetiche sul filesystem dell'host; nessuna
  chiavetta o altro volume fisico è stato usato.
- La baseline Python `src/sanikey/usb.py` e `tests/test_usb.py` sono state lette
  come richiesto dalla scheda, senza modificarle né rieseguire la suite.

## Limiti

La verifica implementata copre il sottoinsieme TXT simulato, non l'intero
contratto `validate_usb` Python (export multipersona, leaflets e tutte le
strutture frontend). Il cambio di mount è rilevato dal controllo finale, ma non
è stata provata la rimozione durante una copia su dispositivo reale. Windows,
macOS e qualifica USB fisica restano pending. La capability USB non è dichiarata
in parità.

## Prossima scheda

P11.01 — creare la GUI egui/eframe approvata, secondo
[la scheda](../tasks/P11.01-creare-la-gui-e-i-profili.md).

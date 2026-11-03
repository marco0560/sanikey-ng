# P11.03 — Export USB e risultato desktop

Data: 2026-11-03. Implementazione e verifiche automatizzate su Linux.

## Modifica

- `usb_image::assemble_usb_image` assembla un'immagine completa per tutte le persone abilitate: database, frontend offline, metadati USB, documenti filtrati, leaflet, report, file tecnici, media/anteprime e viewer DICOM disponibili. Staging, manifest SHA-256 e validazione precedono la finalizzazione.
- `validate_usb_image` verifica i file richiesti, i link locali, la corrispondenza completa fra albero e manifest, gli checksum, le radici non-symlink e gli identificatori persona. Rifiuta build mancanti o duplicate.
- La GUI conserva i risultati build, prepara l'immagine su thread worker, permette di scegliere un volume Linux, mostra mount/filesystem/UUID/spazio disponibile e richiede conferma esplicita. La sincronizzazione si limita alla cartella `SaniKey` nel volume, conserva gli altri file alla radice, segnala la rimozione degli elementi obsoleti della cartella applicativa e mostra copiati/riutilizzati/rimossi/byte più l'esito della validazione.
- La copia ricontrolla spazio, mount e identità prima e dopo il trasferimento e valida manifest, link e checksum sul target. Le immagini e i volumi di prova sono sintetici.

## Verifiche

- `cargo test -p sanikey-core usb_copy::tests --offline`: 8/8 passano.
- `cargo test -p sanikey-core usb_image::tests --offline`: 2/2 passano; include assemblaggio e copia di due persone e l'adattatore Linux verso la cartella `SaniKey` su volume sintetico, oltre a rifiuti per target già esistente, build mancante/duplicata e file alterati/non registrati.
- `cargo test -p sanikey-desktop --offline`: 9/9 passano.
- `cargo clippy -p sanikey-core -p sanikey-desktop --all-targets --offline -- -D warnings`: exit 0.
- `CARGO_NET_OFFLINE=true cargo xtask check`: exit 0; 203 test core con 2 ignorati, 3 CLI, 9 desktop, 1 Windows-volume, 2 macOS-volume, 8 xtask, 2 doctest, build/smoke CLI, pubblicabilità 294 file e rustdoc.
- `uv run codira index`: 3 indicizzati, 225 riusati, 0 fallimenti. L'audit non ha rilievi sui nuovi moduli USB o sul desktop; segnala i 12 formati senza parser e rilievi Rustdoc già presenti in altri moduli.
- La GUI non è stata provata interattivamente: `weston` è installato ma il modulo backend headless richiesto (`headless-backend.so`) manca; l'ambiente non espone un display X11 o compositor Wayland. Nessun volume USB fisico è stato scritto o qualificato.
- Tentativi intermedi registrati: la prima separazione dei validatori applicava quello dell'immagine completa ai fixture TXT e ha fallito 4 test preesistenti; la compatibilità TXT è stata ripristinata con API distinte. Clippy ha segnalato un borrow non necessario, corretto prima del gate finale.

## Limiti e prossimo passo

P11.03 è implementata e validata con fixture sintetiche su Linux, ma il criterio dell'operatore senza terminale attende una prova grafica manuale. Il supporto di copia USB nel desktop è per ora Linux; Windows e macOS, l'hardware USB fisico e la parità completa con il generatore Python restano da qualificare. Proseguire con P11.04 per la prova GUI quando sarà disponibile un display Linux; poi il manutentore eseguirà i test Windows.

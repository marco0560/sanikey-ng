# Ricevuta P03.03 — Gate Rust e CI multipiattaforma

Data: 2026-10-26. Toolchain locale `rustc`/Cargo 1.98.1 su Linux; Python
locale 3.14.10. La CI usa Python 3.13.

`cargo xtask check` verifica la toolchain fissata, fmt, Clippy con warning
bloccanti, test di library/bin, doctest separati, build workspace, smoke test
CLI, link Markdown locali, TOML in `docs/migration/`, percorsi pubblicabili e
rustdoc con warning bloccanti. Il controllo documentale usa `tomllib` e Git
via Python 3.13 standard library; Python non entra nel prodotto installato.

Il workflow `.github/workflows/ci.yml` definisce job separati su
`ubuntu-latest`, `windows-latest` e `macos-latest`, con toolchain 1.98.1,
componenti clippy/rustfmt e Python 3.13.

## Esiti

- `python3 scripts/check_publishable.py`: exit 0; 116 percorsi visibili a Git,
  link locali, documenti obbligatori, TOML e denylist verificati.
- `cargo xtask check`: exit 0 su Linux; 9 unit test e 1 doctest superati,
  build, Clippy, rustdoc e smoke test CLI superati.
- `git diff --check`: exit 0.
- Run GitHub Actions [40404493580](https://github.com/marco0560/sanikey-ng/actions/runs/40404493580),
  commit `6a671e1d674b77a779770d2d947e720a743f480a`: `ubuntu-latest`,
  `windows-latest` e `macos-latest` completati con successo.

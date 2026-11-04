#!/usr/bin/env python3
"""Synchronize developer-local SaniKey data from the Dropbox archive.

Parameters
----------
None

Returns
-------
None
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CONFIG_PATH = REPO_ROOT / "config" / "accounts.toml"
LOCAL_DATA = REPO_ROOT / "local-data"
DEFAULT_SOURCE_ROOT = Path.home() / "Dropbox" / "Family Room" / "Medicalia"
PERSON_ID = re.compile(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*\Z")


@dataclass(frozen=True)
class PersonPaths:
    """Configured local destinations and source archive identity.

    Parameters
    ----------
    person_id : str
        Lowercase account identifier from ``[[person]]``.
    enabled : bool
        Whether this person participates in reload operations.
    source_documents : pathlib.Path
        Local destination for source documents.
    metadata_directory : pathlib.Path
        Local destination for metadata.
    local_build : pathlib.Path
        Local generated build directory.

    Returns
    -------
    None
    """

    person_id: str
    enabled: bool
    source_documents: Path
    metadata_directory: Path
    local_build: Path


def main(argv: list[str] | None = None) -> int:
    """Synchronize, compare, or refresh configured local data.

    Parameters
    ----------
    argv : list[str] | None, optional
        Command-line arguments without the executable name. Uses
        :data:`sys.argv` when omitted.

    Returns
    -------
    int
        Zero on success, one when comparison finds differences, and two for
        an operational or configuration error.
    """

    args = _parse_args(argv)
    try:
        config_path = args.config.expanduser().resolve()
        people = _load_people(config_path)
        enabled_people = tuple(person for person in people if person.enabled)
        _validate_destinations(enabled_people, LOCAL_DATA)

        if args.refresh:
            _refresh_generated(enabled_people)
        else:
            source_root = args.source_root.expanduser().resolve()
            pairs = _sync_pairs(enabled_people, source_root)
            _validate_sources(pairs, LOCAL_DATA)
            _require_rsync()
            if args.compare:
                return _compare(pairs)
            _synchronize_local_data(enabled_people, pairs)
    except (OSError, subprocess.SubprocessError, ValueError) as exc:
        print(f"ERRORE: ricaricamento dati non riuscito: {exc}", file=sys.stderr)
        return 2

    mode = "build-rigenerate" if args.refresh else "sincronizzazione"
    print(f"stato=ok persone={len(enabled_people)} modalità={mode}")
    return 0


def _parse_args(argv: list[str] | None) -> argparse.Namespace:
    """Parse reload modes and local path overrides.

    Parameters
    ----------
    argv : list[str] | None
        Command-line arguments without the executable name.

    Returns
    -------
    argparse.Namespace
        Selected mode, configuration path, and source archive root.

    Raises
    ------
    SystemExit
        If command-line arguments are invalid or help is requested.
    """

    parser = argparse.ArgumentParser(
        description="Aggiorna le directory locali configurate in SaniKey NG."
    )
    modes = parser.add_mutually_exclusive_group(required=True)
    modes.add_argument(
        "--sync",
        action="store_true",
        help=(
            "sincronizza documenti e metadati; elimina gli obsoleti solo nelle "
            "destinazioni configurate"
        ),
    )
    modes.add_argument(
        "--refresh",
        action="store_true",
        help=(
            "ricrea solo le directory di build configurate, preservando "
            "documenti e metadati"
        ),
    )
    modes.add_argument(
        "--compare",
        action="store_true",
        help=(
            "confronta sorgenti e destinazioni senza modificare file; esce con "
            "codice 1 se trova differenze"
        ),
    )
    parser.add_argument(
        "--config",
        type=Path,
        default=CONFIG_PATH,
        help="profilo account TOML (default: config/accounts.toml)",
    )
    parser.add_argument(
        "--source-root",
        type=Path,
        default=DEFAULT_SOURCE_ROOT,
        help="radice dell'archivio Dropbox Medicalia",
    )
    return parser.parse_args(argv)


def _load_people(config_path: Path) -> tuple[PersonPaths, ...]:
    """Load enabled-person paths from the Rust repository TOML schema.

    Parameters
    ----------
    config_path : pathlib.Path
        Path to the ``[[person]]`` account configuration.

    Returns
    -------
    tuple[PersonPaths, ...]
        People in configuration order, including disabled entries.

    Raises
    ------
    OSError
        If the configuration cannot be opened.
    ValueError
        If the TOML structure or required account fields are invalid.
    """

    with config_path.open("rb") as stream:
        data = tomllib.load(stream)
    global_config = data.get("global")
    if not isinstance(global_config, dict):
        raise ValueError("la configurazione deve contenere la tabella [global]")
    version = global_config.get(
        "config_version", global_config.get("repository_version")
    )
    if not isinstance(version, int) or isinstance(version, bool):
        raise ValueError("[global].config_version deve essere un intero")
    raw_people = data.get("person")
    if not isinstance(raw_people, list) or not raw_people:
        raise ValueError("la configurazione deve contenere almeno una voce [[person]]")

    config_root = (
        config_path.parent.parent
        if config_path.parent.name == "config"
        else config_path.parent
    )
    people: list[PersonPaths] = []
    seen_ids: set[str] = set()
    for index, raw_person in enumerate(raw_people):
        if not isinstance(raw_person, dict):
            raise ValueError(f"[[person]] {index}: attesa una tabella TOML")
        person_id = _string_field(raw_person, "id", index)
        if PERSON_ID.fullmatch(person_id) is None:
            raise ValueError(f"[[person]] {index}: id non valido")
        if person_id in seen_ids:
            raise ValueError(f"id persona duplicato in configurazione: {person_id}")
        seen_ids.add(person_id)

        enabled = raw_person.get("enabled", True)
        if not isinstance(enabled, bool):
            raise ValueError(f"[[person]] {index}: enabled deve essere booleano")
        people.append(
            PersonPaths(
                person_id=person_id,
                enabled=enabled,
                source_documents=_resolve_path(
                    _string_field(raw_person, "source_documents", index), config_root
                ),
                metadata_directory=_resolve_path(
                    _string_field(raw_person, "metadata_directory", index), config_root
                ),
                local_build=_resolve_path(
                    _string_field(raw_person, "local_build", index), config_root
                ),
            )
        )

    if not any(person.enabled for person in people):
        raise ValueError("nessuna persona abilitata nella configurazione account")
    return tuple(people)


def _string_field(person: dict[str, object], field: str, index: int) -> str:
    """Read a required non-empty string from one person table.

    Parameters
    ----------
    person : dict[str, object]
        Parsed TOML table for one ``[[person]]`` entry.
    field : str
        Required string field name.
    index : int
        Zero-based person index for diagnostics.

    Returns
    -------
    str
        Field value.

    Raises
    ------
    ValueError
        If the field is missing, empty, or not a string.
    """

    value = person.get(field)
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"[[person]] {index}: {field} deve essere una stringa non vuota")
    return value


def _resolve_path(value: str, base: Path) -> Path:
    """Resolve one configured path using the Rust account-file base rules.

    Parameters
    ----------
    value : str
        Absolute or profile-relative path string.
    base : pathlib.Path
        Repository root for a config under ``config/``, otherwise the config
        file's parent directory.

    Returns
    -------
    pathlib.Path
        Expanded, absolute path without requiring the destination to exist.
    """

    path = Path(value).expanduser()
    return (path if path.is_absolute() else base / path).resolve(strict=False)


def _validate_destinations(people: tuple[PersonPaths, ...], local_data: Path) -> None:
    """Require every managed destination to be isolated under local-data.

    Parameters
    ----------
    people : tuple[PersonPaths, ...]
        Configured enabled people.
    local_data : pathlib.Path
        Private data root managed by this utility.

    Returns
    -------
    None

    Raises
    ------
    ValueError
        If destinations escape local-data or overlap one another.
    """

    repo_root = REPO_ROOT.resolve()
    data_root = local_data.resolve(strict=False)
    if not data_root.is_relative_to(repo_root) or data_root == repo_root:
        raise ValueError("local-data deve restare una directory interna al repository")

    destinations = tuple(
        destination
        for person in people
        for destination in (
            person.source_documents,
            person.metadata_directory,
            person.local_build,
        )
    )
    protected_reference = data_root / "reference"
    for index, destination in enumerate(destinations):
        if destination == data_root or not destination.is_relative_to(data_root):
            raise ValueError("una destinazione configurata esce da local-data")
        if (
            destination == protected_reference
            or protected_reference in destination.parents
            or destination in protected_reference.parents
        ):
            raise ValueError(
                "local-data/reference è riservata al checkout di controllo"
            )
        for other in destinations[index + 1 :]:
            if (
                destination == other
                or destination in other.parents
                or other in destination.parents
            ):
                raise ValueError("le destinazioni configurate si sovrappongono")


def _sync_pairs(
    people: tuple[PersonPaths, ...], source_root: Path
) -> tuple[tuple[Path, Path], ...]:
    """Build Dropbox-to-local directory pairs for enabled people.

    Parameters
    ----------
    people : tuple[PersonPaths, ...]
        Configured people in account order.
    source_root : pathlib.Path
        Dropbox archive root containing title-cased document directories and
        the ``_Metadata`` directory.

    Returns
    -------
    tuple[tuple[pathlib.Path, pathlib.Path], ...]
        Document and metadata source-to-destination pairs.
    """

    return tuple(
        pair
        for person in people
        for pair in (
            (source_root / person.person_id.capitalize(), person.source_documents),
            (
                source_root / "_Metadata" / person.person_id,
                person.metadata_directory,
            ),
        )
    )


def _validate_sources(
    pairs: tuple[tuple[Path, Path], ...], local_data: Path
) -> None:
    """Validate archive sources and prevent overlap with local destinations.

    Parameters
    ----------
    pairs : tuple[tuple[pathlib.Path, pathlib.Path], ...]
        Source-to-destination pairs.
    local_data : pathlib.Path
        Private data root managed by this utility.

    Returns
    -------
    None

    Raises
    ------
    ValueError
        If a source is missing or overlaps local-data.
    """

    data_root = local_data.resolve(strict=False)
    missing = [source for source, _ in pairs if not source.is_dir()]
    if missing:
        rendered = ", ".join(str(path) for path in missing)
        raise ValueError(f"directory sorgente assenti: {rendered}")
    for source, destination in pairs:
        resolved_source = source.resolve()
        resolved_destination = destination.resolve(strict=False)
        if (
            resolved_source.is_relative_to(data_root)
            or resolved_destination.is_relative_to(resolved_source)
            or resolved_source.is_relative_to(resolved_destination)
        ):
            raise ValueError("sorgente e destinazione rsync non devono sovrapporsi")


def _require_rsync() -> None:
    """Ensure that rsync is available before synchronization or comparison.

    Parameters
    ----------
    None

    Returns
    -------
    None

    Raises
    ------
    ValueError
        If ``rsync`` is not available on ``PATH``.
    """

    if shutil.which("rsync") is None:
        raise ValueError("eseguibile rsync non trovato nel PATH")


def _synchronize_local_data(
    people: tuple[PersonPaths, ...], pairs: tuple[tuple[Path, Path], ...]
) -> None:
    """Synchronize configured documents and metadata, preserving other data.

    Parameters
    ----------
    people : tuple[PersonPaths, ...]
        Enabled people whose local build directories must exist.
    pairs : tuple[tuple[pathlib.Path, pathlib.Path], ...]
        Validated source-to-destination pairs.

    Returns
    -------
    None

    Raises
    ------
    OSError
        If creating a destination directory fails.
    subprocess.CalledProcessError
        If rsync fails for a source-to-destination pair.
    """

    for source, destination in pairs:
        destination.mkdir(parents=True, exist_ok=True)
        _rsync(source, destination)
    for person in people:
        person.local_build.mkdir(parents=True, exist_ok=True)


def _refresh_generated(people: tuple[PersonPaths, ...]) -> None:
    """Recreate configured local build directories only.

    Parameters
    ----------
    people : tuple[PersonPaths, ...]
        Enabled people whose generated builds should be refreshed.

    Returns
    -------
    None

    Raises
    ------
    OSError
        If a generated directory cannot be removed or recreated.
    """

    for person in people:
        if person.local_build.exists():
            shutil.rmtree(person.local_build)
        person.local_build.mkdir(parents=True, exist_ok=True)


def _compare(pairs: tuple[tuple[Path, Path], ...]) -> int:
    """Compare archive and local directories without modifying either side.

    Parameters
    ----------
    pairs : tuple[tuple[pathlib.Path, pathlib.Path], ...]
        Validated source-to-destination pairs.

    Returns
    -------
    int
        Zero when identical, otherwise one when differences exist.

    Raises
    ------
    subprocess.CalledProcessError
        If rsync returns a non-zero exit status.
    """

    differences = False
    for source, destination in pairs:
        differences = _rsync(source, destination, dry_run=True) or differences
    print(f"stato={'differenze' if differences else 'identico'}")
    return int(differences)


def _rsync(source: Path, destination: Path, *, dry_run: bool = False) -> bool:
    """Run rsync for one pair and report dry-run differences.

    Parameters
    ----------
    source : pathlib.Path
        Existing source directory.
    destination : pathlib.Path
        Configured local destination directory.
    dry_run : bool, optional
        Compare and list changes without modifying either directory.

    Returns
    -------
    bool
        Whether a dry run found differences. Always false after synchronization.

    Raises
    ------
    subprocess.CalledProcessError
        If rsync returns a non-zero exit status.
    """

    command = ["rsync", "--archive", "--delete"]
    if dry_run:
        command.extend(("--dry-run", "--itemize-changes"))
    command.extend((f"{source}/", f"{destination}/"))
    result = subprocess.run(command, check=True, capture_output=dry_run, text=True)
    if not dry_run:
        return False
    if result.stdout:
        print(f"sorgente={source}")
        print(f"destinazione={destination}")
        print(result.stdout, end="")
    return bool(result.stdout)


if __name__ == "__main__":
    raise SystemExit(main())

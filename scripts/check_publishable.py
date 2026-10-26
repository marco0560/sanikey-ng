"""Check migration documentation and files intended for publication.

Parameters
----------
None

Returns
-------
None
"""

from __future__ import annotations

import re
import subprocess
import sys
import tomllib
from pathlib import Path
from urllib.parse import unquote, urlsplit

REQUIRED_DOCUMENTS = (
    "CONTRIBUTING.md",
    "docs/architecture.md",
    "docs/migration/START-HERE.md",
    "docs/migration/ledger.md",
    "docs/migration/plan.md",
    "docs/migration/tasks/README.md",
    "docs/product-contract.md",
)
FORBIDDEN_PREFIXES = (
    ".artifacts/",
    ".codira/",
    "config/",
    "dist/",
    "exports/",
    "local-data/",
    "logs/",
    "target/",
)
FORBIDDEN_SUFFIXES = (
    ".db",
    ".key",
    ".log",
    ".p12",
    ".pem",
    ".pfx",
    ".sqlite",
    ".sqlite3",
)
MARKDOWN_LINK = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")


def repository_paths(root: Path) -> tuple[list[Path], list[str]]:
    """Return tracked and visible untracked files from Git.

    Parameters
    ----------
    root : Path
        Repository root directory.

    Returns
    -------
    tuple[list[Path], list[str]]
        Repository-relative paths and command errors.
    """
    try:
        result = subprocess.run(
            ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
            cwd=root,
            check=False,
            capture_output=True,
        )
    except OSError as error:
        return [], [f"impossibile eseguire git ls-files: {error}"]
    if result.returncode != 0:
        return [], ["git ls-files non è riuscito"]
    paths = [Path(item.decode()) for item in result.stdout.split(b"\0") if item]
    return paths, []


def local_link_errors(root: Path, paths: list[Path]) -> list[str]:
    """Find Markdown links whose local targets do not exist.

    Parameters
    ----------
    root : Path
        Repository root directory.
    paths : list[Path]
        Candidate repository-relative paths.

    Returns
    -------
    list[str]
        Human-readable errors for missing local targets.
    """
    errors: list[str] = []
    for relative_path in paths:
        if relative_path.suffix.lower() != ".md":
            continue
        source = root / relative_path
        if not source.is_file():
            continue
        try:
            content = source.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            errors.append(f"impossibile leggere {relative_path}: {error}")
            continue
        for match in MARKDOWN_LINK.finditer(content):
            targets = match.group(1).strip().split(maxsplit=1)
            if not targets:
                continue
            raw_target = targets[0].strip("<>")
            target = urlsplit(raw_target)
            if target.scheme or target.netloc or not target.path:
                continue
            local_target = (source.parent / unquote(target.path)).resolve()
            if (
                not local_target.is_relative_to(root.resolve())
                or not local_target.exists()
            ):
                errors.append(f"{relative_path}: collegamento locale mancante: {raw_target}")
    return errors


def publishability_errors(paths: list[Path]) -> list[str]:
    """Reject private runtime paths and generated data from publication.

    Parameters
    ----------
    paths : list[Path]
        Candidate repository-relative paths.

    Returns
    -------
    list[str]
        Paths that violate repository publication rules.
    """
    errors: list[str] = []
    for path in paths:
        name = path.as_posix()
        basename = path.name
        if (
            name.startswith(FORBIDDEN_PREFIXES)
            or (basename.startswith(".env") and basename != ".env.example")
            or path.suffix.lower() in FORBIDDEN_SUFFIXES
        ):
            errors.append(f"contenuto non pubblicabile: {name}")
    return errors


def documentation_errors(root: Path, paths: list[Path]) -> list[str]:
    """Validate required documents, their local links, and migration TOML.

    Parameters
    ----------
    root : Path
        Repository root directory.
    paths : list[Path]
        Candidate repository-relative paths.

    Returns
    -------
    list[str]
        Missing documents, broken links, or invalid TOML errors.
    """
    errors = [
        f"documento richiesto mancante: {name}"
        for name in REQUIRED_DOCUMENTS
        if not (root / name).is_file()
    ]
    errors.extend(local_link_errors(root, paths))
    for toml_path in sorted((root / "docs/migration").rglob("*.toml")):
        try:
            tomllib.loads(toml_path.read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError) as error:
            errors.append(f"TOML non valido {toml_path.relative_to(root)}: {error}")
    return errors


def main() -> int:
    """Run publication and documentation checks.

    Parameters
    ----------
    None

    Returns
    -------
    int
        Process exit code: zero when all checks pass.
    """
    root = Path(__file__).resolve().parents[1]
    paths, errors = repository_paths(root)
    errors.extend(documentation_errors(root, paths))
    errors.extend(publishability_errors(paths))
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"documenti e contenuti pubblicabili: OK ({len(paths)} file)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

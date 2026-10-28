//! Verifiche di privacy runtime indipendenti da Git.

use crate::config::AccountsConfig;
use crate::operation::{ErrorCode, OperationError};
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

const LOCAL_DATA_DIRS: &[&str] = &[
    "config",
    "patients",
    "generated",
    "exports",
    "logs",
    "local-data",
];

/// Verifica che i percorsi dei dati non puntino a contenuti versionati.
///
/// I percorsi esterni alla root sono ammessi. All'interno della root sono
/// consentite solo le directory locali previste dal contratto; il controllo
/// che tali directory siano ignorate da Git appartiene al gate di sviluppo.
///
/// # Errors
///
/// Restituisce un errore `privacy` se la root non è risolvibile o un percorso
/// di una persona entra in contenuto versionato del repository.
pub fn validate_runtime_privacy(
    config: &AccountsConfig,
    repo_root: impl AsRef<Path>,
) -> Result<(), OperationError> {
    let root = fs::canonicalize(repo_root.as_ref()).map_err(|cause| {
        privacy_error(format!(
            "root del repository non risolvibile {}: {cause}",
            repo_root.as_ref().display()
        ))
    })?;
    for person in &config.people {
        for (field, path) in [
            ("source_documents", person.source_documents.as_path()),
            ("metadata_directory", person.metadata_directory.as_path()),
            ("local_build", person.local_build.as_path()),
        ] {
            let resolved = resolve_non_strict(path).map_err(|cause| {
                privacy_error(format!(
                    "{}.{} non risolvibile {}: {cause}",
                    person.id,
                    field,
                    path.display()
                ))
            })?;
            let Ok(relative) = resolved.strip_prefix(&root) else {
                continue;
            };
            let allowed = relative
                .components()
                .next()
                .and_then(|component| component.as_os_str().to_str())
                .is_some_and(|name| LOCAL_DATA_DIRS.contains(&name));
            if !allowed {
                return Err(privacy_error(format!(
                    "{}.{} punta dentro contenuto versionato del repository: {}",
                    person.id,
                    field,
                    resolved.display()
                )));
            }
        }
    }
    Ok(())
}

fn resolve_non_strict(path: &Path) -> std::io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let normalized = normalize(&absolute);
    let mut ancestor = normalized.as_path();
    let mut missing = Vec::<OsString>::new();
    while !ancestor.exists() {
        let Some(name) = ancestor.file_name() else {
            break;
        };
        missing.push(name.to_os_string());
        let Some(parent) = ancestor.parent() else {
            break;
        };
        ancestor = parent;
    }
    let mut resolved = fs::canonicalize(ancestor)?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    Ok(normalize(&resolved))
}

fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn privacy_error(message: impl Into<String>) -> OperationError {
    OperationError::new(ErrorCode::Privacy, message)
}

#[cfg(test)]
mod tests;

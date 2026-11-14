//! Verifiche di privacy runtime indipendenti da Git.

use crate::config::AccountsConfig;
use crate::logging::{
    Component as LogComponent, TraceField, TraceValue, redact_secret_assignments, trace_current,
};
use crate::operation::{ErrorCode, OperationError};
use serde_json::json;
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
    let trace = trace_current(
        LogComponent::Core,
        "privacy::validate_runtime_privacy",
        || {
            let people = config
                .people
                .iter()
                .map(|person| {
                    json!({
                        "id": person.id,
                        "source_documents": TraceValue::path(&person.source_documents).to_json(),
                        "metadata_directory": TraceValue::path(&person.metadata_directory).to_json(),
                        "local_build": TraceValue::path(&person.local_build).to_json(),
                    })
                })
                .collect::<Vec<_>>();
            vec![
                TraceField::new("repo_root", TraceValue::path(repo_root.as_ref())),
                TraceField::new(
                    "config_projection",
                    TraceValue::Data(json!({
                        "config_version": config.config_version,
                        "people": people,
                    })),
                ),
                TraceField::new(
                    "config_representation",
                    TraceValue::Data(json!({
                        "format": "rust_debug",
                        "value": redact_secret_assignments(&format!("{config:#?}")),
                    })),
                ),
            ]
        },
    );

    let result: Result<(), OperationError> = (|| {
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
    })();

    if let Some(trace) = trace {
        let result = match &result {
            Ok(()) => json!({"outcome": "succeeded"}),
            Err(error) => json!({
                "outcome": "failed",
                "code": error.code().as_str(),
                "message": error.message(),
            }),
        };
        let _ = trace.finish(TraceValue::Data(result));
    }
    result
}

fn resolve_non_strict(path: &Path) -> std::io::Result<PathBuf> {
    let trace = trace_current(LogComponent::Core, "privacy::resolve_non_strict", || {
        vec![TraceField::new("path", TraceValue::path(path))]
    });
    let result: std::io::Result<PathBuf> = (|| {
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
    })();
    if let Some(trace) = trace {
        let result_value = match &result {
            Ok(path) => json!({"outcome": "succeeded", "path": TraceValue::path(path).to_json()}),
            Err(error) => json!({
                "outcome": "failed",
                "error_kind": error.kind().to_string(),
            }),
        };
        let _ = trace.finish(TraceValue::Data(result_value));
    }
    result
}

fn normalize(path: &Path) -> PathBuf {
    let trace = trace_current(LogComponent::Core, "privacy::normalize", || {
        vec![TraceField::new("path", TraceValue::path(path))]
    });
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
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!({
            "path": TraceValue::path(&normalized).to_json(),
        })));
    }
    normalized
}

fn privacy_error(message: impl Into<String>) -> OperationError {
    let message = message.into();
    let trace = trace_current(LogComponent::Core, "privacy::privacy_error", || {
        vec![TraceField::new("message", TraceValue::Data(json!(message)))]
    });
    let error = OperationError::new(ErrorCode::Privacy, message);
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!({
            "code": error.code().as_str(),
            "message": error.message(),
        })));
    }
    error
}

#[cfg(test)]
mod tests;

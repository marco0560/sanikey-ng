//! Scansione deterministica dei file sorgente e identificazione dei duplicati.

mod classify;
mod filters;

#[cfg(test)]
mod tests;

use crate::config::PersonConfig;
use crate::inventory::filters::IngestionMatcher;
use crate::metadata::DocumentRecord;
use crate::operation::{ErrorCode, OperationError};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Scansiona ogni file sorgente ammesso e conserva anche i duplicati.
///
/// I risultati sono ordinati per percorso relativo POSIX. Una directory
/// sorgente inesistente o non-directory produce un inventario vuoto; errori
/// durante l'attraversamento o la lettura di un file vengono restituiti.
/// Le regole include prevalgono sempre sulle esclusioni.
///
/// # Errors
///
/// Restituisce `ErrorCode::Configuration` per errori del filesystem durante
/// l'attraversamento o il calcolo dei digest.
pub fn scan_document_inventory(
    person: &PersonConfig,
) -> Result<Vec<DocumentRecord>, OperationError> {
    let root = &person.source_documents;
    match fs::metadata(root) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Ok(Vec::new()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(filesystem_error(
                "impossibile leggere la directory",
                root,
                error,
            ));
        }
    };
    let filters = IngestionMatcher::new(&person.ingestion);
    let mut files = Vec::new();
    collect_files(root, root, &filters, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));

    files
        .into_iter()
        .map(|(_, path)| document_record(path, root, &person.id))
        .collect()
}

/// Scansiona i documenti e mantiene la prima occorrenza di ciascun digest.
///
/// La prima occorrenza segue l'ordinamento deterministico di
/// [`scan_document_inventory`].
///
/// # Errors
///
/// Restituisce gli errori del filesystem prodotti dalla scansione.
pub fn scan_documents(person: &PersonConfig) -> Result<Vec<DocumentRecord>, OperationError> {
    let mut seen = HashSet::new();
    Ok(scan_document_inventory(person)?
        .into_iter()
        .filter(|document| seen.insert(document.sha256.clone()))
        .collect())
}

/// Raggruppa i documenti con contenuto identico in ordine di digest.
#[must_use]
pub fn find_duplicate_documents(
    documents: &[DocumentRecord],
) -> BTreeMap<String, Vec<DocumentRecord>> {
    let mut grouped: BTreeMap<String, Vec<DocumentRecord>> = BTreeMap::new();
    for document in documents {
        grouped
            .entry(document.sha256.clone())
            .or_default()
            .push(document.clone());
    }
    grouped.retain(|_, documents| documents.len() > 1);
    grouped
}

fn collect_files(
    root: &Path,
    directory: &Path,
    filters: &IngestionMatcher,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<(), OperationError> {
    let entries = fs::read_dir(directory)
        .map_err(|error| filesystem_error("impossibile scandire la directory", directory, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            filesystem_error(
                "impossibile leggere un elemento della directory",
                directory,
                error,
            )
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            filesystem_error("impossibile determinare il tipo del percorso", &path, error)
        })?;

        if file_type.is_dir() {
            collect_files(root, &path, filters, files)?;
        } else if file_type.is_file() || file_type.is_symlink() {
            let metadata = match fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(filesystem_error(
                        "impossibile leggere il percorso",
                        &path,
                        error,
                    ));
                }
            };
            if !metadata.is_file() {
                continue;
            }
            let relative = path.strip_prefix(root).map_err(|error| {
                OperationError::new(
                    ErrorCode::Configuration,
                    format!("percorso sorgente fuori dalla directory configurata: {error}"),
                )
            })?;
            let relative = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if !filters.is_excluded(&relative) {
                files.push((relative, path));
            }
        }
    }
    Ok(())
}

fn document_record(
    path: PathBuf,
    root: &Path,
    patient_id: &str,
) -> Result<DocumentRecord, OperationError> {
    let digest = sha256_file(&path)?;
    let relative = path.strip_prefix(root).map_err(|error| {
        OperationError::new(
            ErrorCode::Configuration,
            format!("percorso sorgente fuori dalla directory configurata: {error}"),
        )
    })?;
    let (date, title) = classify::title_from_path(&path);
    let category = classify::category(relative);
    let kind = classify::kind(&path);
    Ok(DocumentRecord {
        document_id: digest.clone(),
        patient_id: patient_id.to_owned(),
        path,
        title,
        category,
        kind,
        sha256: digest,
        date,
        series: None,
        tags: Vec::new(),
        origin: "source".to_owned(),
        container_id: None,
        internal_path: None,
    })
}

fn sha256_file(path: &Path) -> Result<String, OperationError> {
    let mut file = File::open(path)
        .map_err(|error| filesystem_error("impossibile aprire il documento", path, error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| filesystem_error("impossibile leggere il documento", path, error))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn filesystem_error(action: &str, path: &Path, cause: io::Error) -> OperationError {
    OperationError::new(
        ErrorCode::Configuration,
        format!("{action} {}: {cause}", path.display()),
    )
}

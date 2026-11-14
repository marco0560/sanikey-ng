//! Scansione deterministica dei file sorgente e identificazione dei duplicati.

mod classify;
mod filters;

#[cfg(test)]
mod tests;

use crate::config::PersonConfig;
use crate::inventory::filters::IngestionMatcher;
use crate::logging::{
    Component as LogComponent, TraceField, TraceOmission, TraceValue, redact_secret_assignments,
    trace_current,
};
use crate::metadata::DocumentRecord;
use crate::operation::{ErrorCode, OperationError};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

macro_rules! trace_inventory_call {
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], paths [$($path_name:literal => $path:expr),* $(,)?], fields [$($field_name:literal => $field_value:expr),* $(,)?], result_paths $result_paths:expr, $body:expr) => {{
        let trace = trace_current(LogComponent::Core, $function, || {
            Vec::from([
                $(TraceField::new($name, trace_debug($value)),)*
                $(TraceField::new($path_name, TraceValue::path($path)),)*
                $(TraceField::new($field_name, $field_value),)*
            ])
        });
        let result = run_inventory_body($body);
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!({
                "format":"rust_debug",
                "value":redact_secret_assignments(&format!("{result:#?}")),
                "native_paths":$result_paths(&result).to_json(),
            })));
        }
        result
    }};
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], paths [$($path_name:literal => $path:expr),* $(,)?], fields [$($field_name:literal => $field_value:expr),* $(,)?], $body:expr) => {
        trace_inventory_call!($function, [$($name => $value),*], paths [$($path_name => $path),*], fields [$($field_name => $field_value),*], result_paths trace_no_paths, $body)
    };
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], paths [$($path_name:literal => $path:expr),* $(,)?], result_paths $result_paths:expr, $body:expr) => {
        trace_inventory_call!($function, [$($name => $value),*], paths [$($path_name => $path),*], fields [], result_paths $result_paths, $body)
    };
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], paths [$($path_name:literal => $path:expr),* $(,)?], $body:expr) => {
        trace_inventory_call!($function, [$($name => $value),*], paths [$($path_name => $path),*], fields [], result_paths trace_no_paths, $body)
    };
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], result_paths $result_paths:expr, $body:expr) => {
        trace_inventory_call!($function, [$($name => $value),*], paths [], fields [], result_paths $result_paths, $body)
    };
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], $body:expr) => {
        trace_inventory_call!($function, [$($name => $value),*], paths [], fields [], result_paths trace_no_paths, $body)
    };
}

fn run_inventory_body<R>(body: impl FnOnce() -> R) -> R {
    body()
}

fn trace_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> TraceValue {
    TraceValue::Data(
        json!({"format":"rust_debug","value":redact_secret_assignments(&format!("{value:#?}"))}),
    )
}

fn trace_no_paths<T: ?Sized>(_: &T) -> TraceValue {
    TraceValue::Data(json!([]))
}

fn trace_person_paths(person: &PersonConfig) -> TraceValue {
    TraceValue::Data(json!([
        {"name":"source_documents","path":TraceValue::path(&person.source_documents).to_json()},
        {"name":"metadata_directory","path":TraceValue::path(&person.metadata_directory).to_json()},
        {"name":"local_build","path":TraceValue::path(&person.local_build).to_json()},
        {"name":"ui.background_image","path":person.ui.background_image.as_deref().map(TraceValue::path).map(|path|path.to_json())},
        {"name":"search.dictionary","path":person.search.dictionary.as_deref().map(TraceValue::path).map(|path|path.to_json())}
    ]))
}

fn trace_document_paths(documents: &[DocumentRecord]) -> TraceValue {
    TraceValue::Data(Value::Array(
        documents
            .iter()
            .enumerate()
            .map(|(index, document)| {
                json!({
                    "index":index,
                    "document_id":document.document_id,
                    "path":TraceValue::path(&document.path).to_json(),
                })
            })
            .collect(),
    ))
}

fn trace_document_result(result: &Result<Vec<DocumentRecord>, OperationError>) -> TraceValue {
    TraceValue::Data(
        result
            .as_ref()
            .map(|documents| trace_document_paths(documents).to_json())
            .unwrap_or_else(|_| json!({"state":"error","paths":[]})),
    )
}

fn trace_document(result: &Result<DocumentRecord, OperationError>) -> TraceValue {
    TraceValue::Data(
        result
            .as_ref()
            .map(|document| json!({"path":TraceValue::path(&document.path).to_json()}))
            .unwrap_or_else(|_| json!({"state":"error","paths":[]})),
    )
}

fn trace_duplicate_paths(duplicates: &BTreeMap<String, Vec<DocumentRecord>>) -> TraceValue {
    TraceValue::Data(Value::Array(duplicates.iter().map(|(digest, documents)| json!({"digest":digest,"documents":trace_document_paths(documents).to_json()})).collect()))
}

fn trace_collected_paths(files: &[(String, PathBuf)]) -> TraceValue {
    TraceValue::Data(Value::Array(
        files
            .iter()
            .enumerate()
            .map(|(index, (relative, path))| {
                json!({
                    "index":index,
                    "relative_path":relative,
                    "path":TraceValue::path(path).to_json(),
                })
            })
            .collect(),
    ))
}

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
    trace_inventory_call!(
        "inventory::scan_document_inventory",
        ["person" => person],
        paths [],
        fields ["person_native_paths" => trace_person_paths(person)],
        result_paths trace_document_result,
        || {
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
    collect_files(root, root, &filters, &mut files, 0)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));

    files
        .into_iter()
        .map(|(_, path)| document_record(path, root, &person.id))
        .collect()
        }
    )
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
    trace_inventory_call!(
        "inventory::scan_documents",
        ["person" => person],
        paths [],
        fields ["person_native_paths" => trace_person_paths(person)],
        result_paths trace_document_result,
        || {
    let mut seen = HashSet::new();
    Ok(scan_document_inventory(person)?
        .into_iter()
        .filter(|document| seen.insert(document.sha256.clone()))
        .collect())
        }
    )
}

/// Raggruppa i documenti con contenuto identico in ordine di digest.
#[must_use]
pub fn find_duplicate_documents(
    documents: &[DocumentRecord],
) -> BTreeMap<String, Vec<DocumentRecord>> {
    trace_inventory_call!(
        "inventory::find_duplicate_documents",
        ["documents" => documents],
        paths [],
        fields ["document_native_paths" => trace_document_paths(documents)],
        result_paths trace_duplicate_paths,
        || {
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
    )
}

fn collect_files(
    root: &Path,
    directory: &Path,
    filters: &IngestionMatcher,
    files: &mut Vec<(String, PathBuf)>,
    depth: usize,
) -> Result<(), OperationError> {
    let files_length_before = files.len();
    let trace = trace_current(LogComponent::Core, "inventory::collect_files", || {
        let mut fields = vec![
            TraceField::new("root", TraceValue::path(root)),
            TraceField::new("directory", TraceValue::path(directory)),
            TraceField::new("depth", trace_debug(&depth)),
            TraceField::new(
                "filters",
                TraceValue::Omitted(TraceOmission::UnsupportedType),
            ),
        ];
        if depth == 0 {
            fields.push(TraceField::new(
                "files_before",
                trace_collected_paths(files),
            ));
        } else {
            fields.push(TraceField::new(
                "files_state_reference",
                TraceValue::Data(json!({
                    "base":"root_call_files_before",
                    "length_at_entry":files_length_before,
                    "root_call_resolution":"nearest depth=0 inventory::collect_files ancestor via parent_call_id/call_id",
                    "reconstruction":"combine subtree paths_added by absolute index; sequence is event order, not append order",
                })),
            ));
        }
        fields
    });
    let trace_enabled = trace.is_some();
    let mut paths_added = Vec::new();
    let result = run_inventory_body(|| {
        let entries = fs::read_dir(directory).map_err(|error| {
            filesystem_error("impossibile scandire la directory", directory, error)
        })?;
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
                collect_files(root, &path, filters, files, depth + 1)?;
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
                    if trace_enabled {
                        let index = files.len() - 1;
                        paths_added.push(json!({
                            "index":index,
                            "relative_path":redact_secret_assignments(&files[index].0),
                            "path":TraceValue::path(&files[index].1).to_json(),
                        }));
                    }
                }
            }
        }
        Ok(())
    });
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!({
            "format":"rust_debug_with_reconstructible_path_delta",
            "value":{
                "return":redact_secret_assignments(&format!("{result:#?}")),
                "depth":depth,
                "files_length_after":files.len(),
                "paths_added":paths_added,
                "files_after":if depth==0 {Some(trace_collected_paths(files).to_json())} else {None},
            }
        })));
    }
    result
}

fn document_record(
    path: PathBuf,
    root: &Path,
    patient_id: &str,
) -> Result<DocumentRecord, OperationError> {
    trace_inventory_call!(
        "inventory::document_record",
        ["patient_id" => &patient_id],
        paths ["path" => &path, "root" => root],
        result_paths trace_document,
        || {
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
    )
}

pub(crate) fn staged_document_record(
    path: PathBuf,
    root: &Path,
    patient_id: &str,
    container_id: &str,
) -> Result<DocumentRecord, OperationError> {
    trace_inventory_call!(
        "inventory::staged_document_record",
        ["patient_id" => &patient_id, "container_id" => &container_id],
        paths ["path" => &path, "root" => root],
        result_paths trace_document,
        || {
    let mut record = document_record(path, root, patient_id)?;
    record.origin = "container".to_owned();
    record.container_id = Some(container_id.to_owned());
    record.internal_path = Some(
        record
            .path
            .strip_prefix(root)
            .map_err(|error| {
                OperationError::new(
                    ErrorCode::Configuration,
                    format!("percorso membro fuori dallo staging: {error}"),
                )
            })?
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    );
    Ok(record)
        }
    )
}

pub(crate) fn is_ingestion_excluded(config: &crate::config::IngestionConfig, path: &str) -> bool {
    trace_inventory_call!(
        "inventory::is_ingestion_excluded",
        ["config" => config, "path" => &path],
        paths [],
        fields ["path_native" => TraceValue::path(Path::new(path))],
        || IngestionMatcher::new(config).is_excluded(path)
    )
}

fn sha256_file(path: &Path) -> Result<String, OperationError> {
    trace_inventory_call!("inventory::sha256_file", [], paths ["path" => path], || {
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
    })
}

fn filesystem_error(action: &str, path: &Path, cause: io::Error) -> OperationError {
    trace_inventory_call!("inventory::filesystem_error", ["action" => &action, "cause" => &cause], paths ["path" => path], || {
        OperationError::new(
            ErrorCode::Configuration,
            format!("{action} {}: {cause}", path.display()),
        )
    })
}

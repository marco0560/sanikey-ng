//! Esportazione e validazione del campione TXT su directory simulata.

#[cfg(test)]
mod tests;

use crate::config::PersonConfig;
use crate::logging::{
    Component as LogComponent, TraceField, TraceValue, trace_current, trace_json_value,
    trace_sensitive_text,
};
use crate::operation::{ErrorCode, OperationError};
use crate::txt_pipeline::build_txt_pipeline;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_STAGE: AtomicUsize = AtomicUsize::new(0);

fn trace_export_call<T>(
    function: &'static str,
    arguments: impl FnOnce() -> Vec<TraceField>,
    action: impl FnOnce() -> T,
    result: impl FnOnce(&T) -> TraceValue,
) -> T {
    let trace = trace_current(LogComponent::Core, function, arguments);
    let value = action();
    if let Some(trace) = trace {
        let _ = trace.finish(result(&value));
    }
    value
}

fn trace_export_operation<T: std::fmt::Debug>(
    function: &'static str,
    arguments: impl FnOnce() -> Vec<TraceField>,
    action: impl FnOnce() -> Result<T, OperationError>,
) -> Result<T, OperationError> {
    trace_export_call(function, arguments, action, trace_result)
}

fn trace_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> TraceValue {
    trace_sensitive_text(&format!("{value:#?}"))
}

fn trace_result<T: std::fmt::Debug>(result: &Result<T, OperationError>) -> TraceValue {
    match result {
        Ok(value) => trace_debug(value),
        Err(error) => trace_error(error),
    }
}

fn trace_error(error: &OperationError) -> TraceValue {
    TraceValue::record(&[
        TraceField::new("code", TraceValue::data(error.code().as_str())),
        TraceField::new("message", trace_sensitive_text(error.message())),
    ])
}

fn trace_omission(reason: &'static str) -> TraceValue {
    TraceValue::record(&[
        TraceField::new("state", TraceValue::data("omitted")),
        TraceField::new("reason", TraceValue::data(reason)),
    ])
}

fn trace_json(value: &Value) -> TraceValue {
    trace_json_value(value)
}

fn trace_path(path: &Path) -> TraceValue {
    trace_json_value(&TraceValue::path(path).to_json())
}

/// Risultato dell'esportazione del sottoinsieme TXT.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimulatedExportResult {
    /// Radice della directory esportata.
    pub root: PathBuf,
    /// Manifest con gli SHA-256 degli artefatti.
    pub manifest: PathBuf,
    /// Numero di persone esportate.
    pub patients: usize,
    /// Numero totale di file, incluso il manifest.
    pub files: usize,
}

struct StageDirectory {
    path: PathBuf,
    committed: bool,
}

/// Rimuove lo staging se l'esportazione non è stata committata.
impl Drop for StageDirectory {
    fn drop(&mut self) {
        if !self.committed {
            let path = &self.path;
            let cleanup = trace_export_call(
                "simulated_export::StageDirectory::drop",
                || {
                    vec![
                        TraceField::new("stage_path", trace_path(path)),
                        TraceField::new("committed", TraceValue::data(false)),
                    ]
                },
                || fs::remove_dir_all(path).map_err(|cause| cause.to_string()),
                |result| match result {
                    Ok(()) => TraceValue::data("removed"),
                    Err(error) => trace_sensitive_text(error),
                },
            );
            let _ = cleanup;
        }
    }
}

/// Crea una directory autonoma con database, frontend, documenti TXT e manifest.
///
/// La destinazione deve essere nuova e simulata. Il generatore non riconosce né
/// scrive su volumi fisici. L'esportazione è costruita in una directory di
/// staging e rinominata alla destinazione solo dopo validazione completa.
///
/// # Errors
///
/// Restituisce `ErrorCode::Configuration` se la destinazione esiste, si
/// sovrappone agli input o se copia, checksum o validazione falliscono.
pub fn export_txt_simulation(
    person: &PersonConfig,
    target: impl AsRef<Path>,
) -> Result<SimulatedExportResult, OperationError> {
    trace_export_call(
        "simulated_export::export_txt_simulation",
        || {
            vec![
                TraceField::new("person", trace_debug(person)),
                TraceField::new("target", trace_path(target.as_ref())),
            ]
        },
        || export_txt_simulation_inner(person, target.as_ref()),
        trace_result,
    )
}

fn export_txt_simulation_inner(
    person: &PersonConfig,
    target: &Path,
) -> Result<SimulatedExportResult, OperationError> {
    trace_export_operation(
        "simulated_export::export_txt_simulation_inner",
        || {
            vec![
                TraceField::new("person", trace_debug(person)),
                TraceField::new("target", trace_path(target)),
            ]
        },
        || {
            let target = absolute_path(target)?;
            reject_input_overlap(person, &target)?;
            if target.exists() || fs::symlink_metadata(&target).is_ok() {
                return Err(configuration_error(format!(
                    "la destinazione simulata esiste già: {}",
                    target.display()
                )));
            }
            let pipeline = build_txt_pipeline(person)?;
            let parent = target.parent().ok_or_else(|| {
                configuration_error(format!(
                    "la destinazione non ha una directory padre: {}",
                    target.display()
                ))
            })?;
            fs::create_dir_all(parent).map_err(|cause| {
                filesystem_error("impossibile creare la directory padre", parent, cause)
            })?;
            let file_name = target
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    configuration_error(format!(
                        "nome destinazione non valido: {}",
                        target.display()
                    ))
                })?;
            let stage_path = parent.join(format!(
                ".{file_name}.staging-{}-{}",
                std::process::id(),
                NEXT_STAGE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&stage_path).map_err(|cause| {
                filesystem_error("impossibile creare lo staging", &stage_path, cause)
            })?;
            let mut stage = StageDirectory {
                path: stage_path,
                committed: false,
            };

            let patient_root = stage.path.join("patients").join(&person.id);
            fs::create_dir_all(&patient_root).map_err(|cause| {
                filesystem_error(
                    "impossibile creare l'archivio persona",
                    &patient_root,
                    cause,
                )
            })?;
            copy_file(&pipeline.database, &patient_root.join("medical_archive.db"))?;
            copy_tree(&person.local_build.join("web"), &patient_root.join("web"))?;
            copy_tree(
                &person.local_build.join("documents"),
                &patient_root.join("documents"),
            )?;
            write_root_index(&stage.path, person)?;
            let checksums = checksum_tree(&stage.path)?;
            write_manifest(&stage.path, person, &checksums)?;
            validate_txt_simulation(&stage.path)?;
            let files = count_files(&stage.path)?;

            fs::rename(&stage.path, &target).map_err(|cause| {
                filesystem_error("impossibile finalizzare l'esportazione", &target, cause)
            })?;
            stage.committed = true;
            let manifest = target.join("SANIKEY-MANIFEST.json");
            Ok(SimulatedExportResult {
                root: target,
                manifest,
                patients: 1,
                files,
            })
        },
    )
}

/// Convalida manifest, checksum e link locali del sottoinsieme TXT.
///
/// # Errors
///
/// Restituisce `ErrorCode::Configuration` se manca un artefatto richiesto,
/// il manifest non è valido, un checksum non coincide o un link esce
/// dall'archivio della persona.
pub fn validate_txt_simulation(target: impl AsRef<Path>) -> Result<(), OperationError> {
    trace_export_call(
        "simulated_export::validate_txt_simulation",
        || vec![TraceField::new("target", trace_path(target.as_ref()))],
        || validate_txt_simulation_inner(target.as_ref()),
        trace_result,
    )
}

fn validate_txt_simulation_inner(target: &Path) -> Result<(), OperationError> {
    trace_export_operation(
        "simulated_export::validate_txt_simulation_inner",
        || vec![TraceField::new("target", trace_path(target))],
        || {
            let manifest_path = target.join("SANIKEY-MANIFEST.json");
            let manifest_bytes = fs::read(&manifest_path).map_err(|cause| {
                filesystem_error("manifest non leggibile", &manifest_path, cause)
            })?;
            let manifest: Value = trace_export_call(
                "simulated_export::parse_manifest_json",
                || {
                    vec![
                        TraceField::new("path", trace_path(&manifest_path)),
                        TraceField::new(
                            "byte_length",
                            TraceValue::data(manifest_bytes.len() as u64),
                        ),
                        TraceField::new(
                            "raw_bytes",
                            trace_omission("json_is_recorded_after_parsing"),
                        ),
                    ]
                },
                || {
                    serde_json::from_slice::<Value>(&manifest_bytes).map_err(|cause| {
                        configuration_error(format!(
                            "manifest JSON non valido {}: {cause}",
                            manifest_path.display()
                        ))
                    })
                },
                |result| match result {
                    Ok(value) => trace_json(value),
                    Err(error) => trace_error(error),
                },
            )?;
            if manifest["schema_version"].as_i64() != Some(1) {
                return Err(configuration_error("versione manifest non supportata"));
            }
            if !target.join("index.html").is_file() {
                return Err(configuration_error("indice principale mancante"));
            }
            let patients = manifest["patients"]
                .as_array()
                .ok_or_else(|| configuration_error("elenco persone non valido nel manifest"))?;
            if patients.len() != 1 {
                return Err(configuration_error(
                    "il campione TXT richiede una sola persona",
                ));
            }
            let patient_id = patients[0]["id"]
                .as_str()
                .ok_or_else(|| configuration_error("id persona mancante nel manifest"))?;
            let index_path = target.join("index.html");
            let index = trace_export_call(
                "simulated_export::read_root_index",
                || vec![TraceField::new("path", trace_path(&index_path))],
                || {
                    fs::read_to_string(&index_path).map_err(|cause| {
                        filesystem_error("indice principale non leggibile", target, cause)
                    })
                },
                |result| match result {
                    Ok(value) => trace_sensitive_text(value),
                    Err(error) => trace_error(error),
                },
            )?;
            let expected_href = format!("href=\"patients/{patient_id}/web/index.html\"");
            if !index.contains(&expected_href) {
                return Err(configuration_error(
                    "link alla pagina persona mancante dall'indice",
                ));
            }
            validate_patient(target, patient_id)?;

            let checksums = manifest["checksums"]
                .as_object()
                .ok_or_else(|| configuration_error("tabella checksum non valida nel manifest"))?;
            validate_checksums(target, checksums)
        },
    )
}

fn validate_patient(target: &Path, patient_id: &str) -> Result<(), OperationError> {
    trace_export_call(
        "simulated_export::validate_patient",
        || {
            vec![
                TraceField::new("target", trace_path(target)),
                TraceField::new("patient_id", trace_sensitive_text(patient_id)),
            ]
        },
        || validate_patient_inner(target, patient_id),
        trace_result,
    )
}

fn validate_patient_inner(target: &Path, patient_id: &str) -> Result<(), OperationError> {
    trace_export_operation(
        "simulated_export::validate_patient_inner",
        || {
            vec![
                TraceField::new("target", trace_path(target)),
                TraceField::new("patient_id", trace_sensitive_text(patient_id)),
            ]
        },
        || {
            let patient_component = safe_relative_path(patient_id)?;
            if patient_component.components().count() != 1 {
                return Err(configuration_error("id persona non valido nel manifest"));
            }
            let patient_root = target.join("patients").join(patient_component);
            for required in [
                patient_root.join("medical_archive.db"),
                patient_root.join("web/index.html"),
                patient_root.join("web/data/documents.json"),
            ] {
                if !required.is_file() {
                    return Err(configuration_error(format!(
                        "artefatto richiesto mancante: {}",
                        required.display()
                    )));
                }
            }
            validate_document_links(&patient_root)
        },
    )
}

fn validate_document_links(patient_root: &Path) -> Result<(), OperationError> {
    trace_export_call(
        "simulated_export::validate_document_links",
        || vec![TraceField::new("patient_root", trace_path(patient_root))],
        || validate_document_links_inner(patient_root),
        trace_result,
    )
}

fn validate_document_links_inner(patient_root: &Path) -> Result<(), OperationError> {
    trace_export_operation(
        "simulated_export::validate_document_links_inner",
        || vec![TraceField::new("patient_root", trace_path(patient_root))],
        || {
            let data_path = patient_root.join("web/data/documents.json");
            let bytes = fs::read(&data_path).map_err(|cause| {
                filesystem_error("dati documenti non leggibili", &data_path, cause)
            })?;
            let data: Value = trace_export_call(
                "simulated_export::parse_document_export_json",
                || {
                    vec![
                        TraceField::new("path", trace_path(&data_path)),
                        TraceField::new("byte_length", TraceValue::data(bytes.len() as u64)),
                        TraceField::new(
                            "raw_bytes",
                            trace_omission("json_is_recorded_after_parsing"),
                        ),
                    ]
                },
                || {
                    serde_json::from_slice::<Value>(&bytes).map_err(|cause| {
                        configuration_error(format!("JSON documenti non valido: {cause}"))
                    })
                },
                |result| match result {
                    Ok(value) => trace_json(value),
                    Err(error) => trace_error(error),
                },
            )?;
            let documents = data["documents"]
                .as_array()
                .ok_or_else(|| configuration_error("elenco documenti non valido"))?;
            let page_path = patient_root.join("web/index.html");
            let page = trace_export_call(
                "simulated_export::read_patient_index",
                || vec![TraceField::new("path", trace_path(&page_path))],
                || {
                    fs::read_to_string(&page_path).map_err(|cause| {
                        filesystem_error("pagina non leggibile", &page_path, cause)
                    })
                },
                |result| match result {
                    Ok(value) => trace_sensitive_text(value),
                    Err(error) => trace_error(error),
                },
            )?;
            for document in documents {
                let href = document["href"]
                    .as_str()
                    .ok_or_else(|| configuration_error("href documento mancante"))?;
                if !href.starts_with("../documents/")
                    || href.contains('\\')
                    || href.contains(':')
                    || href.contains('?')
                    || href.contains('#')
                {
                    return Err(configuration_error(format!(
                        "href non relativo non ammesso: {href}"
                    )));
                }
                let relative = percent_decode(&href["../documents/".len()..])?;
                let relative = safe_relative_path(&relative)?;
                let linked = patient_root.join("documents").join(relative);
                if !linked.is_file() {
                    return Err(configuration_error(format!(
                        "documento linkato mancante: {}",
                        linked.display()
                    )));
                }
                let escaped_href = href.replace('&', "&amp;").replace('"', "&quot;");
                if !page.contains(&format!("href=\"{escaped_href}\"")) {
                    return Err(configuration_error(format!(
                        "link assente dalla pagina: {href}"
                    )));
                }
            }
            Ok(())
        },
    )
}

fn validate_checksums(target: &Path, checksums: &Map<String, Value>) -> Result<(), OperationError> {
    trace_export_call(
        "simulated_export::validate_checksums",
        || {
            vec![
                TraceField::new("target", trace_path(target)),
                TraceField::new("checksums", trace_debug(checksums)),
            ]
        },
        || validate_checksums_inner(target, checksums),
        trace_result,
    )
}

fn validate_checksums_inner(
    target: &Path,
    checksums: &Map<String, Value>,
) -> Result<(), OperationError> {
    trace_export_operation(
        "simulated_export::validate_checksums_inner",
        || {
            vec![
                TraceField::new("target", trace_path(target)),
                TraceField::new("checksums", trace_debug(checksums)),
            ]
        },
        || {
            let mut declared = BTreeSet::new();
            for (relative, expected) in checksums {
                let path = safe_relative_path(relative)?;
                let expected_hash = expected
                    .as_str()
                    .ok_or_else(|| configuration_error("checksum non testuale nel manifest"))?;
                let file = target.join(&path);
                if !file.is_file() || sha256_file(&file)? != expected_hash {
                    return Err(configuration_error(format!(
                        "checksum mancante o non coincidente: {relative}"
                    )));
                }
                declared.insert(relative.replace('\\', "/"));
            }
            let mut actual = BTreeSet::new();
            collect_relative_files(target, target, &mut actual)?;
            actual.remove("SANIKEY-MANIFEST.json");
            if declared != actual {
                return Err(configuration_error("insieme file diverso dal manifest"));
            }
            Ok(())
        },
    )
}

fn write_manifest(
    root: &Path,
    person: &PersonConfig,
    checksums: &BTreeMap<String, String>,
) -> Result<(), OperationError> {
    trace_export_call(
        "simulated_export::write_manifest",
        || {
            vec![
                TraceField::new("root", trace_path(root)),
                TraceField::new("person", trace_debug(person)),
                TraceField::new("checksums", trace_debug(&checksums)),
            ]
        },
        || write_manifest_inner(root, person, checksums),
        trace_result,
    )
}

fn write_manifest_inner(
    root: &Path,
    person: &PersonConfig,
    checksums: &BTreeMap<String, String>,
) -> Result<(), OperationError> {
    trace_export_operation(
        "simulated_export::write_manifest_inner",
        || {
            vec![
                TraceField::new("root", trace_path(root)),
                TraceField::new("person", trace_debug(person)),
                TraceField::new("checksums", trace_debug(checksums)),
            ]
        },
        || {
            let path = root.join("SANIKEY-MANIFEST.json");
            let manifest = json!({
                "schema_version": 1,
                "exported_at": null,
                "sanikey_version": crate::version(),
                "copy_strategy": "simulated",
                "patients": [{
                    "id": person.id,
                    "display_name": person.display_name,
                    "usb_uuid": person.usb_uuid,
                }],
                "checksums": checksums,
            });
            let contents = serde_json::to_string_pretty(&manifest).map_err(|cause| {
                configuration_error(format!("impossibile serializzare il manifest: {cause}"))
            })?;
            let contents = format!("{contents}\n");
            trace_export_call(
                "simulated_export::write_manifest_file",
                || {
                    vec![
                        TraceField::new("path", trace_path(&path)),
                        TraceField::new("content", trace_sensitive_text(&contents)),
                    ]
                },
                || {
                    fs::write(&path, &contents).map_err(|cause| {
                        filesystem_error("impossibile scrivere il manifest", &path, cause)
                    })
                },
                trace_result,
            )
        },
    )
}

fn write_root_index(root: &Path, person: &PersonConfig) -> Result<(), OperationError> {
    trace_export_call(
        "simulated_export::write_root_index",
        || {
            vec![
                TraceField::new("root", trace_path(root)),
                TraceField::new("person", trace_debug(person)),
            ]
        },
        || write_root_index_inner(root, person),
        trace_result,
    )
}

fn write_root_index_inner(root: &Path, person: &PersonConfig) -> Result<(), OperationError> {
    trace_export_operation(
        "simulated_export::write_root_index_inner",
        || {
            vec![
                TraceField::new("root", trace_path(root)),
                TraceField::new("person", trace_debug(person)),
            ]
        },
        || {
            let href = format!("patients/{}/web/index.html", person.id);
            let html = format!(
                "<!doctype html>\n<html lang=\"it\"><head><meta charset=\"utf-8\"><title>SaniKey</title></head><body><main><h1>Archivio SaniKey</h1><ul><li><a href=\"{}\">{}</a></li></ul></main></body></html>\n",
                escape_html(&href),
                escape_html(&person.display_name)
            );
            let path = root.join("index.html");
            trace_export_call(
                "simulated_export::write_root_index_file",
                || {
                    vec![
                        TraceField::new("path", trace_path(&path)),
                        TraceField::new("content", trace_sensitive_text(&html)),
                    ]
                },
                || {
                    fs::write(&path, &html).map_err(|cause| {
                        filesystem_error("impossibile scrivere l'indice USB", &path, cause)
                    })
                },
                trace_result,
            )
        },
    )
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), OperationError> {
    trace_export_call(
        "simulated_export::copy_tree",
        || {
            vec![
                TraceField::new("source", trace_path(source)),
                TraceField::new("target", trace_path(target)),
            ]
        },
        || copy_tree_inner(source, target),
        trace_result,
    )
}

fn copy_tree_inner(source: &Path, target: &Path) -> Result<(), OperationError> {
    trace_export_operation(
        "simulated_export::copy_tree_inner",
        || {
            vec![
                TraceField::new("source", trace_path(source)),
                TraceField::new("target", trace_path(target)),
            ]
        },
        || {
            if !source.is_dir() {
                return Err(configuration_error(format!(
                    "directory di build mancante: {}",
                    source.display()
                )));
            }
            fs::create_dir_all(target).map_err(|cause| {
                filesystem_error("impossibile creare la directory export", target, cause)
            })?;
            let mut entries = fs::read_dir(source)
                .map_err(|cause| {
                    filesystem_error("directory di build non leggibile", source, cause)
                })?
                .map(|entry| {
                    entry.map_err(|cause| {
                        filesystem_error("elemento della build non leggibile", source, cause)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for entry in entries {
                let source_path = entry.path();
                let target_path = target.join(entry.file_name());
                let kind = entry.file_type().map_err(|cause| {
                    filesystem_error("tipo elemento build non leggibile", &source_path, cause)
                })?;
                if kind.is_symlink() {
                    return Err(configuration_error(format!(
                        "link simbolico non ammesso nell'export: {}",
                        source_path.display()
                    )));
                }
                if kind.is_dir() {
                    copy_tree(&source_path, &target_path)?;
                } else if kind.is_file() {
                    copy_file(&source_path, &target_path)?;
                }
            }
            Ok(())
        },
    )
}

fn copy_file(source: &Path, target: &Path) -> Result<(), OperationError> {
    trace_export_call(
        "simulated_export::copy_file",
        || {
            let byte_length = fs::metadata(source).ok().map(|metadata| metadata.len());
            vec![
                TraceField::new("source", trace_path(source)),
                TraceField::new("target", trace_path(target)),
                TraceField::new("byte_length", TraceValue::data(byte_length)),
                TraceField::new(
                    "content",
                    trace_omission("filesystem_copy_bytes_not_observed"),
                ),
            ]
        },
        || copy_file_inner(source, target),
        trace_result,
    )
}

fn copy_file_inner(source: &Path, target: &Path) -> Result<(), OperationError> {
    trace_export_operation(
        "simulated_export::copy_file_inner",
        || {
            vec![
                TraceField::new("source", trace_path(source)),
                TraceField::new("target", trace_path(target)),
            ]
        },
        || {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|cause| {
                    filesystem_error("impossibile creare la directory export", parent, cause)
                })?;
            }
            fs::copy(source, target).map_err(|cause| {
                filesystem_error("impossibile copiare l'artefatto", target, cause)
            })?;
            Ok(())
        },
    )
}

fn checksum_tree(root: &Path) -> Result<BTreeMap<String, String>, OperationError> {
    trace_export_call(
        "simulated_export::checksum_tree",
        || vec![TraceField::new("root", trace_path(root))],
        || checksum_tree_inner(root),
        trace_result,
    )
}

fn checksum_tree_inner(root: &Path) -> Result<BTreeMap<String, String>, OperationError> {
    trace_export_operation(
        "simulated_export::checksum_tree_inner",
        || vec![TraceField::new("root", trace_path(root))],
        || {
            let mut files = BTreeSet::new();
            collect_relative_files(root, root, &mut files)?;
            files
                .into_iter()
                .map(|relative| {
                    let path = root.join(&relative);
                    Ok((relative, sha256_file(&path)?))
                })
                .collect()
        },
    )
}

fn collect_relative_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<String>,
) -> Result<(), OperationError> {
    trace_export_call(
        "simulated_export::collect_relative_files",
        || {
            vec![
                TraceField::new("root", trace_path(root)),
                TraceField::new("directory", trace_path(directory)),
            ]
        },
        || collect_relative_files_inner(root, directory, files),
        trace_result,
    )
}

fn collect_relative_files_inner(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<String>,
) -> Result<(), OperationError> {
    let initial_file_count = files.len();
    trace_export_operation(
        "simulated_export::collect_relative_files_inner",
        || {
            vec![
                TraceField::new("root", trace_path(root)),
                TraceField::new("directory", trace_path(directory)),
                TraceField::new("initial_file_count", TraceValue::data(initial_file_count)),
            ]
        },
        || {
            let mut entries = fs::read_dir(directory)
                .map_err(|cause| {
                    filesystem_error("directory export non leggibile", directory, cause)
                })?
                .map(|entry| {
                    entry.map_err(|cause| {
                        filesystem_error("elemento export non leggibile", directory, cause)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for entry in entries {
                let path = entry.path();
                let kind = entry.file_type().map_err(|cause| {
                    filesystem_error("tipo elemento export non leggibile", &path, cause)
                })?;
                if kind.is_symlink() {
                    return Err(configuration_error(format!(
                        "link simbolico non ammesso nell'export: {}",
                        path.display()
                    )));
                }
                if kind.is_dir() {
                    collect_relative_files(root, &path, files)?;
                } else if kind.is_file() {
                    let relative = path.strip_prefix(root).map_err(|cause| {
                        configuration_error(format!("percorso export fuori dalla radice: {cause}"))
                    })?;
                    let normalized = relative
                        .components()
                        .map(|component| component.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/");
                    files.insert(normalized);
                }
            }
            Ok(())
        },
    )
}

fn safe_relative_path(value: &str) -> Result<PathBuf, OperationError> {
    trace_export_call(
        "simulated_export::safe_relative_path",
        || vec![TraceField::new("value", trace_sensitive_text(value))],
        || safe_relative_path_inner(value),
        trace_result,
    )
}

fn safe_relative_path_inner(value: &str) -> Result<PathBuf, OperationError> {
    trace_export_operation(
        "simulated_export::safe_relative_path_inner",
        || vec![TraceField::new("value", trace_sensitive_text(value))],
        || {
            if value.is_empty() || value.contains('\\') || value.starts_with('/') {
                return Err(configuration_error(format!(
                    "percorso export non relativo: {value}"
                )));
            }
            let path = Path::new(value);
            if path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            {
                return Err(configuration_error(format!(
                    "percorso export non sicuro: {value}"
                )));
            }
            Ok(path.to_path_buf())
        },
    )
}

fn percent_decode(value: &str) -> Result<String, OperationError> {
    trace_export_call(
        "simulated_export::percent_decode",
        || vec![TraceField::new("value", trace_sensitive_text(value))],
        || percent_decode_inner(value),
        trace_result,
    )
}

fn percent_decode_inner(value: &str) -> Result<String, OperationError> {
    trace_export_operation(
        "simulated_export::percent_decode_inner",
        || vec![TraceField::new("value", trace_sensitive_text(value))],
        || {
            let bytes = value.as_bytes();
            let mut decoded = Vec::with_capacity(bytes.len());
            let mut index = 0;
            while index < bytes.len() {
                if bytes[index] == b'%' {
                    let end = index + 3;
                    if end > bytes.len() {
                        return Err(configuration_error("encoding percent incompleto nel link"));
                    }
                    let digits = std::str::from_utf8(&bytes[index + 1..end])
                        .map_err(|_| configuration_error("encoding percent non valido nel link"))?;
                    let byte = u8::from_str_radix(digits, 16)
                        .map_err(|_| configuration_error("encoding percent non valido nel link"))?;
                    decoded.push(byte);
                    index = end;
                } else {
                    decoded.push(bytes[index]);
                    index += 1;
                }
            }
            String::from_utf8(decoded)
                .map_err(|_| configuration_error("percorso UTF-8 non valido nel link"))
        },
    )
}

fn sha256_file(path: &Path) -> Result<String, OperationError> {
    trace_export_call(
        "simulated_export::sha256_file",
        || {
            let byte_length = fs::metadata(path).ok().map(|metadata| metadata.len());
            vec![
                TraceField::new("path", trace_path(path)),
                TraceField::new("byte_length", TraceValue::data(byte_length)),
                TraceField::new("raw_bytes", trace_omission("raw_file_bytes_not_recorded")),
            ]
        },
        || sha256_file_inner(path),
        trace_result,
    )
}

fn sha256_file_inner(path: &Path) -> Result<String, OperationError> {
    trace_export_operation(
        "simulated_export::sha256_file_inner",
        || vec![TraceField::new("path", trace_path(path))],
        || {
            let mut file = fs::File::open(path)
                .map_err(|cause| filesystem_error("file export non leggibile", path, cause))?;
            let mut digest = Sha256::new();
            let mut buffer = [0_u8; 1024 * 1024];
            loop {
                let length = file.read(&mut buffer).map_err(|cause| {
                    filesystem_error("impossibile calcolare SHA-256", path, cause)
                })?;
                if length == 0 {
                    break;
                }
                digest.update(&buffer[..length]);
            }
            Ok(format!("{:x}", digest.finalize()))
        },
    )
}

fn count_files(root: &Path) -> Result<usize, OperationError> {
    trace_export_call(
        "simulated_export::count_files",
        || vec![TraceField::new("root", trace_path(root))],
        || count_files_inner(root),
        trace_result,
    )
}

fn count_files_inner(root: &Path) -> Result<usize, OperationError> {
    trace_export_operation(
        "simulated_export::count_files_inner",
        || vec![TraceField::new("root", trace_path(root))],
        || {
            let mut files = BTreeSet::new();
            collect_relative_files(root, root, &mut files)?;
            Ok(files.len())
        },
    )
}

fn absolute_path(path: &Path) -> Result<PathBuf, OperationError> {
    trace_export_call(
        "simulated_export::absolute_path",
        || vec![TraceField::new("path", trace_path(path))],
        || absolute_path_inner(path),
        trace_result,
    )
}

fn absolute_path_inner(path: &Path) -> Result<PathBuf, OperationError> {
    trace_export_operation(
        "simulated_export::absolute_path_inner",
        || vec![TraceField::new("path", trace_path(path))],
        || {
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .map_err(|cause| {
                        configuration_error(format!("directory corrente non disponibile: {cause}"))
                    })?
                    .join(path)
            };
            let mut normalized = PathBuf::new();
            for component in absolute.components() {
                match component {
                    Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
                    Component::RootDir => normalized.push(component.as_os_str()),
                    Component::CurDir => {}
                    Component::ParentDir => {
                        normalized.pop();
                    }
                    Component::Normal(part) => normalized.push(part),
                }
            }
            Ok(normalized)
        },
    )
}

fn reject_input_overlap(person: &PersonConfig, target: &Path) -> Result<(), OperationError> {
    trace_export_call(
        "simulated_export::reject_input_overlap",
        || {
            vec![
                TraceField::new("person", trace_debug(person)),
                TraceField::new("target", trace_path(target)),
            ]
        },
        || reject_input_overlap_inner(person, target),
        trace_result,
    )
}

fn reject_input_overlap_inner(person: &PersonConfig, target: &Path) -> Result<(), OperationError> {
    trace_export_operation(
        "simulated_export::reject_input_overlap_inner",
        || {
            vec![
                TraceField::new("person", trace_debug(person)),
                TraceField::new("target", trace_path(target)),
            ]
        },
        || {
            for input in [&person.source_documents, &person.local_build] {
                let input = absolute_path(input)?;
                if target.starts_with(&input) || input.starts_with(target) {
                    return Err(configuration_error(format!(
                        "destinazione simulata sovrapposta agli input: {}",
                        target.display()
                    )));
                }
            }
            Ok(())
        },
    )
}

fn escape_html(value: &str) -> String {
    trace_export_call(
        "simulated_export::escape_html",
        || vec![TraceField::new("value", trace_sensitive_text(value))],
        || {
            value
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&#39;")
        },
        |escaped| trace_sensitive_text(escaped),
    )
}

fn filesystem_error(context: &str, path: &Path, cause: std::io::Error) -> OperationError {
    trace_export_call(
        "simulated_export::filesystem_error",
        || {
            vec![
                TraceField::new("context", trace_sensitive_text(context)),
                TraceField::new("path", trace_path(path)),
                TraceField::new("cause", trace_sensitive_text(&cause.to_string())),
            ]
        },
        || configuration_error(format!("{context} {}: {cause}", path.display())),
        trace_error,
    )
}

fn configuration_error(message: impl Into<String>) -> OperationError {
    let message = message.into();
    trace_export_call(
        "simulated_export::configuration_error",
        || vec![TraceField::new("message", trace_sensitive_text(&message))],
        || OperationError::new(ErrorCode::Configuration, message.clone()),
        trace_error,
    )
}

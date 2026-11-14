//! Build locali per una persona o per tutti i profili abilitati.

#[cfg(test)]
mod tests;

use crate::aifa::{LeafletDownloadResult, download_confirmed_leaflets};
use crate::config::{AccountsConfig, PersonConfig};
use crate::containers::{ContainerStagingResult, stage_container_documents};
use crate::database::build_database;
use crate::dicom::{DicomStudy, generate_dicom_previews, prepare_dicom_media};
use crate::exports::generate_exports;
use crate::frontend::build_frontend;
use crate::inventory::{find_duplicate_documents, scan_document_inventory};
use crate::logging::{Component, TraceField, TraceValue, redact_secret_assignments, trace_current};
use crate::metadata::{
    CuratedMetadata, DocumentRecord, load_curated_metadata, prepare_people_metadata,
};
use crate::observation_imports::ensure_observation_imports_current;
use crate::office::{OfficeRenderOutcome, render_office_document, supports_conversion};
use crate::operation::{ErrorCode, OperationError};
use crate::parameter_reports::{write_parameter_extraction_reports, write_parameter_rejections};
use crate::parameter_rules::{
    ParameterBuildResult, ParameterRules, build_parameter_slices, load_patient_parameter_rules,
};
use crate::parameter_slices::{discover_configured_candidates, normalize_label};
use crate::pdf::{
    ExtractedText, PdfExtractionOptions, TextPageSpan, extract_pdf_text_with_options,
};
use crate::vital_signs::{extract_vital_signs, merge_vital_signs};
use chrono::{SecondsFormat, Utc};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const CACHE_SCHEMA_VERSION: u64 = 1;

macro_rules! trace_build_call {
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], paths [$($path_name:literal => $path:expr),* $(,)?], fields [$($field_name:literal => $field_value:expr),* $(,)?], result_paths [$($result_path_name:literal => $result_path:expr),* $(,)?], $body:expr) => {{
        let trace = trace_current(Component::Core, $function, || {
            Vec::from([
                $(TraceField::new($name, trace_debug($value)),)*
                $(TraceField::new($path_name, TraceValue::path($path)),)*
                $(TraceField::new($field_name, $field_value),)*
            ])
        });
        let result = ($body)();
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!({
                "format": "rust_debug",
                "value": redact_secret_assignments(&format!("{result:#?}")),
                "native_paths": Vec::<Value>::from([
                    $(json!({"name": $result_path_name, "value": $result_path(&result).to_json()}),)*
                ]),
            })));
        }
        result
    }};
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], paths [$($path_name:literal => $path:expr),* $(,)?], result_paths [$($result_path_name:literal => $result_path:expr),* $(,)?], $body:expr) => {{
        trace_build_call!($function, [$($name => $value),*], paths [$($path_name => $path),*], fields [], result_paths [$($result_path_name => $result_path),*], $body)
    }};
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], paths [$($path_name:literal => $path:expr),* $(,)?], fields [$($field_name:literal => $field_value:expr),* $(,)?], $body:expr) => {{
        trace_build_call!($function, [$($name => $value),*], paths [$($path_name => $path),*], fields [$($field_name => $field_value),*], result_paths [], $body)
    }};
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], paths [$($path_name:literal => $path:expr),* $(,)?], $body:expr) => {{
        trace_build_call!($function, [$($name => $value),*], paths [$($path_name => $path),*], fields [], result_paths [], $body)
    }};
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], $body:expr) => {
        trace_build_call!($function, [$($name => $value),*], paths [], fields [], result_paths [], $body)
    };
}

fn trace_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> TraceValue {
    TraceValue::Data(json!({
        "format": "rust_debug",
        "value": redact_secret_assignments(&format!("{value:#?}")),
    }))
}

fn trace_person_paths(person: &PersonConfig) -> TraceValue {
    TraceValue::Data(json!([
        {"name":"source_documents","path":TraceValue::path(&person.source_documents).to_json()},
        {"name":"metadata_directory","path":TraceValue::path(&person.metadata_directory).to_json()},
        {"name":"local_build","path":TraceValue::path(&person.local_build).to_json()},
        {"name":"ui.background_image","path":person.ui.background_image.as_deref().map(TraceValue::path).map(|path| path.to_json())},
        {"name":"search.dictionary","path":person.search.dictionary.as_deref().map(TraceValue::path).map(|path| path.to_json())}
    ]))
}

fn trace_accounts_paths(config: &AccountsConfig) -> TraceValue {
    TraceValue::Data(json!({
        "config_path": TraceValue::path(&config.path).to_json(),
        "people": config.people.iter().enumerate().map(|(index, person)| json!({
            "index": index,
            "id": person.id,
            "paths": trace_person_paths(person).to_json(),
        })).collect::<Vec<_>>(),
        "ui.background_image": config.ui.background_image.as_deref().map(TraceValue::path).map(|path| path.to_json()),
        "search.dictionary": config.search.dictionary.as_deref().map(TraceValue::path).map(|path| path.to_json())
    }))
}

fn trace_document_paths(documents: &[DocumentRecord]) -> TraceValue {
    TraceValue::Data(Value::Array(
        documents
            .iter()
            .enumerate()
            .map(|(index, document)| {
                json!({
                    "index": index,
                    "document_id": document.document_id,
                    "path": TraceValue::path(&document.path).to_json(),
                })
            })
            .collect(),
    ))
}

fn trace_path_vector(paths: &[PathBuf]) -> TraceValue {
    TraceValue::Data(json!({
        "format": "rust_debug_with_native_paths",
        "rust_debug": redact_secret_assignments(&format!("{paths:#?}")),
        "paths": paths.iter().enumerate().map(|(index, path)| json!({
            "index": index,
            "path": TraceValue::path(path).to_json(),
        })).collect::<Vec<_>>(),
    }))
}

fn trace_path_result(result: &Result<PathBuf, OperationError>) -> TraceValue {
    TraceValue::Data(json!({
        "state": if result.is_ok() {"complete"} else {"error"},
        "path": result.as_ref().ok().map(|path| TraceValue::path(path).to_json())
    }))
}

fn trace_path<T: AsRef<Path>>(path: &T) -> TraceValue {
    TraceValue::path(path.as_ref())
}

fn trace_document_result_paths<T: AsRef<[DocumentRecord]>>(documents: &T) -> TraceValue {
    trace_document_paths(documents.as_ref())
}

fn trace_patient_result_paths(result: &Result<PatientBuildResult, OperationError>) -> TraceValue {
    let paths = result.as_ref().ok().map(|result| {
        json!([
            {"name":"build_root","path":TraceValue::path(&result.build_root).to_json()},
            {"name":"database","path":TraceValue::path(&result.database).to_json()},
            {"name":"manifest","path":TraceValue::path(&result.manifest).to_json()},
            {"name":"checksums","path":TraceValue::path(&result.checksums).to_json()},
            {"name":"report","path":TraceValue::path(&result.report).to_json()}
        ])
    });
    TraceValue::Data(
        json!({"state": if result.is_ok() {"complete"} else {"error"}, "paths": paths}),
    )
}

fn trace_patient_results_paths(
    result: &Result<Vec<PatientBuildResult>, OperationError>,
) -> TraceValue {
    TraceValue::Data(json!({
        "state": if result.is_ok() {"complete"} else {"error"},
        "patients": result.as_ref().ok().map(|patients| patients.iter().enumerate().map(|(index, patient)| json!({
            "index": index,
            "patient_id": patient.patient_id,
            "paths": [
                {"name":"build_root","path":TraceValue::path(&patient.build_root).to_json()},
                {"name":"database","path":TraceValue::path(&patient.database).to_json()},
                {"name":"manifest","path":TraceValue::path(&patient.manifest).to_json()},
                {"name":"checksums","path":TraceValue::path(&patient.checksums).to_json()},
                {"name":"report","path":TraceValue::path(&patient.report).to_json()}
            ]
        })).collect::<Vec<_>>())
    }))
}

/// Modalità di build disponibili.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BuildMode {
    /// Ricalcola il testo e ricostruisce gli artefatti.
    Full,
    /// Riutilizza il testo estratto quando identità e digest coincidono.
    #[default]
    Incremental,
    /// Ricostruisce gli artefatti senza riutilizzare il testo in cache.
    Validation,
}

impl BuildMode {
    fn as_str(self) -> &'static str {
        trace_build_call!("build::BuildMode::as_str", ["self" => &self], || {
            match self {
                Self::Full => "full",
                Self::Incremental => "incremental",
                Self::Validation => "validation",
            }
        })
    }
}

/// Opzioni condivise dalle build singole e multi-persona.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BuildOptions {
    /// Modalità di build.
    pub mode: BuildMode,
    /// Ripete l'OCR dei PDF che contengono già testo digitale sufficiente.
    pub force_pdf_ocr: bool,
    /// Riserva per il livello di diagnostica parametri della baseline.
    pub parameter_debug: bool,
}

/// Contatori della build di una persona.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BuildCounts {
    /// Documenti sorgente distinti.
    pub documents: usize,
    /// Documenti derivati dallo staging dei contenitori.
    pub derived_documents: usize,
    /// Istanze DICOM rilevate.
    pub dicom_instances: usize,
    /// Record sorgente e derivati complessivi.
    pub total_records: usize,
    /// Documenti estratti durante questa build.
    pub extracted_documents: usize,
    /// Documenti riutilizzati dalla cache.
    pub cached_documents: usize,
}

/// Esito della build locale per una persona.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatientBuildResult {
    /// Identificatore stabile della persona.
    pub patient_id: String,
    /// Directory degli artefatti locali.
    pub build_root: PathBuf,
    /// Contatori dei documenti elaborati.
    pub counts: BuildCounts,
    /// Numero di gruppi di duplicati rilevati.
    pub duplicates: usize,
    /// Avvisi non bloccanti.
    pub warnings: usize,
    /// Messaggi degli avvisi non bloccanti.
    pub warning_messages: Vec<String>,
    /// Database SQLite prodotto.
    pub database: PathBuf,
    /// Manifest della build.
    pub manifest: PathBuf,
    /// Checksum di tutti gli artefatti locali.
    pub checksums: PathBuf,
    /// Report della build.
    pub report: PathBuf,
}

struct PreparedPatient {
    person: PersonConfig,
    metadata: CuratedMetadata,
    parameter_rules: Option<ParameterRules>,
}

#[derive(Debug)]
struct ExtractionResult {
    items: Vec<ExtractedText>,
    extracted: usize,
    cached: usize,
}

/// Costruisce gli artefatti locali per una persona.
///
/// La configurazione dei parametri viene validata prima di creare la directory
/// di build. La cache è limitata al testo estratto e resta sotto `local_build`.
///
/// # Errors
///
/// Restituisce un errore se la configurazione, i metadati, una fonte o un
/// artefatto della build non è valido o non può essere elaborato.
pub fn build_patient(
    person: &PersonConfig,
    options: BuildOptions,
) -> Result<PatientBuildResult, OperationError> {
    trace_build_call!(
        "build::build_patient",
        ["person" => person, "options" => &options],
        paths [
            "source_documents" => &person.source_documents,
            "metadata_directory" => &person.metadata_directory,
            "local_build" => &person.local_build,
        ],
        fields ["person_native_paths" => trace_person_paths(person)],
        result_paths ["patient_native_paths" => trace_patient_result_paths],
        || {
            let parameter_rules = load_person_parameter_rules(person)?;
            let metadata = load_curated_metadata(&person.metadata_directory)?;
            build_prepared_patient(person.clone(), metadata, parameter_rules, options)
        }
    )
}

/// Costruisce in ordine tutte le persone abilitate e conserva gli output già
/// completati se una persona successiva fallisce.
///
/// Prima di iniziare verifica privacy e metadati di tutti i profili abilitati.
/// Dopo il preflight le build procedono in ordine di configurazione; un errore
/// interrompe la sequenza e la modalità incrementale permette di riprenderla.
///
/// # Errors
///
/// Restituisce il primo errore di preflight o della pipeline. Gli artefatti
/// già scritti per le persone precedenti restano disponibili.
pub fn build_all(
    config: &AccountsConfig,
    options: BuildOptions,
) -> Result<Vec<PatientBuildResult>, OperationError> {
    trace_build_call!(
        "build::build_all",
        ["config" => config, "options" => &options],
        paths ["config_path" => &config.path],
        fields ["config_native_paths" => trace_accounts_paths(config)],
        result_paths ["patient_native_paths" => trace_patient_results_paths],
        || {
            let root = repository_root(&config.path);
            let prepared_metadata = prepare_people_metadata(config, &root)?;
            let metadata_by_person: HashMap<_, _> = prepared_metadata
                .into_iter()
                .map(|prepared| (prepared.person_id, prepared.metadata))
                .collect();
            let mut patients = Vec::new();
            for person in config.enabled_people() {
                let metadata = metadata_by_person
                    .get(&person.id)
                    .cloned()
                    .ok_or_else(|| build_error(format!("metadati preflight mancanti: {}", person.id)))?;
                let parameter_rules = load_person_parameter_rules(person)?;
                patients.push(PreparedPatient {
                    person: person.clone(),
                    metadata,
                    parameter_rules,
                });
            }
            patients
                .into_iter()
                .map(|patient| {
                    build_prepared_patient(
                        patient.person,
                        patient.metadata,
                        patient.parameter_rules,
                        options,
                    )
                })
                .collect()
        }
    )
}

fn build_prepared_patient(
    person: PersonConfig,
    mut metadata: CuratedMetadata,
    parameter_rules: Option<ParameterRules>,
    options: BuildOptions,
) -> Result<PatientBuildResult, OperationError> {
    trace_build_call!(
        "build::build_prepared_patient",
        [
            "person" => &person,
            "metadata" => &metadata,
            "parameter_rules" => &parameter_rules,
            "options" => &options,
        ],
        paths [],
        fields ["person_native_paths" => trace_person_paths(&person)],
        result_paths ["patient_native_paths" => trace_patient_result_paths],
        || {
    let build_root = &person.local_build;
    fs::create_dir_all(build_root).map_err(|cause| {
        filesystem_error(
            "impossibile creare la directory di build",
            build_root,
            cause,
        )
    })?;

    let downloads = download_confirmed_leaflets(build_root, &metadata.medication_leaflets)?;
    ensure_leaflet_downloads_succeeded(&downloads)?;
    ensure_therapy_leaflets_complete(&person, &metadata)?;
    ensure_observation_imports_current(&person.metadata_directory, &person.source_documents)?;

    let inventory = scan_document_inventory(&person)?;
    let documents = deduplicate_documents(&inventory);
    let staging = stage_container_documents(&person, &documents)?;
    let mut catalog_candidates = inventory;
    catalog_candidates.extend(staging.documents.iter().cloned());
    let duplicates = find_duplicate_documents(&catalog_candidates);
    let catalog_documents = deduplicate_documents(&catalog_candidates);
    let dicom_studies = crate::dicom::catalog_dicom_studies(&person, &catalog_documents);
    let dicom_media = prepare_dicom_media(&person, &dicom_studies)?;
    let dicom_previews = generate_dicom_previews(&person, &dicom_studies)?;

    let extraction_documents: Vec<_> = catalog_documents
        .iter()
        .filter(|document| !document.kind.starts_with("dicom_"))
        .cloned()
        .collect();
    let extraction = extract_documents(&person, &extraction_documents, options)?;

    let mut parameter_discovery = None;
    let mut parameter_result = None;
    if let Some(rules) = &parameter_rules {
        let discovery = discover_configured_candidates(
            &catalog_documents,
            &extraction.items,
            &rules
                .rules
                .iter()
                .filter(|rule| rule.enabled)
                .flat_map(|rule| rule.synonyms.iter().cloned())
                .collect::<Vec<_>>(),
            Some(&parameter_document_hrefs(&person, &catalog_documents)),
            &rules.discovery,
        )
        .map_err(build_error)?;
        let result = build_parameter_slices(&discovery.candidates, rules);
        metadata = merge_parameter_observations(&metadata, &result)?;
        write_parameter_extraction_reports(build_root, &result)?;
        parameter_discovery = Some(discovery);
        parameter_result = Some(result);
    }

    let vital_signs = extract_vital_signs(
        &catalog_documents,
        &extraction.items,
        Some(&parameter_document_hrefs(&person, &catalog_documents)),
    );
    metadata = merge_vital_signs(&metadata, &vital_signs);
    if let (Some(discovery), Some(result)) = (&parameter_discovery, &parameter_result) {
        write_parameter_rejections(build_root, discovery, result, &vital_signs.rejections)?;
    }

    let render_warnings = prepare_consultation_documents(&person, &catalog_documents)?;
    let counts = BuildCounts {
        documents: documents.len(),
        derived_documents: staging.documents.len(),
        dicom_instances: catalog_documents
            .iter()
            .filter(|document| document.kind == "dicom_file")
            .count(),
        total_records: catalog_documents.len(),
        extracted_documents: extraction.extracted,
        cached_documents: extraction.cached,
    };
    let database = build_database(
        &person,
        &catalog_documents,
        &metadata,
        &dicom_studies,
        &extraction.items,
    )?;
    let exports = generate_exports(
        &person,
        &catalog_documents,
        &metadata,
        &dicom_studies,
        &extraction.items,
    )?;
    build_frontend(&person)?;
    let mut warning_messages =
        inspection_warnings(&documents, &duplicates, &dicom_studies, &staging);
    warning_messages.extend(static_document_warning_messages(&staging.documents));
    warning_messages.extend(extraction_warning_messages(
        &extraction_documents,
        &extraction.items,
    ));
    warning_messages.extend(render_warnings);
    warning_messages.extend(exports.warning_messages.iter().cloned());
    warning_messages.extend(
        dicom_media
            .iter()
            .flat_map(|media| media.warnings.iter().cloned()),
    );
    warning_messages.extend(
        dicom_previews
            .iter()
            .flat_map(|preview| preview.warnings.iter().cloned()),
    );

    let manifest = write_manifest(
        &person,
        options.mode,
        counts.documents,
        warning_messages.len(),
    )?;
    let report = write_report(&person, &counts, duplicates.len(), &warning_messages)?;
    let checksums = write_checksums(build_root)?;
    Ok(PatientBuildResult {
        patient_id: person.id,
        build_root: build_root.to_path_buf(),
        counts,
        duplicates: duplicates.len(),
        warnings: warning_messages.len(),
        warning_messages,
        database: database.path,
        manifest,
        checksums,
        report,
    })
        }
    )
}

fn load_person_parameter_rules(
    person: &PersonConfig,
) -> Result<Option<ParameterRules>, OperationError> {
    trace_build_call!(
        "build::load_person_parameter_rules",
        ["person" => person],
        paths [],
        fields ["person_native_paths" => trace_person_paths(person)],
        || {
    let patient_path = person.metadata_directory.join("parameters.toml");
    let common_path = person
        .search
        .dictionary
        .as_ref()
        .and_then(|path| path.parent())
        .map(|directory| directory.join("parameters.toml"))
        .unwrap_or_else(|| person.metadata_directory.join("parameters.common.toml"));
    if !patient_path.exists() && !common_path.exists() {
        return Ok(None);
    }
    load_patient_parameter_rules(&common_path, &patient_path, &person.search.dictionary_data)
        .map(Some)
        }
    )
}

fn deduplicate_documents(documents: &[DocumentRecord]) -> Vec<DocumentRecord> {
    trace_build_call!(
        "build::deduplicate_documents",
        ["documents" => documents],
        paths [],
        fields ["document_native_paths" => trace_document_paths(documents)],
        result_paths ["document_native_paths" => trace_document_result_paths],
        || {
    let mut seen = HashSet::new();
    documents
        .iter()
        .filter(|document| seen.insert(document.document_id.as_str()))
        .cloned()
        .collect()
        }
    )
}

fn extract_documents(
    person: &PersonConfig,
    documents: &[DocumentRecord],
    options: BuildOptions,
) -> Result<ExtractionResult, OperationError> {
    trace_build_call!(
        "build::extract_documents",
        ["person" => person, "documents" => documents, "options" => &options],
        paths [],
        fields ["person_native_paths" => trace_person_paths(person), "document_native_paths" => trace_document_paths(documents)],
        || {
    let cache_path = person.local_build.join("cache/extracted_text.json");
    let cache = if options.mode == BuildMode::Incremental && !options.force_pdf_ocr {
        read_extraction_cache(&cache_path)
    } else {
        BTreeMap::new()
    };
    let mut next_cache = BTreeMap::new();
    let mut items = Vec::with_capacity(documents.len());
    let mut extracted = 0;
    let mut cached = 0;
    for document in documents {
        let cached_item = cache
            .get(&document.document_id)
            .and_then(|entry| cached_extracted_text(entry, document));
        let item = if let Some(item) = cached_item {
            cached += 1;
            item
        } else {
            extracted += 1;
            extract_document_text(person, document, options.force_pdf_ocr)?
        };
        next_cache.insert(
            document.document_id.clone(),
            extraction_cache_entry(document, &item),
        );
        items.push(item);
    }
    write_extraction_cache(&cache_path, &next_cache)?;
    Ok(ExtractionResult {
        items,
        extracted,
        cached,
    })
        }
    )
}

fn extract_document_text(
    person: &PersonConfig,
    document: &DocumentRecord,
    force_pdf_ocr: bool,
) -> Result<ExtractedText, OperationError> {
    trace_build_call!(
        "build::extract_document_text",
        ["person" => person, "document" => document, "force_pdf_ocr" => &force_pdf_ocr],
        paths [],
        fields ["person_native_paths" => trace_person_paths(person), "document_native_path" => TraceValue::path(&document.path)],
        || {
    if document.kind == "pdf" {
        return Ok(extract_pdf_text_with_options(
            document,
            PdfExtractionOptions { force_pdf_ocr },
        ));
    }
    if document.kind == "text" {
        let bytes = fs::read(&document.path).map_err(|cause| {
            filesystem_error(
                "impossibile leggere il documento testuale",
                &document.path,
                cause,
            )
        })?;
        let text = String::from_utf8_lossy(&bytes)
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        return Ok(ExtractedText {
            document_id: document.document_id.clone(),
            text,
            warnings: Vec::new(),
            page_spans: Vec::new(),
        });
    }
    if document.kind == "office" && supports_conversion(document) {
        let directory = person
            .local_build
            .join("rendered-documents")
            .join(&document.document_id);
        return Ok(
            match render_office_document(&document.path, &directory, &document.path) {
                Ok(OfficeRenderOutcome::Rendered(path)) => {
                    let mut pdf = document.clone();
                    pdf.path = path;
                    pdf.kind = "pdf".to_owned();
                    let mut extracted =
                        extract_pdf_text_with_options(&pdf, PdfExtractionOptions { force_pdf_ocr });
                    extracted.document_id.clone_from(&document.document_id);
                    extracted
                }
                Ok(OfficeRenderOutcome::Warning(warning)) => {
                    empty_extraction(document, vec![warning])
                }
                Err(error) => empty_extraction(document, vec![error.message().to_owned()]),
            },
        );
    }
    Ok(empty_extraction(document, Vec::new()))
        }
    )
}

fn empty_extraction(document: &DocumentRecord, warnings: Vec<String>) -> ExtractedText {
    trace_build_call!(
        "build::empty_extraction",
        ["document" => document, "warnings" => &warnings],
        paths [],
        fields ["document_native_path" => TraceValue::path(&document.path)],
        || {
    ExtractedText {
        document_id: document.document_id.clone(),
        text: String::new(),
        warnings,
        page_spans: Vec::new(),
    }
        }
    )
}

fn read_extraction_cache(path: &Path) -> BTreeMap<String, Value> {
    trace_build_call!(
        "build::read_extraction_cache",
        [],
        paths ["path" => path],
        || {
    let Ok(bytes) = fs::read(path) else {
        return BTreeMap::new();
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return BTreeMap::new();
    };
    if value["schema_version"].as_u64() != Some(CACHE_SCHEMA_VERSION) {
        return BTreeMap::new();
    }
    value["documents"]
        .as_object()
        .map(|entries| {
            entries
                .iter()
                .filter(|(_, entry)| entry.is_object())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
        }
    )
}

fn cached_extracted_text(entry: &Value, document: &DocumentRecord) -> Option<ExtractedText> {
    trace_build_call!(
        "build::cached_extracted_text",
        ["entry" => entry, "document" => document],
        paths [],
        fields ["document_native_path" => TraceValue::path(&document.path)],
        || {
    let identity = cache_identity(document);
    if identity
        .iter()
        .any(|(key, value)| entry.get(key) != Some(value))
    {
        return None;
    }
    let text = entry.get("text")?.as_str()?.to_owned();
    let warnings = entry
        .get("warnings")?
        .as_array()?
        .iter()
        .map(|warning| warning.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    let raw_spans: &[Value] = match entry.get("page_spans") {
        Some(spans) => spans.as_array()?.as_slice(),
        None if document
            .path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")) =>
        {
            return None;
        }
        None => &[],
    };
    let page_spans = raw_spans
        .iter()
        .map(|span| {
            Some(TextPageSpan {
                page_number: usize::try_from(span.get("page_number")?.as_u64()?).ok()?,
                character_start: usize::try_from(span.get("character_start")?.as_u64()?).ok()?,
                character_end: usize::try_from(span.get("character_end")?.as_u64()?).ok()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ExtractedText {
        document_id: document.document_id.clone(),
        text,
        warnings,
        page_spans,
    })
        }
    )
}

fn extraction_cache_entry(document: &DocumentRecord, extracted: &ExtractedText) -> Value {
    trace_build_call!(
        "build::extraction_cache_entry",
        ["document" => document, "extracted" => extracted],
        paths [],
        fields ["document_native_path" => TraceValue::path(&document.path)],
        || {
    let mut entry: Map<String, Value> = cache_identity(document).into_iter().collect();
    entry.insert("text".to_owned(), json!(extracted.text));
    entry.insert("warnings".to_owned(), json!(extracted.warnings));
    entry.insert(
        "page_spans".to_owned(),
        json!(
            extracted
                .page_spans
                .iter()
                .map(|span| json!({
                    "page_number": span.page_number,
                    "character_start": span.character_start,
                    "character_end": span.character_end,
                }))
                .collect::<Vec<_>>()
        ),
    );
    Value::Object(entry)
        }
    )
}

fn cache_identity(document: &DocumentRecord) -> Vec<(String, Value)> {
    trace_build_call!(
        "build::cache_identity",
        ["document" => document],
        paths [],
        fields ["document_native_path" => TraceValue::path(&document.path)],
        || {
    vec![
        ("document_id".to_owned(), json!(document.document_id)),
        ("path".to_owned(), json!(document.path.to_string_lossy())),
        ("kind".to_owned(), json!(document.kind)),
        ("sha256".to_owned(), json!(document.sha256)),
        ("origin".to_owned(), json!(document.origin)),
        ("container_id".to_owned(), json!(document.container_id)),
        ("internal_path".to_owned(), json!(document.internal_path)),
    ]
        }
    )
}

fn write_extraction_cache(
    path: &Path,
    documents: &BTreeMap<String, Value>,
) -> Result<(), OperationError> {
    trace_build_call!(
        "build::write_extraction_cache",
        ["documents" => documents],
        paths ["path" => path],
        || {
    let parent = path.parent().expect("cache con directory genitore");
    fs::create_dir_all(parent)
        .map_err(|cause| filesystem_error("impossibile creare la cache", parent, cause))?;
    let bytes = serde_json::to_vec_pretty(&json!({
        "schema_version": CACHE_SCHEMA_VERSION,
        "documents": documents,
    }))
    .map_err(|cause| build_error(format!("cache di estrazione non serializzabile: {cause}")))?;
    let mut output = bytes;
    output.push(b'\n');
    write_bytes(path, &output, "impossibile scrivere la cache di estrazione")
        }
    )
}

fn prepare_consultation_documents(
    person: &PersonConfig,
    documents: &[DocumentRecord],
) -> Result<Vec<String>, OperationError> {
    trace_build_call!(
        "build::prepare_consultation_documents",
        ["person" => person, "documents" => documents],
        paths [],
        fields ["person_native_paths" => trace_person_paths(person), "document_native_paths" => trace_document_paths(documents)],
        || {
    let root = person.local_build.join("rendered-documents");
    match fs::remove_dir_all(&root) {
        Ok(()) => {}
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {}
        Err(cause) => {
            return Err(filesystem_error(
                "impossibile aggiornare i derivati",
                &root,
                cause,
            ));
        }
    }
    fs::create_dir_all(&root).map_err(|cause| {
        filesystem_error("impossibile creare i derivati consultabili", &root, cause)
    })?;
    let mut warnings = Vec::new();
    for document in documents {
        if document.kind.starts_with("dicom_")
            || matches!(document.kind.as_str(), "archive" | "binary")
        {
            continue;
        }
        let suffix = lower_suffix(&document.path);
        if document.origin == "container" && matches!(suffix.as_str(), ".htm" | ".html") {
            continue;
        }
        let browser_suffix = [
            ".pdf", ".txt", ".md", ".htm", ".html", ".jpg", ".jpeg", ".png",
        ]
        .contains(&suffix.as_str());
        if browser_suffix {
            if document.origin == "container" {
                let filename = format!("original{}", suffix);
                let target = root.join(&document.document_id).join(filename);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|cause| {
                        filesystem_error(
                            "impossibile creare la directory del documento derivato",
                            parent,
                            cause,
                        )
                    })?;
                }
                fs::copy(&document.path, &target).map_err(|cause| {
                    filesystem_error("impossibile copiare il documento derivato", &target, cause)
                })?;
            }
            continue;
        }
        if document.kind != "office" {
            continue;
        }
        let directory = root.join(&document.document_id);
        match render_office_document(&document.path, &directory, &document.path)? {
            OfficeRenderOutcome::Rendered(_) => {}
            OfficeRenderOutcome::Warning(warning) => {
                warnings.push(format!("{}: {warning}", document.path.display()));
            }
        }
    }
    Ok(warnings)
        }
    )
}

fn merge_parameter_observations(
    metadata: &CuratedMetadata,
    result: &ParameterBuildResult,
) -> Result<CuratedMetadata, OperationError> {
    trace_build_call!(
        "build::merge_parameter_observations",
        ["metadata" => metadata, "result" => result],
        || {
    let mut merged = metadata.clone();
    let mut replacements = HashMap::new();
    let existing: HashMap<_, _> = metadata
        .observation_series
        .iter()
        .map(|series| (series.id.as_str(), series))
        .collect();
    let mut additions = Vec::new();
    for derived in &result.series {
        if let Some(old) = existing.get(derived.id.as_str()) {
            if !compatible_series(old, derived) {
                return Err(build_error(format!(
                    "serie osservazioni incompatibile per parametro: {}",
                    derived.id
                )));
            }
            continue;
        }
        let targets: Vec<_> = metadata
            .observation_series
            .iter()
            .filter(|series| normalize_label(&series.name) == normalize_label(&derived.name))
            .filter(|series| compatible_series(series, derived))
            .collect();
        match targets.as_slice() {
            [target] => {
                replacements.insert(derived.id.as_str(), target.id.as_str());
            }
            [] => additions.push(derived.clone()),
            _ => {
                return Err(build_error(format!(
                    "serie osservazioni ambigua per parametro: {}",
                    derived.name
                )));
            }
        }
    }
    merged.observation_series.extend(additions);
    merged
        .observation_points
        .extend(result.points.iter().cloned().map(|mut point| {
            if let Some(replacement) = replacements.get(point.series_id.as_str()) {
                point.series_id = (*replacement).to_owned();
            }
            point
        }));
    Ok(merged)
        }
    )
}

fn compatible_series(
    left: &crate::metadata::ObservationSeries,
    right: &crate::metadata::ObservationSeries,
) -> bool {
    trace_build_call!(
        "build::compatible_series",
        ["left" => left, "right" => right],
        || {
            left.value_type == right.value_type
                && match (&left.unit, &right.unit) {
                    (None, None) => true,
                    (Some(left), Some(right)) => normalize_label(left) == normalize_label(right),
                    _ => false,
                }
        }
    )
}

fn parameter_document_hrefs(
    person: &PersonConfig,
    documents: &[DocumentRecord],
) -> HashMap<String, String> {
    trace_build_call!(
        "build::parameter_document_hrefs",
        ["person" => person, "documents" => documents],
        paths [],
        fields ["person_native_paths" => trace_person_paths(person), "document_native_paths" => trace_document_paths(documents)],
        || {
            documents
                .iter()
                .filter(|document| document.origin == "source")
                .filter_map(|document| {
                    let relative = document.path.strip_prefix(&person.source_documents).ok()?;
                    Some((
                        document.document_id.clone(),
                        format!(
                            "../documents/{}",
                            relative.to_string_lossy().replace('\\', "/")
                        ),
                    ))
                })
                .collect()
        }
    )
}

fn inspection_warnings(
    documents: &[DocumentRecord],
    duplicates: &BTreeMap<String, Vec<DocumentRecord>>,
    studies: &[DicomStudy],
    staging: &ContainerStagingResult,
) -> Vec<String> {
    trace_build_call!(
        "build::inspection_warnings",
        ["documents" => documents, "duplicates" => duplicates, "studies" => studies, "staging" => staging],
        paths [],
        fields ["document_native_paths" => trace_document_paths(documents)],
        || {
    let mut warnings = Vec::new();
    for (digest, copies) in duplicates {
        if let Some(retained) = copies.first() {
            for skipped in copies.iter().skip(1) {
                warnings.push(format!(
                    "contenuto documento duplicato saltato. I file seguenti sono identici (sha256={digest}): \n{}\n{}",
                    retained.path.display(),
                    skipped.path.display()
                ));
            }
        }
    }
    warnings.extend(studies.iter().flat_map(|study| {
        study
            .warnings
            .iter()
            .map(|warning| format!("{}: {warning}", study.support_path.display()))
    }));
    warnings.extend(static_document_warning_messages(documents));
    warnings.extend(
        staging
            .warning_messages
            .iter()
            .filter(|warning| !warning.contains("contenitore DICOM non espanso"))
            .cloned(),
    );
    warnings
        }
    )
}

fn static_document_warning_messages(documents: &[DocumentRecord]) -> Vec<String> {
    trace_build_call!(
        "build::static_document_warning_messages",
        ["documents" => documents],
        paths [],
        fields ["document_native_paths" => trace_document_paths(documents)],
        || {
            documents
                .iter()
                .filter(|document| document.kind == "binary")
                .map(|document| {
                    let suffix = document
                        .path
                        .extension()
                        .map(|extension| format!(".{}", extension.to_string_lossy().to_lowercase()))
                        .unwrap_or_else(|| "file senza estensione".to_owned());
                    format!(
                        "{}: estrazione testo non supportata per {suffix}",
                        document.path.display()
                    )
                })
                .collect()
        }
    )
}

fn extraction_warning_messages(
    documents: &[DocumentRecord],
    extracted: &[ExtractedText],
) -> Vec<String> {
    trace_build_call!(
        "build::extraction_warning_messages",
        ["documents" => documents, "extracted" => extracted],
        || {
            documents
                .iter()
                .zip(extracted)
                .flat_map(|(document, item)| {
                    item.warnings
                        .iter()
                        .map(|warning| format!("{}: {warning}", document.path.display()))
                        .collect::<Vec<_>>()
                })
                .collect()
        }
    )
}

fn ensure_leaflet_downloads_succeeded(
    downloads: &LeafletDownloadResult,
) -> Result<(), OperationError> {
    trace_build_call!(
        "build::ensure_leaflet_downloads_succeeded",
        ["downloads" => downloads],
        || {
    if downloads.failures.is_empty() {
        return Ok(());
    }
    let details = downloads
        .failures
        .iter()
        .map(|failure| {
            format!(
                "farmaco={} documento={} {} {}",
                failure.medication_id, failure.kind, failure.reason, failure.url
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    Err(build_error(format!(
        "download documenti AIFA non riuscito: {details}"
    )))
        }
    )
}

fn ensure_therapy_leaflets_complete(
    person: &PersonConfig,
    metadata: &CuratedMetadata,
) -> Result<(), OperationError> {
    trace_build_call!(
        "build::ensure_therapy_leaflets_complete",
        ["person" => person, "metadata" => metadata],
        paths [],
        fields ["person_native_paths" => trace_person_paths(person)],
        || {
    let medication_ids: HashSet<_> = metadata
        .therapies
        .iter()
        .map(|therapy| therapy.medication_id.as_str())
        .collect();
    let references: HashSet<_> = metadata
        .medication_leaflets
        .iter()
        .map(|reference| reference.medication_id.as_str())
        .collect();
    let exclusions: HashSet<_> = metadata
        .medication_leaflet_exclusions
        .iter()
        .map(|exclusion| exclusion.medication_id.as_str())
        .collect();
    let mut unresolved: Vec<_> = medication_ids
        .difference(&references)
        .filter(|id| !exclusions.contains(**id))
        .copied()
        .collect();
    unresolved.sort_unstable();
    if !unresolved.is_empty() {
        return Err(build_error(format!(
            "{}: farmaci in terapia senza riferimento AIFA o stato non_aifa: {}",
            person
                .metadata_directory
                .join("medication_leaflets.toml")
                .display(),
            unresolved.join(", ")
        )));
    }
    let mut incomplete = Vec::new();
    for medication_id in medication_ids.intersection(&references) {
        let directory = person
            .local_build
            .join("medication-leaflets")
            .join(medication_id);
        let missing: Vec<_> = ["foglio-illustrativo.pdf", "rcp.pdf"]
            .into_iter()
            .filter(|filename| !directory.join(filename).is_file())
            .collect();
        if !missing.is_empty() {
            incomplete.push(format!("{medication_id} ({})", missing.join(", ")));
        }
    }
    if incomplete.is_empty() {
        return Ok(());
    }
    incomplete.sort();
    Err(build_error(format!(
        "documenti AIFA locali mancanti dopo il download: {}",
        incomplete.join(", ")
    )))
        }
    )
}

fn write_manifest(
    person: &PersonConfig,
    mode: BuildMode,
    documents: usize,
    warnings: usize,
) -> Result<PathBuf, OperationError> {
    trace_build_call!(
        "build::write_manifest",
        ["person" => person, "mode" => &mode, "documents" => &documents, "warnings" => &warnings],
        paths [],
        fields ["person_native_paths" => trace_person_paths(person)],
        result_paths ["manifest_native_path" => trace_path_result],
        || {
    let path = person.local_build.join("manifests/manifest.json");
    let base = json!({
        "schema_version": 1,
        "patient_id": person.id,
        "build_mode": mode.as_str(),
        "documents": documents,
        "warnings": warnings,
    });
    let generated_at = fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(|existing| {
            base.as_object().is_some_and(|fields| {
                fields
                    .iter()
                    .all(|(key, value)| existing.get(key) == Some(value))
            })
        })
        .and_then(|existing| existing.get("generated_at")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true));
    let mut payload = base;
    payload["generated_at"] = json!(generated_at);
    write_json(&path, &payload, "impossibile scrivere il manifest build")?;
    Ok(path)
        }
    )
}

fn write_report(
    person: &PersonConfig,
    counts: &BuildCounts,
    duplicates: usize,
    warning_messages: &[String],
) -> Result<PathBuf, OperationError> {
    trace_build_call!(
        "build::write_report",
        ["person" => person, "counts" => counts, "duplicates" => &duplicates, "warning_messages" => warning_messages],
        paths [],
        fields ["person_native_paths" => trace_person_paths(person)],
        result_paths ["report_native_path" => trace_path_result],
        || {
    let path = person.local_build.join("reports/build_report.json");
    write_json(
        &path,
        &json!({
            "patient_id": person.id,
            "documents": counts.documents,
            "derived_documents": counts.derived_documents,
            "dicom_instances": counts.dicom_instances,
            "total_records": counts.total_records,
            "duplicates": duplicates,
            "warnings": warning_messages.len(),
            "warning_messages": warning_messages,
        }),
        "impossibile scrivere il report build",
    )?;
    Ok(path)
        }
    )
}

fn write_checksums(build_root: &Path) -> Result<PathBuf, OperationError> {
    trace_build_call!(
        "build::write_checksums",
        ["build_root" => build_root],
        paths [],
        fields ["build_root_native_path" => TraceValue::path(build_root)],
        result_paths ["checksums_native_path" => trace_path_result],
        || {
    let target = build_root.join("checksums.sha256");
    let mut paths = Vec::new();
    collect_files(build_root, &mut paths, 0)?;
    paths.sort();
    let mut rows = Vec::new();
    for path in paths.into_iter().filter(|path| path != &target) {
        rows.push(format!(
            "{}  {}",
            sha256_file(&path)?,
            path.strip_prefix(build_root)
                .expect("file checksum sotto build root")
                .to_string_lossy()
                .replace('\\', "/")
        ));
    }
    let mut contents = rows.join("\n");
    if !contents.is_empty() {
        contents.push('\n');
    }
    write_bytes(
        &target,
        contents.as_bytes(),
        "impossibile scrivere i checksum",
    )?;
    Ok(target)
        }
    )
}

fn collect_files(
    directory: &Path,
    files: &mut Vec<PathBuf>,
    depth: usize,
) -> Result<(), OperationError> {
    let files_length_before = files.len();
    let trace = trace_current(Component::Core, "build::collect_files", || {
        if depth == 0 {
            Vec::from([
                TraceField::new("directory", TraceValue::path(directory)),
                TraceField::new("depth", trace_debug(&depth)),
                TraceField::new("files_before", trace_path_vector(files)),
            ])
        } else {
            Vec::from([
                TraceField::new("directory", TraceValue::path(directory)),
                TraceField::new("depth", trace_debug(&depth)),
                TraceField::new(
                    "files_state_reference",
                    TraceValue::Data(json!({
                        "base": "root_call_files_before",
                        "length_at_entry": files_length_before,
                        "root_call_resolution": "nearest depth=0 build::collect_files ancestor via parent_call_id/call_id",
                        "reconstruction": "follow call_id/parent_call_id tree; combine paths_added from the root subtree and select indices below length_at_entry",
                    })),
                ),
            ])
        }
    });
    let trace_enabled = trace.is_some();
    let mut paths_added = Vec::new();
    let result = (|| {
        for entry in fs::read_dir(directory).map_err(|cause| {
            filesystem_error("impossibile leggere la directory build", directory, cause)
        })? {
            let entry = entry.map_err(|cause| {
                filesystem_error("impossibile leggere un artefatto build", directory, cause)
            })?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|cause| {
                filesystem_error("impossibile determinare l'artefatto build", &path, cause)
            })?;
            if kind.is_dir() {
                collect_files(&path, files, depth + 1)?;
            } else if kind.is_file() {
                files.push(path);
                if trace_enabled {
                    let index = files.len() - 1;
                    paths_added.push(json!({
                        "index": index,
                        "path": TraceValue::path(&files[index]).to_json(),
                    }));
                }
            }
        }
        Ok(())
    })();
    if let Some(trace) = trace {
        let result = json!({
            "format": "rust_debug_with_reconstructible_path_delta",
            "value": {
                "return": redact_secret_assignments(&format!("{result:#?}")),
                "depth": depth,
                "files_length_after": files.len(),
                "paths_added": paths_added,
                "files_after": if depth == 0 { Some(trace_path_vector(files).to_json()) } else { None },
            },
        });
        let _ = trace.finish(TraceValue::Data(result));
    }
    result
}

fn sha256_file(path: &Path) -> Result<String, OperationError> {
    trace_build_call!("build::sha256_file", [], paths ["path" => path], || {
        let bytes = fs::read(path)
            .map_err(|cause| filesystem_error("impossibile calcolare il checksum", path, cause))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    })
}

fn write_json(path: &Path, value: &Value, context: &str) -> Result<(), OperationError> {
    trace_build_call!(
        "build::write_json",
        ["value" => value, "context" => &context],
        paths ["path" => path],
        || {
    let mut contents = serde_json::to_vec_pretty(value).map_err(|cause| {
        build_error(format!("serializzazione JSON build non riuscita: {cause}"))
    })?;
    contents.push(b'\n');
    write_bytes(path, &contents, context)
        }
    )
}

fn write_bytes(path: &Path, contents: &[u8], context: &str) -> Result<(), OperationError> {
    trace_build_call!(
        "build::write_bytes",
        ["contents" => contents, "context" => &context],
        paths ["path" => path],
        || {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|cause| {
            filesystem_error("impossibile creare la directory build", parent, cause)
        })?;
    }
    fs::write(path, contents).map_err(|cause| filesystem_error(context, path, cause))
        }
    )
}

fn repository_root(config_path: &Path) -> PathBuf {
    trace_build_call!("build::repository_root", [], paths ["config_path" => config_path], result_paths ["repository_root_native_path" => trace_path], || {
        let directory = config_path.parent().unwrap_or_else(|| Path::new("."));
        if directory.file_name().is_some_and(|name| name == "config") {
            directory.parent().unwrap_or(directory).to_path_buf()
        } else {
            directory.to_path_buf()
        }
    })
}

fn lower_suffix(path: &Path) -> String {
    trace_build_call!("build::lower_suffix", [], paths ["path" => path], || {
        path.extension()
            .map(|extension| format!(".{}", extension.to_string_lossy().to_ascii_lowercase()))
            .unwrap_or_default()
    })
}

fn build_error(message: impl Into<String>) -> OperationError {
    let message = message.into();
    trace_build_call!("build::build_error", ["message" => &message], || {
        OperationError::new(ErrorCode::Configuration, message)
    })
}

fn filesystem_error(context: &str, path: &Path, cause: std::io::Error) -> OperationError {
    trace_build_call!(
        "build::filesystem_error",
        ["context" => &context, "path" => path, "cause" => &cause],
        || { build_error(format!("{context} {}: {cause}", path.display())) }
    )
}

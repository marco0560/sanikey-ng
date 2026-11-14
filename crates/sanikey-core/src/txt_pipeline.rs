//! Pipeline consultabili per file TXT, PDF e formati browser-apribili.

#[cfg(test)]
mod tests;

use crate::config::PersonConfig;
use crate::inventory::scan_documents;
use crate::logging::{
    Component as LogComponent, TraceField, TraceOmission, TraceValue, trace_current,
    trace_json_value, trace_sensitive_text,
};
use crate::metadata::{DocumentRecord, load_curated_metadata};
use crate::office::{OfficeRenderOutcome, render_office_document, supports_conversion};
use crate::operation::{ErrorCode, OperationError};
use crate::pdf::{PdfExtractionOptions, TextPageSpan, extract_pdf_text_with_options};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DATABASE: AtomicUsize = AtomicUsize::new(0);

macro_rules! trace_pipeline_call {
    ($function:literal, [$(TraceField::new($name:expr, $field:expr $(,)?)),* $(,)?], $body:expr, $result_paths:expr) => {{
        let trace = trace_current(LogComponent::Core, $function, || {
            vec![$(
                TraceField::new($name, sanitize_trace_field($function, $name, $field)),
            )*]
        });
        let result = ($body)();
        if let Some(trace) = trace {
            let path_projection = ($result_paths)(&result);
            let value = json!({
                "result": result.trace_value().to_json(),
                "path_projection": path_projection,
            });
            let _ = trace.finish(TraceValue::Data(redact_trace_json(value)));
        }
        result
    }};
}

fn redact_trace_json(value: Value) -> Value {
    match trace_json_value(&value) {
        TraceValue::Data(value) => value,
        TraceValue::Secret | TraceValue::Omitted(_) => Value::Null,
    }
}

fn sanitize_trace_value(value: TraceValue) -> TraceValue {
    sanitize_trace_field("", "", value)
}

fn sanitize_trace_field(function: &str, name: &str, value: TraceValue) -> TraceValue {
    match value {
        TraceValue::Data(value) => {
            let value = if function == "txt_pipeline::write_file" && name == "contents" {
                match value {
                    Value::String(contents)
                        if contents.trim_start().starts_with("<!doctype html>")
                            || contents.trim_start().starts_with("<html") =>
                    {
                        Value::String(decode_html_text(&contents))
                    }
                    value => value,
                }
            } else {
                value
            };
            trace_json_value(&value)
        }
        TraceValue::Secret => TraceValue::Secret,
        TraceValue::Omitted(reason) => TraceValue::Omitted(reason),
    }
}

fn decode_html_text(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn trace_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> TraceValue {
    trace_sensitive_text(&format!("{value:#?}"))
}

/// Converte i risultati della pipeline nella forma registrabile in Trace.
trait PipelineTraceOutput {
    fn trace_value(&self) -> TraceValue;
}

/// Redige e registra i valori e gli errori della pipeline.
impl<T: std::fmt::Debug> PipelineTraceOutput for Result<T, OperationError> {
    fn trace_value(&self) -> TraceValue {
        match self {
            Ok(value) => trace_debug(value),
            Err(error) => TraceValue::Data(json!({
                "state":"error",
                "code":error.code().as_str(),
                "message":trace_sensitive_text(error.message()).to_json(),
            })),
        }
    }
}

/// Registra direttamente gli esiti booleani.
impl PipelineTraceOutput for bool {
    fn trace_value(&self) -> TraceValue {
        TraceValue::data(*self)
    }
}

/// Registra l'elenco testuale restituito dalla pipeline.
impl PipelineTraceOutput for Vec<String> {
    fn trace_value(&self) -> TraceValue {
        TraceValue::Data(json!(self))
    }
}

/// Decodifica e redige il testo prodotto dalla pipeline.
impl PipelineTraceOutput for String {
    fn trace_value(&self) -> TraceValue {
        trace_sensitive_text(&decode_html_text(self))
    }
}

/// Registra un percorso usando la codifica nativa lossless.
impl PipelineTraceOutput for PathBuf {
    fn trace_value(&self) -> TraceValue {
        TraceValue::path(self)
    }
}

/// Registra codice e messaggio redatto dell'errore.
impl PipelineTraceOutput for OperationError {
    fn trace_value(&self) -> TraceValue {
        TraceValue::Data(json!({
            "state":"error",
            "code":self.code().as_str(),
            "message":trace_sensitive_text(self.message()).to_json(),
        }))
    }
}

fn trace_person(person: &PersonConfig) -> TraceValue {
    TraceValue::Data(json!({
        "id":person.id,
        "display_name":person.display_name,
        "source_documents":TraceValue::path(&person.source_documents).to_json(),
        "metadata_directory":TraceValue::path(&person.metadata_directory).to_json(),
        "local_build":TraceValue::path(&person.local_build).to_json(),
        "enabled":person.enabled,
        "ui":trace_debug(&person.ui).to_json(),
        "search":trace_debug(&person.search).to_json(),
        "ingestion":trace_debug(&person.ingestion).to_json(),
    }))
}

fn trace_document_record(record: &DocumentRecord) -> TraceValue {
    TraceValue::Data(json!({
        "document_id":record.document_id,
        "patient_id":record.patient_id,
        "path":TraceValue::path(&record.path).to_json(),
        "title":record.title,
        "category":record.category,
        "kind":record.kind,
        "sha256":record.sha256,
        "date":record.date,
        "series":record.series,
        "tags":record.tags,
        "origin":record.origin,
        "container_id":record.container_id,
        "internal_path":record.internal_path,
    }))
}

fn trace_document_paths(documents: &[TxtDocument]) -> Value {
    json!(
        documents
            .iter()
            .map(|document| json!({
                "document_id": document.record.document_id,
                "source_path": TraceValue::path(&document.record.path).to_json(),
            }))
            .collect::<Vec<_>>()
    )
}

struct DatabaseStage {
    path: PathBuf,
    committed: bool,
}

/// Rimuove il database temporaneo se la pipeline non lo ha committato.
impl Drop for DatabaseStage {
    fn drop(&mut self) {
        let trace = trace_current(
            LogComponent::Core,
            "txt_pipeline::DatabaseStage::drop",
            || {
                vec![
                    TraceField::new("path", sanitize_trace_value(TraceValue::path(&self.path))),
                    TraceField::new(
                        "committed",
                        sanitize_trace_value(TraceValue::Data(json!(self.committed))),
                    ),
                ]
            },
        );
        let cleanup = if !self.committed {
            let database_file = fs::remove_file(&self.path)
                .map(|()| json!("removed"))
                .unwrap_or_else(|error| json!({"error_kind": format!("{:?}", error.kind())}));
            let mut journal = self.path.as_os_str().to_os_string();
            journal.push("-journal");
            let journal_file = fs::remove_file(PathBuf::from(journal))
                .map(|()| json!("removed"))
                .unwrap_or_else(|error| json!({"error_kind": format!("{:?}", error.kind())}));
            Some(json!({"database_file": database_file, "journal_file": journal_file}))
        } else {
            None
        };
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(redact_trace_json(
                json!({"cleanup": cleanup}),
            )));
        }
    }
}

/// Risultati e percorsi prodotti da una pipeline di documenti consultabili.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentPipelineResult {
    /// Numero di documenti distinti elaborati.
    pub documents: usize,
    /// Percorso del database SQLite generato.
    pub database: PathBuf,
    /// Percorso dei dati JSON esportati.
    pub data: PathBuf,
    /// Percorso della pagina statica consultabile.
    pub frontend: PathBuf,
}

/// Nome storico del risultato restituito dalle pipeline TXT e PDF.
pub type TxtPipelineResult = DocumentPipelineResult;

/// Cataloga i formati apribili dal browser e converte Office in PDF derivati.
///
/// Include PDF, TXT, Markdown, HTML, immagini JPEG/PNG e formati Office con
/// convertitore baseline. Originali Office e immagini restano invariati.
pub fn build_consultation_pipeline(
    person: &PersonConfig,
) -> Result<DocumentPipelineResult, OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::build_consultation_pipeline",
        [
            TraceField::new("person", sanitize_trace_value(trace_person(person))),
            TraceField::new(
                "source_documents",
                TraceValue::path(&person.source_documents)
            ),
            TraceField::new(
                "metadata_directory",
                TraceValue::path(&person.metadata_directory)
            ),
            TraceField::new("local_build", TraceValue::path(&person.local_build)),
        ],
        || build_consultation_pipeline_with_renderer(person, render_office_document),
        |result: &Result<DocumentPipelineResult, OperationError>| match result {
            Ok(result) =>
                json!({"database": TraceValue::path(&result.database).to_json(), "data": TraceValue::path(&result.data).to_json(), "frontend": TraceValue::path(&result.frontend).to_json()}),
            Err(_) => Value::Null,
        }
    )
}

fn build_consultation_pipeline_with_renderer(
    person: &PersonConfig,
    office_renderer: fn(&Path, &Path, &Path) -> Result<OfficeRenderOutcome, OperationError>,
) -> Result<DocumentPipelineResult, OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::build_consultation_pipeline_with_renderer",
        [
            TraceField::new("person", sanitize_trace_value(trace_person(person))),
            TraceField::new(
                "source_documents",
                TraceValue::path(&person.source_documents)
            ),
            TraceField::new(
                "metadata_directory",
                TraceValue::path(&person.metadata_directory)
            ),
            TraceField::new("local_build", TraceValue::path(&person.local_build)),
            TraceField::new(
                "office_renderer",
                TraceValue::Omitted(TraceOmission::UnsupportedType)
            ),
        ],
        || {
            let rendered_documents = person.local_build.join("rendered-documents");
            match fs::remove_dir_all(&rendered_documents) {
                Ok(()) => {}
                Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {}
                Err(cause) => {
                    return Err(filesystem_error(
                        "impossibile aggiornare i PDF Office derivati",
                        &rendered_documents,
                        cause,
                    ));
                }
            }
            build_pipeline(
                person,
                is_consultable_source,
                |person, record, tags, options| {
                    read_consultation_document(person, record, tags, options, office_renderer)
                },
                "Documenti consultabili",
            )
        },
        |result: &Result<DocumentPipelineResult, OperationError>| match result {
            Ok(result) =>
                json!({"database": TraceValue::path(&result.database).to_json(), "data": TraceValue::path(&result.data).to_json(), "frontend": TraceValue::path(&result.frontend).to_json()}),
            Err(_) => Value::Null,
        }
    )
}

/// Legge i PDF e li rende consultabili, eseguendo OCR sui PDF scansionati.
///
/// I documenti originali vengono copiati invariati; testo, warning e
/// provenienza delle pagine sono esportati in JSON e SQLite.
pub fn build_pdf_pipeline(person: &PersonConfig) -> Result<TxtPipelineResult, OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::build_pdf_pipeline",
        [
            TraceField::new("person", trace_person(person)),
            TraceField::new(
                "source_documents",
                TraceValue::path(&person.source_documents)
            ),
            TraceField::new(
                "metadata_directory",
                TraceValue::path(&person.metadata_directory)
            ),
            TraceField::new("local_build", TraceValue::path(&person.local_build)),
        ],
        || build_pdf_pipeline_with_options(person, PdfExtractionOptions::default()),
        |result: &Result<TxtPipelineResult, OperationError>| match result {
            Ok(result) =>
                json!({"database": TraceValue::path(&result.database).to_json(), "data": TraceValue::path(&result.data).to_json(), "frontend": TraceValue::path(&result.frontend).to_json()}),
            Err(_) => Value::Null,
        }
    )
}

/// Legge i PDF applicando opzioni OCR esplicite.
pub fn build_pdf_pipeline_with_options(
    person: &PersonConfig,
    options: PdfExtractionOptions,
) -> Result<TxtPipelineResult, OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::build_pdf_pipeline_with_options",
        [
            TraceField::new("person", trace_person(person)),
            TraceField::new("options", trace_debug(&options)),
            TraceField::new(
                "source_documents",
                TraceValue::path(&person.source_documents)
            ),
            TraceField::new(
                "metadata_directory",
                TraceValue::path(&person.metadata_directory)
            ),
            TraceField::new("local_build", TraceValue::path(&person.local_build)),
        ],
        || build_pipeline(
            person,
            is_pdf,
            |person, record, tags, _| read_pdf_document(person, record, tags, options),
            "Archivio PDF",
        ),
        |result: &Result<TxtPipelineResult, OperationError>| match result {
            Ok(result) =>
                json!({"database": TraceValue::path(&result.database).to_json(), "data": TraceValue::path(&result.data).to_json(), "frontend": TraceValue::path(&result.frontend).to_json()}),
            Err(_) => Value::Null,
        }
    )
}

#[derive(Clone, Debug, PartialEq)]
struct TxtDocument {
    record: DocumentRecord,
    relative_path: String,
    href: Option<String>,
    text: String,
    warnings: Vec<String>,
    page_spans: Vec<TextPageSpan>,
    tags: Vec<String>,
}

type ConfiguredTags = indexmap::IndexMap<String, Vec<String>>;
/// Legge i documenti `.txt`, crea l'indice SQLite e genera export e pagina HTML.
///
/// Il perimetro comprende solo file con estensione `.txt`, in UTF-8 con
/// sostituzione dei byte non validi. Le righe CRLF e CR diventano LF, come
/// nella lettura testuale Python della baseline. Gli altri formati non vengono
/// estratti né mostrati.
///
/// # Errors
///
/// Restituisce un errore di configurazione se la scansione, la lettura, il
/// database o la scrittura di un artefatto non riesce.
pub fn build_txt_pipeline(person: &PersonConfig) -> Result<TxtPipelineResult, OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::build_txt_pipeline",
        [
            TraceField::new("person", trace_person(person)),
            TraceField::new(
                "source_documents",
                TraceValue::path(&person.source_documents)
            ),
            TraceField::new(
                "metadata_directory",
                TraceValue::path(&person.metadata_directory)
            ),
            TraceField::new("local_build", TraceValue::path(&person.local_build)),
        ],
        || build_pipeline(person, is_plain_text, read_document, "Archivio TXT"),
        |result: &Result<TxtPipelineResult, OperationError>| match result {
            Ok(result) =>
                json!({"database": TraceValue::path(&result.database).to_json(), "data": TraceValue::path(&result.data).to_json(), "frontend": TraceValue::path(&result.frontend).to_json()}),
            Err(_) => Value::Null,
        }
    )
}

fn build_pipeline(
    person: &PersonConfig,
    include: fn(&DocumentRecord) -> bool,
    read: impl Fn(
        &PersonConfig,
        DocumentRecord,
        &ConfiguredTags,
        PdfExtractionOptions,
    ) -> Result<TxtDocument, OperationError>,
    heading: &str,
) -> Result<TxtPipelineResult, OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::build_pipeline",
        [
            TraceField::new("person", trace_person(person)),
            TraceField::new(
                "source_documents",
                TraceValue::path(&person.source_documents)
            ),
            TraceField::new(
                "metadata_directory",
                TraceValue::path(&person.metadata_directory)
            ),
            TraceField::new("local_build", TraceValue::path(&person.local_build)),
            TraceField::new(
                "include_predicate",
                TraceValue::Omitted(TraceOmission::UnsupportedType)
            ),
            TraceField::new(
                "reader",
                TraceValue::Omitted(TraceOmission::UnsupportedType)
            ),
            TraceField::new("heading", TraceValue::Data(json!(heading))),
        ],
        || {
            let inventory = scan_documents(person)?;
            let metadata = load_curated_metadata(&person.metadata_directory)?;
            let mut documents = Vec::new();
            for record in inventory.into_iter().filter(include) {
                documents.push(read(
                    person,
                    record,
                    &metadata.document_tags,
                    PdfExtractionOptions::default(),
                )?);
            }

            let database = person
                .local_build
                .join("database")
                .join("medical_archive.db");
            let data = person
                .local_build
                .join("web")
                .join("data")
                .join("documents.json");
            let frontend = person.local_build.join("web").join("index.html");

            build_database(&database, &documents, &person.id)?;
            export_documents(&data, person, &documents)?;
            build_frontend(&frontend, person, &documents, heading)?;

            Ok(TxtPipelineResult {
                documents: documents.len(),
                database,
                data,
                frontend,
            })
        },
        |result: &Result<TxtPipelineResult, OperationError>| match result {
            Ok(result) =>
                json!({"database": TraceValue::path(&result.database).to_json(), "data": TraceValue::path(&result.data).to_json(), "frontend": TraceValue::path(&result.frontend).to_json()}),
            Err(_) => Value::Null,
        }
    )
}

fn is_pdf(record: &DocumentRecord) -> bool {
    trace_pipeline_call!(
        "txt_pipeline::is_pdf",
        [
            TraceField::new("record", trace_document_record(record)),
            TraceField::new("document_path", TraceValue::path(&record.path)),
        ],
        || record.kind == "pdf"
            && record
                .path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf")),
        |_| Value::Null
    )
}

fn is_plain_text(record: &DocumentRecord) -> bool {
    trace_pipeline_call!(
        "txt_pipeline::is_plain_text",
        [
            TraceField::new("record", trace_document_record(record)),
            TraceField::new("document_path", TraceValue::path(&record.path)),
        ],
        || record.kind == "text"
            && record
                .path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("txt")),
        |_| Value::Null
    )
}

fn is_browser_openable_source(record: &DocumentRecord) -> bool {
    trace_pipeline_call!(
        "txt_pipeline::is_browser_openable_source",
        [
            TraceField::new("record", trace_document_record(record)),
            TraceField::new("document_path", TraceValue::path(&record.path)),
        ],
        || {
            let Some(extension) = record.path.extension() else {
                return false;
            };
            let extension = extension.to_string_lossy();
            match record.kind.as_str() {
                "pdf" => extension.eq_ignore_ascii_case("pdf"),
                "text" => {
                    extension.eq_ignore_ascii_case("txt") || extension.eq_ignore_ascii_case("md")
                }
                "office" => {
                    extension.eq_ignore_ascii_case("htm") || extension.eq_ignore_ascii_case("html")
                }
                "image" => {
                    extension.eq_ignore_ascii_case("jpg")
                        || extension.eq_ignore_ascii_case("jpeg")
                        || extension.eq_ignore_ascii_case("png")
                }
                _ => false,
            }
        },
        |_| Value::Null
    )
}

fn is_consultable_source(record: &DocumentRecord) -> bool {
    trace_pipeline_call!(
        "txt_pipeline::is_consultable_source",
        [
            TraceField::new("record", trace_document_record(record)),
            TraceField::new("document_path", TraceValue::path(&record.path)),
        ],
        || is_browser_openable_source(record) || supports_conversion(record),
        |_| Value::Null
    )
}

fn read_consultation_document(
    person: &PersonConfig,
    record: DocumentRecord,
    configured_tags: &ConfiguredTags,
    options: PdfExtractionOptions,
    office_renderer: fn(&Path, &Path, &Path) -> Result<OfficeRenderOutcome, OperationError>,
) -> Result<TxtDocument, OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::read_consultation_document",
        [
            TraceField::new("person", trace_person(person)),
            TraceField::new(
                "source_documents",
                TraceValue::path(&person.source_documents)
            ),
            TraceField::new("local_build", TraceValue::path(&person.local_build)),
            TraceField::new("record", trace_document_record(&record)),
            TraceField::new("document_path", TraceValue::path(&record.path)),
            TraceField::new("configured_tags", trace_debug(configured_tags)),
            TraceField::new("options", trace_debug(&options)),
            TraceField::new(
                "office_renderer",
                TraceValue::Omitted(TraceOmission::UnsupportedType)
            ),
        ],
        || match record.kind.as_str() {
            "pdf" => read_pdf_document(person, record, configured_tags, options),
            "text" => read_document(person, record, configured_tags, options),
            "office" if supports_conversion(&record) => {
                read_office_document(person, record, configured_tags, options, office_renderer)
            }
            _ => read_link_only_document(person, record, configured_tags),
        },
        |result: &Result<TxtDocument, OperationError>| match result {
            Ok(document) =>
                json!({"record_path": TraceValue::path(&document.record.path).to_json()}),
            Err(_) => Value::Null,
        }
    )
}

fn read_office_document(
    person: &PersonConfig,
    record: DocumentRecord,
    configured_tags: &ConfiguredTags,
    options: PdfExtractionOptions,
    office_renderer: fn(&Path, &Path, &Path) -> Result<OfficeRenderOutcome, OperationError>,
) -> Result<TxtDocument, OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::read_office_document",
        [
            TraceField::new("person", trace_person(person)),
            TraceField::new(
                "source_documents",
                TraceValue::path(&person.source_documents)
            ),
            TraceField::new("record", trace_document_record(&record)),
            TraceField::new("document_path", TraceValue::path(&record.path)),
            TraceField::new("configured_tags", trace_debug(configured_tags)),
            TraceField::new("options", trace_debug(&options)),
            TraceField::new(
                "office_renderer",
                TraceValue::Omitted(TraceOmission::UnsupportedType)
            ),
            TraceField::new("local_build", TraceValue::path(&person.local_build)),
        ],
        || {
            let relative_path = relative_document_path(person, &record.path)?;
            let tags = tags_for_document(&record, &relative_path, configured_tags);
            let output_directory = person
                .local_build
                .join("rendered-documents")
                .join(&record.document_id);
            let renderer_trace = trace_current(
                LogComponent::Core,
                "txt_pipeline::office_renderer_boundary",
                || {
                    vec![
                        TraceField::new(
                            "source_path",
                            sanitize_trace_value(TraceValue::path(&record.path)),
                        ),
                        TraceField::new(
                            "output_directory",
                            sanitize_trace_value(TraceValue::path(&output_directory)),
                        ),
                        TraceField::new(
                            "reference_path",
                            sanitize_trace_value(TraceValue::path(&record.path)),
                        ),
                        TraceField::new(
                            "renderer_internals",
                            TraceValue::Omitted(TraceOmission::ExternalComponent),
                        ),
                    ]
                },
            );
            let rendered = office_renderer(&record.path, &output_directory, &record.path);
            if let Some(trace) = renderer_trace {
                let output = match &rendered {
                    Ok(OfficeRenderOutcome::Rendered(path)) => json!({
                        "outcome": "rendered",
                        "path": TraceValue::path(path).to_json(),
                    }),
                    Ok(OfficeRenderOutcome::Warning(warning)) => {
                        json!({"outcome": "warning", "warning": warning})
                    }
                    Err(error) => {
                        json!({"outcome": "failed", "code": error.code().as_str(), "message": trace_sensitive_text(error.message()).to_json()})
                    }
                };
                let _ = trace.finish(TraceValue::Data(redact_trace_json(output)));
            }
            let (href, text, warnings, page_spans) = match rendered? {
                OfficeRenderOutcome::Rendered(pdf_path) => {
                    let mut pdf_record = record.clone();
                    pdf_record.path = pdf_path;
                    pdf_record.kind = "pdf".to_owned();
                    let extracted = extract_pdf_text_with_options(&pdf_record, options);
                    (
                        Some(format!(
                            "../rendered-documents/{}/document.pdf",
                            record.document_id
                        )),
                        extracted.text,
                        extracted.warnings,
                        extracted.page_spans,
                    )
                }
                OfficeRenderOutcome::Warning(warning) => {
                    (None, String::new(), vec![warning], Vec::new())
                }
            };
            Ok(TxtDocument {
                record,
                relative_path,
                href,
                text,
                warnings,
                page_spans,
                tags,
            })
        },
        |result: &Result<TxtDocument, OperationError>| match result {
            Ok(document) =>
                json!({"record_path": TraceValue::path(&document.record.path).to_json()}),
            Err(_) => Value::Null,
        }
    )
}

fn read_document(
    person: &PersonConfig,
    record: DocumentRecord,
    configured_tags: &ConfiguredTags,
    _options: PdfExtractionOptions,
) -> Result<TxtDocument, OperationError> {
    let trace = trace_current(LogComponent::Core, "txt_pipeline::read_document", || {
        vec![
            TraceField::new("person", sanitize_trace_value(trace_person(person))),
            TraceField::new(
                "source_documents",
                sanitize_trace_value(TraceValue::path(&person.source_documents)),
            ),
            TraceField::new(
                "record",
                sanitize_trace_value(trace_document_record(&record)),
            ),
            TraceField::new(
                "document_path",
                sanitize_trace_value(TraceValue::path(&record.path)),
            ),
            TraceField::new(
                "configured_tags",
                sanitize_trace_value(trace_debug(configured_tags)),
            ),
            TraceField::new("options", sanitize_trace_value(trace_debug(&_options))),
        ]
    });
    let (source_bytes, result) = match fs::read(&record.path) {
        Ok(bytes) => {
            let result = (|| {
                let text = String::from_utf8_lossy(&bytes)
                    .replace("\r\n", "\n")
                    .replace('\r', "\n");
                let relative_path = relative_document_path(person, &record.path)?;
                let tags = tags_for_document(&record, &relative_path, configured_tags);
                let href = format!("../documents/{}", encode_url_path(&relative_path));

                Ok(TxtDocument {
                    record,
                    relative_path,
                    href: Some(href),
                    text,
                    warnings: Vec::new(),
                    page_spans: Vec::new(),
                    tags,
                })
            })();
            (Some(bytes), result)
        }
        Err(cause) => (
            None,
            Err(filesystem_error(
                "impossibile leggere il documento TXT",
                &record.path,
                cause,
            )),
        ),
    };
    if let Some(trace) = trace {
        let value = json!({
            "result": result.trace_value().to_json(),
            "input_source_bytes": source_bytes.as_ref().map_or_else(
                || json!({"kind": "omitted", "reason": "external_component"}),
                |bytes| match std::str::from_utf8(bytes) {
                    Ok(text) => json!({
                        "kind": "data",
                        "encoding": "utf8",
                        "value": trace_sensitive_text(text).to_json(),
                    }),
                    Err(_) => json!({
                        "kind": "data",
                        "encoding": "utf8_lossy",
                        "value": trace_sensitive_text(&String::from_utf8_lossy(bytes)).to_json(),
                        "raw_bytes": {
                            "kind": "omitted",
                            "reason": "invalid_utf8_byte_sequences_not_preserved",
                        },
                    }),
                },
            ),
            "record_path": result.as_ref().ok().map(|document| TraceValue::path(&document.record.path).to_json()),
        });
        let _ = trace.finish(TraceValue::Data(redact_trace_json(value)));
    }
    result
}

fn read_pdf_document(
    person: &PersonConfig,
    record: DocumentRecord,
    configured_tags: &ConfiguredTags,
    options: PdfExtractionOptions,
) -> Result<TxtDocument, OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::read_pdf_document",
        [
            TraceField::new("person", trace_person(person)),
            TraceField::new(
                "source_documents",
                TraceValue::path(&person.source_documents)
            ),
            TraceField::new("record", trace_document_record(&record)),
            TraceField::new("document_path", TraceValue::path(&record.path)),
            TraceField::new("configured_tags", trace_debug(configured_tags)),
            TraceField::new("options", trace_debug(&options)),
        ],
        || {
            let extracted = extract_pdf_text_with_options(&record, options);
            let relative_path = relative_document_path(person, &record.path)?;
            let href = format!("../documents/{}", encode_url_path(&relative_path));
            let tags = tags_for_document(&record, &relative_path, configured_tags);
            Ok(TxtDocument {
                record,
                relative_path,
                href: Some(href),
                text: extracted.text,
                warnings: extracted.warnings,
                page_spans: extracted.page_spans,
                tags,
            })
        },
        |result: &Result<TxtDocument, OperationError>| match result {
            Ok(document) =>
                json!({"record_path": TraceValue::path(&document.record.path).to_json()}),
            Err(_) => Value::Null,
        }
    )
}

fn read_link_only_document(
    person: &PersonConfig,
    record: DocumentRecord,
    configured_tags: &ConfiguredTags,
) -> Result<TxtDocument, OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::read_link_only_document",
        [
            TraceField::new("person", trace_person(person)),
            TraceField::new(
                "source_documents",
                TraceValue::path(&person.source_documents)
            ),
            TraceField::new("record", trace_document_record(&record)),
            TraceField::new("document_path", TraceValue::path(&record.path)),
            TraceField::new("configured_tags", trace_debug(configured_tags)),
        ],
        || {
            let relative_path = relative_document_path(person, &record.path)?;
            let href = format!("../documents/{}", encode_url_path(&relative_path));
            let tags = tags_for_document(&record, &relative_path, configured_tags);
            Ok(TxtDocument {
                record,
                relative_path,
                href: Some(href),
                text: String::new(),
                warnings: Vec::new(),
                page_spans: Vec::new(),
                tags,
            })
        },
        |result: &Result<TxtDocument, OperationError>| match result {
            Ok(document) =>
                json!({"record_path": TraceValue::path(&document.record.path).to_json()}),
            Err(_) => Value::Null,
        }
    )
}

fn tags_for_document(
    record: &DocumentRecord,
    relative_path: &str,
    configured_tags: &ConfiguredTags,
) -> Vec<String> {
    trace_pipeline_call!(
        "txt_pipeline::tags_for_document",
        [
            TraceField::new("record", trace_document_record(record)),
            TraceField::new("document_path", TraceValue::path(&record.path)),
            TraceField::new("relative_path", TraceValue::Data(json!(relative_path))),
            TraceField::new("configured_tags", trace_debug(configured_tags)),
        ],
        || configured_tags
            .iter()
            .find(|(key, _)| {
                let normalized = key.replace('\\', "/");
                relative_path.ends_with(&normalized)
                    || Path::new(relative_path)
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy() == normalized)
            })
            .map_or_else(|| record.tags.clone(), |(_, tags)| tags.clone()),
        |_| Value::Null
    )
}

fn build_database(
    path: &Path,
    documents: &[TxtDocument],
    patient_id: &str,
) -> Result<(), OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::build_database",
        [
            TraceField::new("database_path", TraceValue::path(path)),
            TraceField::new("documents", trace_debug(documents)),
            TraceField::new(
                "document_paths",
                TraceValue::Data(trace_document_paths(documents))
            ),
            TraceField::new("patient_id", TraceValue::Data(json!(patient_id))),
            TraceField::new(
                "sqlite_engine_internals",
                TraceValue::Omitted(TraceOmission::ExternalComponent),
            ),
        ],
        || {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|cause| {
                    filesystem_error("impossibile creare la directory SQLite", parent, cause)
                })?;
            }
            let mut stage = DatabaseStage {
                path: temporary_database_path(path, "building"),
                committed: false,
            };
            {
                let mut connection = Connection::open(&stage.path).map_err(|cause| {
                    database_error(
                        "impossibile creare il database SQLite temporaneo",
                        path,
                        cause,
                    )
                })?;
                connection
                    .pragma_update(None, "foreign_keys", "ON")
                    .map_err(|cause| {
                        database_error("impossibile attivare le chiavi esterne", path, cause)
                    })?;
                let transaction = connection.transaction().map_err(|cause| {
                    database_error("impossibile iniziare la transazione", path, cause)
                })?;
                transaction
                    .execute_batch(
                        "CREATE TABLE documents (
                     id TEXT PRIMARY KEY,
                     patient_id TEXT NOT NULL,
                     path TEXT NOT NULL,
                     title TEXT NOT NULL,
                     category TEXT NOT NULL,
                     kind TEXT NOT NULL,
                     sha256 TEXT NOT NULL,
                     document_date TEXT,
                     series TEXT,
                     origin TEXT NOT NULL,
                     container_id TEXT,
                     internal_path TEXT
                 );
                 CREATE TABLE document_text (
                     document_id TEXT PRIMARY KEY,
                     text TEXT NOT NULL,
                     FOREIGN KEY (document_id) REFERENCES documents(id)
                 );
                 CREATE TABLE document_page_spans (
                     document_id TEXT NOT NULL,
                     page_number INTEGER NOT NULL,
                     character_start INTEGER NOT NULL,
                     character_end INTEGER NOT NULL,
                     PRIMARY KEY (document_id, page_number),
                     FOREIGN KEY (document_id) REFERENCES documents(id)
                 );
                 CREATE VIRTUAL TABLE document_fts USING fts5(
                     title, category, tags, text
                 );",
                    )
                    .map_err(|cause| {
                        database_error("impossibile creare lo schema TXT", path, cause)
                    })?;

                for document in documents {
                    transaction
                        .execute(
                            "INSERT INTO documents (
                         id, patient_id, path, title, category, kind, sha256,
                         document_date, series, origin, container_id, internal_path
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                            params![
                                document.record.document_id,
                                patient_id,
                                document.relative_path,
                                document.record.title,
                                document.record.category,
                                document.record.kind,
                                document.record.sha256,
                                document.record.date,
                                document.record.series,
                                document.record.origin,
                                document.record.container_id,
                                document.record.internal_path,
                            ],
                        )
                        .map_err(|cause| {
                            database_error("impossibile inserire il documento TXT", path, cause)
                        })?;
                    let row_id = transaction.last_insert_rowid();
                    if !document.text.is_empty() {
                        transaction
                            .execute(
                                "INSERT INTO document_text(document_id, text) VALUES (?1, ?2)",
                                params![document.record.document_id, document.text],
                            )
                            .map_err(|cause| {
                                database_error("impossibile salvare il testo TXT", path, cause)
                            })?;
                    }
                    for span in &document.page_spans {
                        let page_number = i64::try_from(span.page_number).map_err(|cause| {
                            integer_range_error(
                                "numero pagina fuori intervallo SQLite",
                                path,
                                cause,
                            )
                        })?;
                        let character_start =
                            i64::try_from(span.character_start).map_err(|cause| {
                                integer_range_error(
                                    "offset pagina fuori intervallo SQLite",
                                    path,
                                    cause,
                                )
                            })?;
                        let character_end = i64::try_from(span.character_end).map_err(|cause| {
                            integer_range_error(
                                "offset pagina fuori intervallo SQLite",
                                path,
                                cause,
                            )
                        })?;
                        transaction
                            .execute(
                                "INSERT INTO document_page_spans(
                             document_id, page_number, character_start, character_end
                         ) VALUES (?1, ?2, ?3, ?4)",
                                params![
                                    document.record.document_id,
                                    page_number,
                                    character_start,
                                    character_end,
                                ],
                            )
                            .map_err(|cause| {
                                database_error(
                                    "impossibile salvare la provenienza delle pagine",
                                    path,
                                    cause,
                                )
                            })?;
                    }
                    transaction
                        .execute(
                            "INSERT INTO document_fts(rowid, title, category, tags, text)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                            params![
                                row_id,
                                document.record.title,
                                document.record.category,
                                document.tags.join(" "),
                                document.text,
                            ],
                        )
                        .map_err(|cause| {
                            database_error("impossibile indicizzare il testo TXT", path, cause)
                        })?;
                }
                transaction.commit().map_err(|cause| {
                    database_error("impossibile confermare il database SQLite", path, cause)
                })?;
            }
            replace_generated_database(&stage.path, path)?;
            stage.committed = true;
            Ok(())
        },
        |_| Value::Null
    )
}

fn temporary_database_path(target: &Path, purpose: &str) -> PathBuf {
    trace_pipeline_call!(
        "txt_pipeline::temporary_database_path",
        [
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new("purpose", TraceValue::Data(json!(purpose))),
        ],
        || {
            let sequence = NEXT_DATABASE.fetch_add(1, Ordering::Relaxed);
            let file_name = target
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("database.db");
            target.with_file_name(format!(
                ".{file_name}.{purpose}-{}-{sequence}",
                std::process::id()
            ))
        },
        |result: &PathBuf| TraceValue::path(result).to_json()
    )
}

fn replace_generated_database(source: &Path, target: &Path) -> Result<(), OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::replace_generated_database",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
        ],
        || {
            match fs::symlink_metadata(target) {
                Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {
                    fs::rename(source, target).map_err(|cause| {
                        filesystem_error(
                            "impossibile installare il database generato",
                            target,
                            cause,
                        )
                    })
                }
                Err(cause) => Err(filesystem_error(
                    "impossibile verificare il database precedente",
                    target,
                    cause,
                )),
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    let backup = temporary_database_path(target, "previous");
                    fs::rename(target, &backup).map_err(|cause| {
                        filesystem_error(
                            "impossibile conservare il database precedente",
                            target,
                            cause,
                        )
                    })?;
                    if let Err(cause) = fs::rename(source, target) {
                        let detail = match fs::rename(&backup, target) {
                            Ok(()) => "database precedente ripristinato".to_owned(),
                            Err(error) => {
                                format!("ripristino del database precedente fallito: {error}")
                            }
                        };
                        return Err(filesystem_error(
                            &format!("impossibile installare il database generato; {detail}"),
                            target,
                            cause,
                        ));
                    }
                    let _ = fs::remove_file(backup);
                    Ok(())
                }
                Ok(_) => Err(OperationError::new(
                    ErrorCode::Configuration,
                    format!(
                        "il percorso del database non è un file regolare: {}",
                        target.display()
                    ),
                )),
            }
        },
        |_| Value::Null
    )
}

fn export_documents(
    path: &Path,
    person: &PersonConfig,
    documents: &[TxtDocument],
) -> Result<(), OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::export_documents",
        [
            TraceField::new("output_path", TraceValue::path(path)),
            TraceField::new("person", trace_person(person)),
            TraceField::new(
                "source_documents",
                TraceValue::path(&person.source_documents)
            ),
            TraceField::new("documents", trace_debug(documents)),
            TraceField::new(
                "document_paths",
                TraceValue::Data(trace_document_paths(documents))
            ),
        ],
        || {
            let rows: Vec<Value> = documents
                .iter()
                .map(|document| {
                    json!({
                        "id": document.record.document_id,
                        "patient_id": person.id,
                        "title": document.record.title,
                        "date": document.record.date,
                        "category": document.record.category,
                        "kind": document.record.kind,
                        "path": document.relative_path,
                        "href": document.href,
                        "sha256": document.record.sha256,
                        "tags": document.tags,
                        "text": document.text,
                        "warnings": document.warnings,
                        "page_spans": document.page_spans.iter().map(|span| json!({
                            "page_number": span.page_number,
                            "character_start": span.character_start,
                            "character_end": span.character_end,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            write_file(
                path,
                serde_json::to_string_pretty(&json!({
                    "patient_id": person.id,
                    "display_name": person.display_name,
                    "documents": rows,
                }))
                .map_err(|cause| {
                    OperationError::new(
                        ErrorCode::Configuration,
                        format!("impossibile serializzare il catalogo documenti: {cause}"),
                    )
                })? + "\n",
            )
        },
        |_| Value::Null
    )
}

fn build_frontend(
    path: &Path,
    person: &PersonConfig,
    documents: &[TxtDocument],
    heading: &str,
) -> Result<(), OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::build_frontend",
        [
            TraceField::new("output_path", TraceValue::path(path)),
            TraceField::new("person", trace_person(person)),
            TraceField::new(
                "source_documents",
                TraceValue::path(&person.source_documents)
            ),
            TraceField::new("documents", trace_debug(documents)),
            TraceField::new(
                "document_paths",
                TraceValue::Data(trace_document_paths(documents))
            ),
            TraceField::new("heading", TraceValue::Data(json!(heading))),
            TraceField::new(
                "filesystem_copy_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent),
            ),
        ],
        || {
            let document_root = path
                .parent()
                .expect("la pagina frontend ha una directory")
                .parent()
                .expect("la directory frontend ha un genitore")
                .join("documents");
            fs::create_dir_all(&document_root).map_err(|cause| {
                filesystem_error(
                    "impossibile creare la directory dei documenti",
                    &document_root,
                    cause,
                )
            })?;
            let mut html = format!(
                "<!doctype html>\n<html lang=\"it\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>SaniKey - {}</title>\n</head>\n<body>\n<main>\n<h1>{} — {}</h1>\n<ul>\n",
                escape_html(&person.display_name),
                escape_html(heading),
                escape_html(&person.display_name)
            );
            for document in documents {
                let copy_path = path
                    .parent()
                    .expect("la pagina frontend ha una directory")
                    .parent()
                    .expect("la directory frontend ha un genitore")
                    .join("documents")
                    .join(&document.relative_path);
                if let Some(parent) = copy_path.parent() {
                    fs::create_dir_all(parent).map_err(|cause| {
                        filesystem_error(
                            "impossibile creare la directory dei documenti",
                            parent,
                            cause,
                        )
                    })?;
                }
                fs::copy(&document.record.path, &copy_path).map_err(|cause| {
                    filesystem_error(
                        "impossibile copiare il documento sorgente",
                        &copy_path,
                        cause,
                    )
                })?;
                let date = document
                    .record
                    .date
                    .as_deref()
                    .unwrap_or("Data non indicata");
                let title = match document.href.as_deref() {
                    Some(href) => format!(
                        "<a href=\"{}\">{}</a>",
                        escape_html(href),
                        escape_html(&document.record.title)
                    ),
                    None => escape_html(&document.record.title),
                };
                let original_link = if document.record.kind == "office" {
                    format!(
                        "<p><a href=\"../documents/{}\">Originale Office</a></p>",
                        encode_url_path(&document.relative_path)
                    )
                } else {
                    String::new()
                };
                let warnings = document
                    .warnings
                    .iter()
                    .map(|warning| {
                        format!("<p class=\"warning\">Avviso: {}</p>", escape_html(warning))
                    })
                    .collect::<String>();
                html.push_str(&format!(
            "<li><article><h2>{title}</h2><p>{} · {}</p><p>Categoria: {}</p>{original_link}{warnings}<pre>{}</pre></article></li>\n",
            escape_html(date),
            escape_html(&document.record.kind),
            escape_html(&document.record.category),
            escape_html(&document.text),
        ));
            }
            html.push_str("</ul>\n</main>\n</body>\n</html>\n");
            write_file(path, html)
        },
        |_| Value::Null
    )
}

fn relative_document_path(person: &PersonConfig, path: &Path) -> Result<String, OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::relative_document_path",
        [
            TraceField::new("person", trace_person(person)),
            TraceField::new(
                "source_documents",
                TraceValue::path(&person.source_documents)
            ),
            TraceField::new("path", TraceValue::path(path)),
        ],
        || {
            let relative = path
                .strip_prefix(&person.source_documents)
                .map_err(|cause| {
                    OperationError::new(
                        ErrorCode::Configuration,
                        format!(
                            "documento fuori dalla directory sorgente {}: {cause}",
                            path.display()
                        ),
                    )
                })?;
            let mut components = Vec::new();
            for component in relative.components() {
                match component {
                    Component::Normal(part) => components.push(part.to_string_lossy().into_owned()),
                    _ => {
                        return Err(OperationError::new(
                            ErrorCode::Configuration,
                            format!("percorso documento relativo non valido: {}", path.display()),
                        ));
                    }
                }
            }
            Ok(components.join("/"))
        },
        |_| Value::Null
    )
}

fn encode_url_path(path: &str) -> String {
    trace_pipeline_call!(
        "txt_pipeline::encode_url_path",
        [TraceField::new("path", TraceValue::Data(json!(path)))],
        || {
            let mut encoded = String::new();
            for byte in path.bytes() {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~')
                {
                    encoded.push(char::from(byte));
                } else {
                    encoded.push_str(&format!("%{byte:02X}"));
                }
            }
            encoded
        },
        |_| Value::Null
    )
}

fn escape_html(value: &str) -> String {
    trace_pipeline_call!(
        "txt_pipeline::escape_html",
        [TraceField::new("value", TraceValue::Data(json!(value)))],
        || value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;"),
        |_| Value::Null
    )
}

fn write_file(path: &Path, contents: String) -> Result<(), OperationError> {
    trace_pipeline_call!(
        "txt_pipeline::write_file",
        [
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new("contents", TraceValue::Data(json!(contents))),
        ],
        || {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|cause| {
                    filesystem_error("impossibile creare la directory di output", parent, cause)
                })?;
            }
            fs::write(path, contents)
                .map_err(|cause| filesystem_error("impossibile scrivere l'artefatto", path, cause))
        },
        |_| Value::Null
    )
}

fn filesystem_error(context: &str, path: &Path, cause: std::io::Error) -> OperationError {
    trace_pipeline_call!(
        "txt_pipeline::filesystem_error",
        [
            TraceField::new("context", TraceValue::Data(json!(context))),
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new("cause", trace_debug(&cause)),
        ],
        || OperationError::new(
            ErrorCode::Configuration,
            format!("{context} {}: {cause}", path.display()),
        ),
        |_| Value::Null
    )
}

fn database_error(context: &str, path: &Path, cause: rusqlite::Error) -> OperationError {
    trace_pipeline_call!(
        "txt_pipeline::database_error",
        [
            TraceField::new("context", TraceValue::Data(json!(context))),
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new("cause", trace_debug(&cause)),
        ],
        || OperationError::new(
            ErrorCode::Configuration,
            format!("{context} {}: {cause}", path.display()),
        ),
        |_| Value::Null
    )
}

fn integer_range_error(
    context: &str,
    path: &Path,
    cause: std::num::TryFromIntError,
) -> OperationError {
    trace_pipeline_call!(
        "txt_pipeline::integer_range_error",
        [
            TraceField::new("context", TraceValue::Data(json!(context))),
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new("cause", trace_debug(&cause)),
        ],
        || OperationError::new(
            ErrorCode::Configuration,
            format!("{context} {}: {cause}", path.display()),
        ),
        |_| Value::Null
    )
}

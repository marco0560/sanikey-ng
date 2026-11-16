//! Estrazione del testo digitale PDF e OCR per pagine scansionate.

use crate::logging::{
    Component, TraceField, TraceOmission, TraceValue, trace_current, trace_json_value,
    trace_sensitive_text,
};
use crate::metadata::DocumentRecord;
use pdfium_render::prelude::{PdfPageObjectCommon, PdfPageObjectsCommon, PdfRenderConfig, Pdfium};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const MIN_PDF_TEXT_CHARACTERS: usize = 40;
const SCANNED_PDF_PAGE_IMAGE_COVERAGE: f32 = 0.90;
const OCR_TARGET_WIDTH: i32 = 2550;

macro_rules! trace_pdf_call {
    ($function:literal, [$($field:expr),* $(,)?], $body:expr, $output:expr) => {{
        let trace = trace_current(Component::Core, $function, || vec![$($field),*]);
        let result = run_pdf_body(|| $body);
        if let Some(trace) = trace {
            let output = $output(&result);
            let _ = trace.finish(TraceValue::Data(json!({
                "output": output.to_json(),
            })));
        }
        result
    }};
}

fn run_pdf_body<T>(body: impl FnOnce() -> T) -> T {
    body()
}

fn trace_document(document: &DocumentRecord) -> TraceValue {
    trace_json_value(&json!({
        "document_id": document.document_id,
        "patient_id": document.patient_id,
        "path": TraceValue::path(&document.path).to_json(),
        "title": document.title,
        "category": document.category,
        "kind": document.kind,
        "sha256": document.sha256,
        "date": document.date,
        "series": document.series,
        "tags": document.tags,
        "origin": document.origin,
        "container_id": document.container_id,
        "internal_path": document.internal_path,
    }))
}

fn trace_spans(spans: &[TextPageSpan]) -> TraceValue {
    TraceValue::Data(Value::Array(
        spans
            .iter()
            .map(|span| {
                json!({
                    "page_number": span.page_number,
                    "character_start": span.character_start,
                    "character_end": span.character_end,
                })
            })
            .collect(),
    ))
}

fn trace_extracted(extracted: &ExtractedText) -> TraceValue {
    trace_json_value(&json!({
        "document_id": extracted.document_id,
        "text": extracted.text,
        "warnings": extracted.warnings,
        "page_spans": trace_spans(&extracted.page_spans).to_json(),
    }))
}

/// Intervallo di testo estratto da una pagina PDF, con offset Unicode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextPageSpan {
    /// Numero pagina a partire da 1.
    pub page_number: usize,
    /// Offset iniziale nel testo concatenato, in valori Unicode.
    pub character_start: usize,
    /// Offset finale esclusivo nel testo concatenato, in valori Unicode.
    pub character_end: usize,
}

/// Testo estratto e provenienza per un documento PDF.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedText {
    /// Identificatore stabile del documento originale.
    pub document_id: String,
    /// Testo delle pagine separate da newline.
    pub text: String,
    /// Avvisi su estrazione incompleta o non disponibile.
    pub warnings: Vec<String>,
    /// Intervalli del testo associati alle pagine originali.
    pub page_spans: Vec<TextPageSpan>,
}

/// Controlli opzionali applicati durante l'estrazione PDF.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PdfExtractionOptions {
    /// Ripete l'OCR sui PDF scansionati anche quando il testo digitale è sufficiente.
    pub force_pdf_ocr: bool,
}

/// Estrae il testo PDF e associa ogni intervallo alla pagina originale.
///
/// Se il testo digitale non raggiunge 40 caratteri non whitespace, prova OCR
/// sulle pagine PDF. La forzatura OCR è limitata ai documenti in cui ogni
/// pagina è coperta per almeno il 90% da una singola immagine. Non esegue OCR
/// diretto sui file immagine sorgente né modifica il PDF.
pub fn extract_pdf_text(document: &DocumentRecord) -> ExtractedText {
    trace_pdf_call!(
        "pdf::extract_pdf_text",
        [TraceField::new("document", trace_document(document))],
        extract_pdf_text_with_options(document, PdfExtractionOptions::default()),
        |result: &ExtractedText| trace_extracted(result)
    )
}

/// Estrae il testo PDF applicando controlli espliciti di forzatura OCR.
pub fn extract_pdf_text_with_options(
    document: &DocumentRecord,
    options: PdfExtractionOptions,
) -> ExtractedText {
    trace_pdf_call!(
        "pdf::extract_pdf_text_with_options",
        [
            TraceField::new("document", trace_document(document)),
            TraceField::new("options", TraceValue::data(format!("{options:#?}"))),
        ],
        {
            let mut digital = extract_digital_pdf_text(document);
            let sufficient_text = has_sufficient_pdf_text(&digital.text);
            if sufficient_text && !options.force_pdf_ocr {
                return digital;
            }

            let ocr = match extract_pdf_ocr_text(
                &document.document_id,
                &document.path,
                sufficient_text && options.force_pdf_ocr,
            ) {
                Ok(ocr) => ocr,
                Err(error) => {
                    digital.warnings.retain(|warning| {
                warning != "il testo PDF digitale è insufficiente e l'OCR non è ancora disponibile"
            });
                    if !sufficient_text {
                        digital.warnings.push(format!(
                            "testo PDF insufficiente; OCR non riuscito: {error}"
                        ));
                    } else if options.force_pdf_ocr {
                        digital
                            .warnings
                            .push(format!("OCR PDF forzato non riuscito: {error}"));
                    }
                    return digital;
                }
            };

            if !ocr.attempted {
                return digital;
            }

            let mut result = ocr.extracted;
            result.warnings = digital
                .warnings
                .into_iter()
                .filter(|warning| {
                    warning
                        != "il testo PDF digitale è insufficiente e l'OCR non è ancora disponibile"
                })
                .collect();
            result.warnings.extend(ocr.warnings);
            result
        },
        |result: &ExtractedText| trace_extracted(result)
    )
}

fn extract_digital_pdf_text(document: &DocumentRecord) -> ExtractedText {
    trace_pdf_call!(
        "pdf::extract_digital_pdf_text",
        [
            TraceField::new("document", trace_document(document)),
            TraceField::new(
                "pdf_file_bytes",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        {
            let mut result = ExtractedText {
                document_id: document.document_id.clone(),
                text: String::new(),
                warnings: Vec::new(),
                page_spans: Vec::new(),
            };
            let pdf = match lopdf::Document::load(&document.path) {
                Ok(pdf) => pdf,
                Err(error) => {
                    result
                        .warnings
                        .push(format!("impossibile leggere il PDF digitale: {error}"));
                    return result;
                }
            };
            let pages = pdf.get_pages();
            if pages.is_empty() {
                result
                    .warnings
                    .push("il PDF non contiene pagine".to_owned());
                return result;
            }
            for (index, page_number) in pages.keys().enumerate() {
                let page_text = match pdf.extract_text(&[*page_number]) {
                    Ok(text) => text,
                    Err(error) => {
                        result.warnings.push(format!(
                            "impossibile estrarre il testo PDF della pagina {page_number}: {error}"
                        ));
                        return result;
                    }
                };
                if index > 0 {
                    result.text.push('\n');
                }
                let start = result.text.chars().count();
                result.text.push_str(&page_text);
                let end = result.text.chars().count();
                result.page_spans.push(TextPageSpan {
                    page_number: *page_number as usize,
                    character_start: start,
                    character_end: end,
                });
            }
            if !has_sufficient_pdf_text(&result.text) {
                result.warnings.push(
                    "il testo PDF digitale è insufficiente e l'OCR non è ancora disponibile"
                        .to_owned(),
                );
            }
            result
        },
        |result: &ExtractedText| trace_extracted(result)
    )
}

fn has_sufficient_pdf_text(text: &str) -> bool {
    trace_pdf_call!(
        "pdf::has_sufficient_pdf_text",
        [TraceField::new("text", trace_sensitive_text(text))],
        text.chars()
            .filter(|character| !character.is_whitespace())
            .count()
            >= MIN_PDF_TEXT_CHARACTERS,
        |result: &bool| TraceValue::data(*result)
    )
}

#[derive(Debug)]
struct OcrExtraction {
    extracted: ExtractedText,
    warnings: Vec<String>,
    attempted: bool,
}

fn trace_ocr_result(result: &Result<OcrExtraction, String>) -> TraceValue {
    TraceValue::Data(json!(match result {
        Ok(ocr) => json!({
            "extracted": trace_extracted(&ocr.extracted).to_json(),
            "warnings": ocr.warnings,
            "attempted": ocr.attempted,
        }),
        Err(error) => json!({"error": trace_sensitive_text(error).to_json()}),
    }))
}

fn extract_pdf_ocr_text(
    document_id: &str,
    path: &Path,
    require_scanned_pdf: bool,
) -> Result<OcrExtraction, String> {
    trace_pdf_call!(
        "pdf::extract_pdf_ocr_text",
        [
            TraceField::new("document_id", TraceValue::data(document_id)),
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new("require_scanned_pdf", TraceValue::data(require_scanned_pdf)),
            TraceField::new(
                "pdf_file_bytes",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
            TraceField::new(
                "pdfium_document_handle",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        {
            let pdfium = bind_pdfium()?;
            let document = pdfium
                .load_pdf_from_file(path, None)
                .map_err(|error| format!("PDFium non può aprire il PDF: {error}"))?;
            if document.pages().is_empty() {
                return Err("il PDF non contiene pagine".to_owned());
            }

            for (index, page) in document.pages().iter().enumerate() {
                let page_area = page.width().value * page.height().value;
                if page_area <= 0.0 {
                    return Err(format!("area non valida nella pagina {}", index + 1));
                }
                let largest_image_area = page
                    .objects()
                    .iter()
                    .filter_map(|object| {
                        object.as_image_object().and_then(|image| {
                            let bounds = image.bounds().ok()?;
                            let left = bounds.left().value.max(0.0);
                            let right = bounds.right().value.min(page.width().value);
                            let bottom = bounds.bottom().value.max(0.0);
                            let top = bounds.top().value.min(page.height().value);
                            let width = (right - left).max(0.0);
                            let height = (top - bottom).max(0.0);
                            (width > 0.0 && height > 0.0).then_some(width * height)
                        })
                    })
                    .fold(0.0_f32, f32::max);
                if require_scanned_pdf
                    && largest_image_area / page_area < SCANNED_PDF_PAGE_IMAGE_COVERAGE
                {
                    return Ok(OcrExtraction {
                        extracted: empty_extraction(document_id),
                        warnings: Vec::new(),
                        attempted: false,
                    });
                }
            }

            let tesseract = find_tesseract()?;
            let glossary = GlossaryFile::create()?;
            let mut extracted = empty_extraction(document_id);
            let mut warnings = Vec::new();
            for (index, page) in document.pages().iter().enumerate() {
                let bitmap = page
                    .render_with_config(
                        &PdfRenderConfig::new()
                            .set_target_width(OCR_TARGET_WIDTH)
                            .set_maximum_height(3500),
                    )
                    .map_err(|error| {
                        format!("render pagina {} non riuscito: {error}", index + 1)
                    })?;
                let ppm = rgba_to_ppm(&bitmap.as_rgba_bytes(), bitmap.width(), bitmap.height())?;
                let output = run_tesseract(&tesseract, &glossary.path, &ppm, index + 1)?;
                let page_text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if index > 0 {
                    extracted.text.push('\n');
                }
                let start = extracted.text.chars().count();
                extracted.text.push_str(&page_text);
                let end = extracted.text.chars().count();
                extracted.page_spans.push(TextPageSpan {
                    page_number: index + 1,
                    character_start: start,
                    character_end: end,
                });
            }
            if !has_sufficient_pdf_text(&extracted.text) {
                warnings.push("l'OCR non ha restituito testo sufficiente".to_owned());
            }
            Ok(OcrExtraction {
                extracted,
                warnings,
                attempted: true,
            })
        },
        trace_ocr_result
    )
}

fn empty_extraction(document_id: &str) -> ExtractedText {
    trace_pdf_call!(
        "pdf::empty_extraction",
        [TraceField::new(
            "document_id",
            TraceValue::data(document_id)
        )],
        ExtractedText {
            document_id: document_id.to_owned(),
            text: String::new(),
            warnings: Vec::new(),
            page_spans: Vec::new(),
        },
        |result: &ExtractedText| trace_extracted(result)
    )
}

fn bind_pdfium() -> Result<Pdfium, String> {
    let explicit_library = std::env::var_os("SANIKEY_PDFIUM_LIBRARY");
    let mut candidates_for_trace = Vec::new();
    let trace = trace_current(Component::Core, "pdf::bind_pdfium", || {
        vec![TraceField::new(
            "requested_library_path",
            explicit_library
                .as_deref()
                .map(|path| TraceValue::path(Path::new(&path)))
                .unwrap_or_else(|| TraceValue::data(Value::Null)),
        )]
    });
    let library_name = Pdfium::pdfium_platform_library_name_at_path("");
    let mut candidates = Vec::new();
    if let Some(explicit) = explicit_library {
        let path = PathBuf::from(explicit);
        candidates_for_trace.push(path.clone());
        candidates.push(path);
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        let path = directory.join(&library_name);
        candidates_for_trace.push(path.clone());
        candidates.push(path);
    }
    let report = bind_pdfium_candidates(
        &candidates,
        |candidate| Pdfium::bind_to_library(candidate).map_err(pdfium_binding_error),
        || Pdfium::bind_to_system_library().map_err(pdfium_binding_error),
    );
    let result = match report.result {
        PdfiumBindingResult::Bound(bindings) => Ok(Pdfium::new(bindings)),
        PdfiumBindingResult::AlreadyInitialized => Ok(Pdfium::default()),
        PdfiumBindingResult::Failed(error) => Err(format!(
            "libreria PDFium non disponibile; collocarla accanto all'eseguibile o impostare SANIKEY_PDFIUM_LIBRARY: {error}"
        )),
    };
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!({
            "return": match &result { Ok(_) => json!("bound"), Err(error) => trace_sensitive_text(error).to_json() },
            "output": {
                "candidate_paths": candidates_for_trace.iter().map(|path| TraceValue::path(path).to_json()).collect::<Vec<_>>(),
                "candidate_attempts": report.candidates.iter().map(PdfiumCandidateAttempt::to_json).collect::<Vec<_>>(),
                "binding_handle": TraceValue::Omitted(TraceOmission::ExternalComponent).to_json(),
                "system_binding_attempted": !matches!(&report.system, PdfiumSystemAttempt::NotAttempted),
                "system_library_resolution": report.system.to_json(),
            },
        })));
    }
    result
}

enum PdfiumBindingError {
    AlreadyInitialized,
    Failed(String),
}

enum PdfiumBindingResult<T> {
    Bound(T),
    AlreadyInitialized,
    Failed(String),
}

struct PdfiumCandidateAttempt {
    path: PathBuf,
    outcome: &'static str,
    error: Option<String>,
}

/// Rappresenta un tentativo di binding a una libreria PDFium candidata.
impl PdfiumCandidateAttempt {
    fn to_json(&self) -> Value {
        json!({
            "path": TraceValue::path(&self.path).to_json(),
            "outcome": self.outcome,
            "error": self.error.as_deref().map(|error| trace_sensitive_text(error).to_json()),
        })
    }
}

enum PdfiumSystemAttempt {
    NotAttempted,
    Bound,
    AlreadyInitialized,
    Failed(String),
}

/// Rappresenta l'esito del fallback alla libreria PDFium di sistema.
impl PdfiumSystemAttempt {
    fn to_json(&self) -> Value {
        match self {
            Self::NotAttempted => json!({"outcome": "not_attempted"}),
            Self::Bound => json!({"outcome": "bound"}),
            Self::AlreadyInitialized => json!({"outcome": "already_initialized"}),
            Self::Failed(error) => json!({
                "outcome": "failed",
                "error": trace_sensitive_text(error).to_json(),
            }),
        }
    }
}

struct PdfiumBindingReport<T> {
    result: PdfiumBindingResult<T>,
    candidates: Vec<PdfiumCandidateAttempt>,
    system: PdfiumSystemAttempt,
}

fn bind_pdfium_candidates<T>(
    candidates: &[PathBuf],
    mut bind_candidate: impl FnMut(&Path) -> Result<T, PdfiumBindingError>,
    bind_system: impl FnOnce() -> Result<T, PdfiumBindingError>,
) -> PdfiumBindingReport<T> {
    let mut attempts = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        match bind_candidate(candidate) {
            Ok(bindings) => {
                attempts.push(PdfiumCandidateAttempt {
                    path: candidate.clone(),
                    outcome: "bound",
                    error: None,
                });
                return PdfiumBindingReport {
                    result: PdfiumBindingResult::Bound(bindings),
                    candidates: attempts,
                    system: PdfiumSystemAttempt::NotAttempted,
                };
            }
            Err(PdfiumBindingError::AlreadyInitialized) => {
                attempts.push(PdfiumCandidateAttempt {
                    path: candidate.clone(),
                    outcome: "already_initialized",
                    error: None,
                });
                return PdfiumBindingReport {
                    result: PdfiumBindingResult::AlreadyInitialized,
                    candidates: attempts,
                    system: PdfiumSystemAttempt::NotAttempted,
                };
            }
            Err(PdfiumBindingError::Failed(error)) => {
                attempts.push(PdfiumCandidateAttempt {
                    path: candidate.clone(),
                    outcome: "failed",
                    error: Some(error),
                });
            }
        }
    }

    match bind_system() {
        Ok(bindings) => PdfiumBindingReport {
            result: PdfiumBindingResult::Bound(bindings),
            candidates: attempts,
            system: PdfiumSystemAttempt::Bound,
        },
        Err(PdfiumBindingError::AlreadyInitialized) => PdfiumBindingReport {
            result: PdfiumBindingResult::AlreadyInitialized,
            candidates: attempts,
            system: PdfiumSystemAttempt::AlreadyInitialized,
        },
        Err(PdfiumBindingError::Failed(error)) => PdfiumBindingReport {
            result: PdfiumBindingResult::Failed(error.clone()),
            candidates: attempts,
            system: PdfiumSystemAttempt::Failed(error),
        },
    }
}

fn pdfium_binding_error(error: pdfium_render::prelude::PdfiumError) -> PdfiumBindingError {
    match error {
        pdfium_render::prelude::PdfiumError::PdfiumLibraryBindingsAlreadyInitialized => {
            PdfiumBindingError::AlreadyInitialized
        }
        error => PdfiumBindingError::Failed(error.to_string()),
    }
}

fn find_tesseract() -> Result<PathBuf, String> {
    let explicit_tesseract = std::env::var_os("SANIKEY_TESSERACT");
    let mut search_directories = Vec::new();
    trace_pdf_call!(
        "pdf::find_tesseract",
        [TraceField::new(
            "explicit_executable",
            explicit_tesseract
                .as_deref()
                .map(|path| TraceValue::path(Path::new(&path)))
                .unwrap_or_else(|| TraceValue::data(Value::Null)),
        )],
        {
            if let Some(explicit) = explicit_tesseract {
                let path = PathBuf::from(explicit);
                if path.is_file() {
                    return Ok(path);
                }
                return Err("SANIKEY_TESSERACT non indica un eseguibile disponibile".to_owned());
            }
            let search_path = std::env::var_os("PATH").unwrap_or_default();
            for directory in std::env::split_paths(&search_path) {
                search_directories.push(directory.to_path_buf());
                let executable = directory.join(if cfg!(windows) {
                    "tesseract.exe"
                } else {
                    "tesseract"
                });
                if executable.is_file() {
                    return Ok(executable);
                }
            }
            Err("eseguibile Tesseract non disponibile nel PATH".to_owned())
        },
        |result: &Result<PathBuf, String>| TraceValue::Data(json!({
            "selected_executable": result.as_ref().ok().map(|path| TraceValue::path(path).to_json()),
            "search_directories": search_directories.iter().map(|path| TraceValue::path(path).to_json()).collect::<Vec<_>>(),
            "result": result.as_ref().map(|_| json!("found")).unwrap_or_else(|error| trace_sensitive_text(error).to_json()),
        }))
    )
}

fn rgba_to_ppm(rgba: &[u8], width: i32, height: i32) -> Result<Vec<u8>, String> {
    trace_pdf_call!(
        "pdf::rgba_to_ppm",
        [
            TraceField::new("rgba", TraceValue::Data(json!(rgba))),
            TraceField::new("width", TraceValue::data(width)),
            TraceField::new("height", TraceValue::data(height)),
        ],
        {
            if width <= 0 || height <= 0 {
                return Err("bitmap OCR con dimensioni non valide".to_owned());
            }
            let expected = usize::try_from(width)
                .ok()
                .and_then(|width| {
                    usize::try_from(height)
                        .ok()
                        .and_then(|height| width.checked_mul(height))
                })
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| "dimensioni bitmap OCR eccessive".to_owned())?;
            if rgba.len() != expected {
                return Err("dimensione bitmap OCR incoerente".to_owned());
            }
            let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
            ppm.reserve(expected / 4 * 3);
            let (pixels, remainder) = rgba.as_chunks::<4>();
            debug_assert!(remainder.is_empty());
            for pixel in pixels {
                ppm.extend_from_slice(&pixel[..3]);
            }
            Ok(ppm)
        },
        |result: &Result<Vec<u8>, String>| TraceValue::Data(json!({
            "ppm_bytes": result.as_ref().ok(),
            "result": result.as_ref().map(|_| json!("ok")).unwrap_or_else(|error| trace_sensitive_text(error).to_json()),
        }))
    )
}

fn run_tesseract(
    executable: &Path,
    glossary: &Path,
    ppm: &[u8],
    page_number: usize,
) -> Result<Output, String> {
    let result = trace_pdf_call!(
        "pdf::run_tesseract",
        [
            TraceField::new("executable", TraceValue::path(executable)),
            TraceField::new("glossary", TraceValue::path(glossary)),
            TraceField::new("page_number", TraceValue::data(page_number)),
            TraceField::new(
                "argv",
                TraceValue::Data(json!(["stdin", "stdout", "--dpi", "300", "--user-words"]))
            ),
            TraceField::new("stdin_ppm_bytes", trace_pdf_bytes(ppm)),
            TraceField::new("stderr_capture", TraceValue::data(true)),
        ],
        {
            let mut child = Command::new(executable)
                .args(["stdin", "stdout", "--dpi", "300", "--user-words"])
                .arg(glossary)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| {
                    TesseractFailure::BeforeOutput(format!(
                        "avvio di Tesseract non riuscito: {error}"
                    ))
                })?;
            let stdout = child.stdout.take().ok_or_else(|| {
                TesseractFailure::BeforeOutput("output di Tesseract non disponibile".to_owned())
            })?;
            let stderr = child.stderr.take().ok_or_else(|| {
                TesseractFailure::BeforeOutput("stderr di Tesseract non disponibile".to_owned())
            })?;
            let stdout_reader = std::thread::spawn(move || read_tesseract_pipe(stdout));
            let stderr_reader = std::thread::spawn(move || read_tesseract_pipe(stderr));
            let mut stdin = child.stdin.take().ok_or_else(|| {
                TesseractFailure::BeforeOutput("input di Tesseract non disponibile".to_owned())
            })?;
            let input_error = stdin.write_all(ppm).err();
            drop(stdin);
            let status = child.wait().map_err(|error| {
                TesseractFailure::BeforeOutput(format!("attesa di Tesseract non riuscita: {error}"))
            })?;
            let stdout = join_tesseract_reader(stdout_reader, "stdout")?;
            let stderr = join_tesseract_reader(stderr_reader, "stderr")?;
            if let Some(error) = input_error {
                return Err(TesseractFailure::BeforeOutput(format!(
                    "invio pagina {page_number} a Tesseract non riuscito: {error}"
                )));
            }
            let output = Output {
                status,
                stdout,
                stderr,
            };
            if !output.status.success() {
                return Err(TesseractFailure::Process {
                    message: format!(
                        "Tesseract fallito sulla pagina {page_number} con stato {}",
                        output.status
                    ),
                    output,
                });
            }
            Ok(output)
        },
        |result: &Result<Output, TesseractFailure>| TraceValue::Data(match result {
            Ok(output) => json!({
                "status": format!("{}", output.status),
                "stdout": trace_pdf_bytes(&output.stdout).to_json(),
                "stderr": trace_pdf_bytes(&output.stderr).to_json(),
            }),
            Err(TesseractFailure::BeforeOutput(error)) => {
                json!({"error": trace_sensitive_text(error).to_json()})
            }
            Err(TesseractFailure::Process { message, output }) => json!({
                "error": trace_sensitive_text(message).to_json(),
                "status": format!("{}", output.status),
                "stdout": trace_pdf_bytes(&output.stdout).to_json(),
                "stderr": trace_pdf_bytes(&output.stderr).to_json(),
            }),
        })
    );
    result.map_err(TesseractFailure::into_message)
}

enum TesseractFailure {
    BeforeOutput(String),
    Process { message: String, output: Output },
}

/// Converte i dettagli interni dell'errore OCR nel messaggio restituito al chiamante.
impl TesseractFailure {
    fn into_message(self) -> String {
        match self {
            Self::BeforeOutput(message) | Self::Process { message, .. } => message,
        }
    }
}

fn read_tesseract_pipe(mut pipe: impl Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn join_tesseract_reader(
    reader: std::thread::JoinHandle<std::io::Result<Vec<u8>>>,
    stream: &str,
) -> Result<Vec<u8>, TesseractFailure> {
    reader
        .join()
        .map_err(|_| {
            TesseractFailure::BeforeOutput(format!("lettura di {stream} di Tesseract interrotta"))
        })?
        .map_err(|error| {
            TesseractFailure::BeforeOutput(format!(
                "lettura di {stream} di Tesseract non riuscita: {error}"
            ))
        })
}

fn trace_pdf_bytes(bytes: &[u8]) -> TraceValue {
    match std::str::from_utf8(bytes) {
        Ok(text) => trace_sensitive_text(text),
        Err(_) => TraceValue::Omitted(TraceOmission::UnsupportedType),
    }
}

struct GlossaryFile {
    path: PathBuf,
}

/// Materializza e gestisce il glossario OCR temporaneo.
impl GlossaryFile {
    fn create() -> Result<Self, String> {
        static NEXT_FILE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let bytes = include_bytes!("../assets/ocr/tesseract-medical-it.user-words");
        let trace = trace_current(Component::Core, "pdf::GlossaryFile::create", || {
            vec![TraceField::new(
                "glossary_bytes",
                TraceValue::Data(json!(bytes.as_slice())),
            )]
        });
        let result = (|| {
            for _ in 0..32 {
                let sequence = NEXT_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "sanikey-ocr-glossary-{}-{sequence}.txt",
                    std::process::id()
                ));
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                match options.open(&path) {
                    Ok(mut file) => {
                        if let Err(error) = file.write_all(bytes) {
                            let _ = std::fs::remove_file(&path);
                            return Err(format!("scrittura glossario OCR non riuscita: {error}"));
                        }
                        return Ok(Self { path });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => {
                        return Err(format!("creazione glossario OCR non riuscita: {error}"));
                    }
                }
            }
            Err("impossibile creare un file temporaneo per il glossario OCR".to_owned())
        })();
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!({
                "return": result.as_ref().map(|_| json!("created")).unwrap_or_else(|error| trace_sensitive_text(error).to_json()),
                "output": {
                    "path": result.as_ref().ok().map(|file| TraceValue::path(&file.path).to_json()),
                    "byte_count": bytes.len(),
                    "glossary_bytes_written": bytes.as_slice(),
                },
            })));
        }
        result
    }
}

/// Elimina il glossario temporaneo al termine dell'OCR.
impl Drop for GlossaryFile {
    fn drop(&mut self) {
        let trace = trace_current(Component::Core, "pdf::GlossaryFile::drop", || {
            vec![TraceField::new("path", TraceValue::path(&self.path))]
        });
        let result = std::fs::remove_file(&self.path);
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!({
                "return": result.as_ref().map(|_| json!("removed")).unwrap_or_else(|error| trace_sensitive_text(&error.to_string()).to_json()),
                "output": {"removed": result.is_ok()},
            })));
        }
    }
}

/// Restituisce il numero di pagine quando il PDF è leggibile.
pub fn pdf_page_count(document: &DocumentRecord) -> Option<usize> {
    trace_pdf_call!(
        "pdf::pdf_page_count",
        [
            TraceField::new("document", trace_document(document)),
            TraceField::new(
                "pdf_file_bytes",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        lopdf::Document::load(&document.path)
            .ok()
            .map(|pdf| pdf.get_pages().len()),
        |result: &Option<usize>| TraceValue::Data(json!(result))
    )
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod test_support {
    use lopdf::{
        Document, Object, Stream,
        content::{Content, Operation},
        dictionary,
    };

    pub(crate) fn synthetic_pdf(page_texts: &[&str]) -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let mut page_ids = Vec::new();
        for text in page_texts {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                    Operation::new("Td", vec![50.into(), 700.into()]),
                    Operation::new(
                        "Tj",
                        vec![Object::string_literal(
                            text.chars()
                                .map(|character| {
                                    u8::try_from(u32::from(character))
                                        .expect("fixture PDF sintetica in Latin-1")
                                })
                                .collect::<Vec<_>>(),
                        )],
                    ),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id = document.add_object(Stream::new(
                dictionary! {},
                content.encode().expect("contenuto PDF sintetico"),
            ));
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            });
            page_ids.push(Object::Reference(page_id));
        }
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => page_ids,
                "Count" => page_texts.len() as i64,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document
            .save_modern(&mut bytes)
            .expect("PDF sintetico valido");
        bytes
    }

    pub(crate) fn synthetic_scanned_pdf(text: &str) -> Vec<u8> {
        synthetic_scanned_pdf_with_digital_text(text, None)
    }

    pub(crate) fn synthetic_scanned_pdf_with_digital_text(
        text: &str,
        digital_text: Option<&str>,
    ) -> Vec<u8> {
        use lopdf::{
            Object, Stream,
            content::{Content, Operation},
        };

        const WIDTH: usize = 600;
        const HEIGHT: usize = 800;
        let mut pixels = vec![255_u8; WIDTH * HEIGHT * 3];
        let mut x = 70_usize;
        let mut top = 300_usize;
        for character in text.chars() {
            if character == ' ' {
                if x + 20 > WIDTH - 70 {
                    x = 70;
                    top += 100;
                    continue;
                }
                x += 20;
                continue;
            }
            let glyph = glyph(character).expect("glyph della fixture sintetica");
            for (row, pattern) in glyph.iter().enumerate() {
                for (column, pixel) in pattern.bytes().enumerate() {
                    if pixel != b'1' {
                        continue;
                    }
                    for dy in 0..4 {
                        for dx in 0..4 {
                            let offset = ((top + row * 4 + dy) * WIDTH + x + column * 4 + dx) * 3;
                            pixels[offset..offset + 3].fill(0);
                        }
                    }
                }
            }
            x += 28;
        }

        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let image_id = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => WIDTH as i64,
                "Height" => HEIGHT as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
            },
            pixels,
        ));
        let content = Content {
            operations: {
                let mut operations = vec![
                    Operation::new("q", vec![]),
                    Operation::new(
                        "cm",
                        vec![
                            612.into(),
                            0.into(),
                            0.into(),
                            792.into(),
                            0.into(),
                            0.into(),
                        ],
                    ),
                    Operation::new("Do", vec![Object::Name(b"Scan".to_vec())]),
                    Operation::new("Q", vec![]),
                ];
                if let Some(digital_text) = digital_text {
                    operations.extend([
                        Operation::new("BT", vec![]),
                        Operation::new("Tf", vec!["F1".into(), 12.into()]),
                        Operation::new("Td", vec![20.into(), 20.into()]),
                        Operation::new(
                            "Tj",
                            vec![Object::string_literal(
                                digital_text.bytes().collect::<Vec<_>>(),
                            )],
                        ),
                        Operation::new("ET", vec![]),
                    ]);
                }
                operations
            },
        };
        let content_id = document.add_object(Stream::new(
            dictionary! {},
            content.encode().expect("contenuto PDF scansionato"),
        ));
        let mut resources = dictionary! {
            "XObject" => dictionary! { "Scan" => image_id },
        };
        if digital_text.is_some() {
            let font_id = document.add_object(dictionary! {
                "Type" => "Font",
                "Subtype" => "Type1",
                "BaseFont" => "Helvetica",
                "Encoding" => "WinAnsiEncoding",
            });
            resources.set("Font", dictionary! { "F1" => font_id });
        }
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document
            .save_modern(&mut bytes)
            .expect("PDF scansionato sintetico valido");
        bytes
    }

    fn glyph(character: char) -> Option<[&'static str; 7]> {
        Some(match character {
            'A' => [
                "01110", "10001", "10001", "11111", "10001", "10001", "10001",
            ],
            'C' => [
                "01111", "10000", "10000", "10000", "10000", "10000", "01111",
            ],
            'E' => [
                "11111", "10000", "10000", "11110", "10000", "10000", "11111",
            ],
            'H' => [
                "10001", "10001", "10001", "11111", "10001", "10001", "10001",
            ],
            'I' => [
                "11111", "00100", "00100", "00100", "00100", "00100", "11111",
            ],
            'N' => [
                "10001", "11001", "11001", "10101", "10011", "10011", "10001",
            ],
            'O' => [
                "01110", "10001", "10001", "10001", "10001", "10001", "01110",
            ],
            'R' => [
                "11110", "10001", "10001", "11110", "10100", "10010", "10001",
            ],
            'S' => [
                "01111", "10000", "10000", "01110", "00001", "00001", "11110",
            ],
            'T' => [
                "11111", "00100", "00100", "00100", "00100", "00100", "00100",
            ],
            'Y' => [
                "10001", "10001", "01010", "00100", "00100", "00100", "00100",
            ],
            _ => return None,
        })
    }
}

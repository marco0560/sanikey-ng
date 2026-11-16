//! Fixture sintetiche per estrazione PDF digitale e OCR.

use super::{
    ExtractedText, PdfExtractionOptions, PdfiumBindingError, PdfiumBindingResult,
    PdfiumSystemAttempt, TextPageSpan, bind_pdfium, bind_pdfium_candidates, extract_pdf_text,
    extract_pdf_text_with_options, find_tesseract, pdf_page_count, run_tesseract,
    test_support::{synthetic_pdf, synthetic_scanned_pdf, synthetic_scanned_pdf_with_digital_text},
};
use crate::logging::{Component, Level, Session};
use crate::metadata::DocumentRecord;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_os = "linux")]
use std::time::Duration;

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

/// Gestisce i PDF sintetici e i loro percorsi temporanei.
impl TestDirectory {
    fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("sanikey-ng-pdf-{}-{sequence}", std::process::id()));
        fs::create_dir_all(&path).expect("directory PDF sintetica");
        Self(path)
    }

    fn write_pdf(&self, name: &str, bytes: &[u8]) -> DocumentRecord {
        let path = self.0.join(name);
        fs::write(&path, bytes).expect("PDF sintetico");
        document(&path)
    }
}

/// Rimuove i PDF sintetici quando i test terminano.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn document(path: &Path) -> DocumentRecord {
    DocumentRecord {
        document_id: "synthetic-pdf-id".to_owned(),
        patient_id: "patient-a".to_owned(),
        path: path.to_path_buf(),
        title: "PDF sintetico".to_owned(),
        category: "reports".to_owned(),
        kind: "pdf".to_owned(),
        sha256: "synthetic-sha256".to_owned(),
        date: None,
        series: None,
        tags: vec![],
        origin: "source".to_owned(),
        container_id: None,
        internal_path: None,
    }
}

fn expected_path(path: &Path) -> Value {
    json!({
        "kind": "data",
        "value": {"encoding": "utf8", "value": path.to_str().expect("percorso UTF-8")},
    })
}

fn expected_data(value: Value) -> Value {
    json!({"kind": "data", "value": value})
}

fn expected_document(document: &DocumentRecord) -> Value {
    expected_data(json!({
        "document_id": document.document_id,
        "patient_id": document.patient_id,
        "path": expected_path(&document.path),
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

fn expected_output(value: Value) -> Value {
    json!({"kind": "data", "value": {"output": expected_data(value)}})
}

fn assert_pdf_span(records: &[Value], function: &str, arguments: Value, output: Value) -> u64 {
    assert_pdf_spans(records, function, vec![(arguments, output)])
        .into_iter()
        .next()
        .expect("una chiamata attesa")
}

fn assert_pdf_spans(
    records: &[Value],
    function: &str,
    mut expected: Vec<(Value, Value)>,
) -> Vec<u64> {
    let entries: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect();
    assert_eq!(entries.len(), expected.len(), "entrate per {function}");
    let mut unmatched = entries;
    let mut call_ids = Vec::new();
    for (arguments, output) in expected.drain(..) {
        let index = unmatched
            .iter()
            .position(|entry| entry["arguments"] == arguments)
            .unwrap_or_else(|| {
                panic!(
                    "argomenti attesi assenti per {function}: {arguments}; presenti: {:?}",
                    unmatched
                        .iter()
                        .map(|entry| &entry["arguments"])
                        .collect::<Vec<_>>()
                )
            });
        let entry = unmatched.remove(index);
        let call_id = entry["call_id"].as_u64().expect("call_id numerico");
        let exits: Vec<_> = records
            .iter()
            .filter(|record| {
                record["type"] == "function_exit"
                    && record["function"] == function
                    && record["call_id"] == call_id
            })
            .collect();
        assert_eq!(exits.len(), 1, "una sola uscita correlata per {function}");
        assert_eq!(exits[0]["outcome"], "returned");
        assert_eq!(exits[0]["result"], output, "risultato di {function}");
        assert!(exits[0]["duration_ms"].as_u64().is_some());
        call_ids.push(call_id);
    }
    assert!(unmatched.is_empty());
    call_ids
}

#[test]
fn extracts_each_digital_page_and_records_unicode_offsets() {
    let directory = TestDirectory::new();
    let first = "Synthetic non-sensitive PDF text for PyMuPDF integration testing.";
    let second = "Second synthetic page with café and enough searchable content.";
    let record = directory.write_pdf("two-pages.pdf", &synthetic_pdf(&[first, second]));

    let extracted = extract_pdf_text(&record);

    assert_eq!(extracted.document_id, record.document_id);
    assert!(extracted.text.contains(first));
    assert!(
        extracted.text.contains(second),
        "testo estratto: {:?}",
        extracted.text
    );
    assert!(extracted.warnings.is_empty());
    assert_eq!(pdf_page_count(&record), Some(2));
    assert_eq!(extracted.page_spans.len(), 2);
    let spans = &extracted.page_spans;
    assert_eq!(spans[0].page_number, 1);
    assert_eq!(spans[0].character_start, 0);
    assert_eq!(
        spans[0].character_end,
        extracted.text[..extracted.text.find('\n').expect("separatore pagine")]
            .chars()
            .count()
            + 1
    );
    assert_eq!(spans[1].page_number, 2);
    assert_eq!(
        spans[1].character_start,
        spans[0].character_end + 1,
        "offset successivo al separatore newline"
    );
    assert_eq!(spans[1].character_end, extracted.text.chars().count());
}

#[test]
fn malformed_and_text_poor_pdfs_keep_extraction_warnings() {
    let directory = TestDirectory::new();
    let malformed = directory.write_pdf("malformed.pdf", b"not a PDF");
    let malformed_result = extract_pdf_text(&malformed);
    assert!(malformed_result.text.is_empty());
    assert_eq!(malformed_result.page_spans, []);
    assert!(!malformed_result.warnings.is_empty());
    assert!(malformed_result.warnings[0].contains("impossibile leggere"));
    assert_eq!(pdf_page_count(&malformed), None);

    let image_only = directory.write_pdf("image-only.pdf", &synthetic_pdf(&[""]));
    let image_result = extract_pdf_text(&image_only);
    assert!(
        image_result.text.trim().is_empty(),
        "testo estratto: {:?}",
        image_result.text
    );
    assert_eq!(image_result.page_spans.len(), 1);
    assert_eq!(image_result.warnings.len(), 1);
    assert!(image_result.warnings[0].contains("OCR"));
    assert_eq!(pdf_page_count(&image_only), Some(1));
}

#[test]
fn extracts_text_from_a_synthetic_scanned_pdf() {
    let Ok(tesseract) = find_tesseract() else {
        eprintln!("OCR sintetico saltato: PDFium o Tesseract non disponibile");
        return;
    };
    let english_model_available = std::process::Command::new(&tesseract)
        .arg("--list-langs")
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|language| language.trim() == "eng")
        });
    if bind_pdfium().is_err() || !english_model_available {
        eprintln!("OCR sintetico saltato: PDFium o modello Tesseract eng non disponibile");
        return;
    }
    let directory = TestDirectory::new();
    let record = directory.write_pdf(
        "scanned.pdf",
        &synthetic_scanned_pdf("TEST TEST TEST TEST TEST TEST TEST TEST TEST TEST"),
    );

    let extracted = extract_pdf_text(&record);

    assert!(
        extracted.text.matches("TEST").count() >= 8,
        "testo OCR: {:?}; warning: {:?}",
        extracted.text,
        extracted.warnings
    );
    assert!(extracted.warnings.is_empty(), "{:?}", extracted.warnings);
    assert_eq!(extracted.page_spans.len(), 1);
    assert_eq!(extracted.page_spans[0].page_number, 1);

    let digital_record = directory.write_pdf(
        "scanned-with-digital-text.pdf",
        &synthetic_scanned_pdf_with_digital_text(
            "TEST TEST TEST TEST TEST TEST TEST TEST TEST TEST",
            Some("This synthetic digital text is sufficiently long to skip OCR."),
        ),
    );
    let native = extract_pdf_text(&digital_record);
    assert!(native.text.contains("sufficiently long to skip OCR"));
    let forced = extract_pdf_text_with_options(
        &digital_record,
        PdfExtractionOptions {
            force_pdf_ocr: true,
        },
    );
    assert!(
        forced.text.matches("TEST").count() >= 8,
        "testo OCR forzato: {:?}; warning: {:?}",
        forced.text,
        forced.warnings
    );
}

#[test]
fn tesseract_launch_failure_is_reported_without_exposing_stderr() {
    let result = run_tesseract(
        Path::new("/missing/sanikey-test-tesseract"),
        Path::new("/missing/glossary"),
        b"P6\n1 1\n255\n\0\0\0",
        1,
    );

    assert!(
        result
            .unwrap_err()
            .contains("avvio di Tesseract non riuscito")
    );
}

#[test]
fn pdfium_binding_trace_records_failed_candidates_fallback_and_initialized_state() {
    let candidates = [
        PathBuf::from("synthetic-invalid-pdfium.so"),
        PathBuf::from("synthetic-good-pdfium.so"),
    ];
    let mut candidate_number = 0;
    let report = bind_pdfium_candidates(
        &candidates,
        |_| {
            candidate_number += 1;
            if candidate_number == 1 {
                Err(PdfiumBindingError::Failed(
                    "synthetic loader failure".to_owned(),
                ))
            } else {
                Ok("candidate binding")
            }
        },
        || panic!("il fallback di sistema non deve essere tentato"),
    );
    assert!(matches!(
        &report.result,
        PdfiumBindingResult::Bound(value) if *value == "candidate binding"
    ));
    assert_eq!(report.candidates.len(), 2);
    assert_eq!(report.candidates[0].outcome, "failed");
    assert_eq!(
        report.candidates[0].error.as_deref(),
        Some("synthetic loader failure")
    );
    assert_eq!(report.candidates[1].outcome, "bound");
    assert!(matches!(report.system, PdfiumSystemAttempt::NotAttempted));
    assert!(
        report.candidates[0]
            .to_json()
            .to_string()
            .contains("synthetic loader failure")
    );

    let failed_candidate = [PathBuf::from("synthetic-invalid-pdfium.so")];
    let fallback_report = bind_pdfium_candidates(
        &failed_candidate,
        |_| {
            Err(PdfiumBindingError::Failed(
                "synthetic candidate error".to_owned(),
            ))
        },
        || Ok("system binding"),
    );
    assert!(matches!(
        &fallback_report.result,
        PdfiumBindingResult::Bound(value) if *value == "system binding"
    ));
    assert!(matches!(fallback_report.system, PdfiumSystemAttempt::Bound));
    assert_eq!(
        fallback_report.system.to_json(),
        json!({"outcome": "bound"})
    );
    assert_eq!(
        fallback_report.candidates[0].error.as_deref(),
        Some("synthetic candidate error")
    );

    let initialized_report = bind_pdfium_candidates::<String>(
        &failed_candidate,
        |_| Err(PdfiumBindingError::AlreadyInitialized),
        || panic!("non tentare il fallback dopo binding già inizializzato"),
    );
    assert!(matches!(
        initialized_report.result,
        PdfiumBindingResult::AlreadyInitialized
    ));
    assert_eq!(
        initialized_report.candidates[0].outcome,
        "already_initialized"
    );
    assert!(matches!(
        initialized_report.system,
        PdfiumSystemAttempt::NotAttempted
    ));
    assert_eq!(
        initialized_report.candidates[0].to_json()["outcome"],
        "already_initialized"
    );

    let system_initialized_report = bind_pdfium_candidates::<String>(
        &failed_candidate,
        |_| {
            Err(PdfiumBindingError::Failed(
                "synthetic candidate error".to_owned(),
            ))
        },
        || Err(PdfiumBindingError::AlreadyInitialized),
    );
    assert!(matches!(
        system_initialized_report.result,
        PdfiumBindingResult::AlreadyInitialized
    ));
    assert!(matches!(
        system_initialized_report.system,
        PdfiumSystemAttempt::AlreadyInitialized
    ));
    assert_eq!(
        system_initialized_report.system.to_json(),
        json!({"outcome": "already_initialized"})
    );

    let system_failure_report = bind_pdfium_candidates::<String>(
        &failed_candidate,
        |_| {
            Err(PdfiumBindingError::Failed(
                "synthetic candidate error".to_owned(),
            ))
        },
        || {
            Err(PdfiumBindingError::Failed(
                "synthetic system error".to_owned(),
            ))
        },
    );
    assert!(matches!(
        &system_failure_report.result,
        PdfiumBindingResult::Failed(error) if error == "synthetic system error"
    ));
    assert_eq!(system_failure_report.system.to_json()["outcome"], "failed");
    assert!(
        system_failure_report
            .system
            .to_json()
            .to_string()
            .contains("synthetic system error")
    );
}

#[cfg(target_os = "linux")]
fn write_tesseract_fixture(directory: &TestDirectory, name: &str, exit_code: u8) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let executable = directory.0.join(name);
    let script = format!(
        "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 1200 ]; do\n  printf '%s' '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'\n  printf '%s' 'fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210' >&2\n  i=$((i + 1))\ndone\ncat >/dev/null\nprintf '\\nSYNTHETIC_TESSERACT_STDOUT\\n'\nprintf '\\nSYNTHETIC_TESSERACT_STDERR\\n' >&2\nexit {exit_code}\n"
    );
    fs::write(&executable, script).expect("eseguibile Tesseract sintetico");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .expect("permessi eseguibile Tesseract sintetico");
    executable
}

#[cfg(target_os = "linux")]
fn run_tesseract_with_timeout(
    session: &Session,
    executable: PathBuf,
    glossary: PathBuf,
    ppm: Vec<u8>,
) -> Result<std::process::Output, String> {
    let session = session.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = session.scoped(|| run_tesseract(&executable, &glossary, &ppm, 1));
        let _ = sender.send(result);
    });
    receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("Tesseract sintetico deve terminare entro 5 secondi")
}

#[cfg(target_os = "linux")]
#[test]
fn tesseract_trace_captures_both_streams_on_success_and_failure_without_deadlock() {
    let directory = TestDirectory::new();
    let ppm = b"P6\n1 1\n255\n\0\0\0".to_vec();
    let glossary = directory.0.join("synthetic-glossary");
    fs::write(&glossary, "synthetic\n").expect("glossario sintetico");
    let successful = write_tesseract_fixture(&directory, "tesseract-success", 0);
    let failing = write_tesseract_fixture(&directory, "tesseract-failure", 7);
    let (trace_session, trace_path) = Session::create_in_directory_with_level(
        &directory.0.join("trace"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");

    let output =
        run_tesseract_with_timeout(&trace_session, successful, glossary.clone(), ppm.clone())
            .expect("Tesseract sintetico di successo");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("SYNTHETIC_TESSERACT_STDOUT"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("SYNTHETIC_TESSERACT_STDERR"));

    let error = run_tesseract_with_timeout(&trace_session, failing, glossary, ppm)
        .expect_err("Tesseract sintetico deve fallire");
    assert!(error.contains("Tesseract fallito sulla pagina 1 con stato exit status: 7"));
    drop(trace_session);
    let trace_records: Vec<Value> = fs::read_to_string(trace_path)
        .expect("log Trace sintetico")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record Trace JSON valido"))
        .collect();
    let entries: Vec<_> = trace_records
        .iter()
        .filter(|record| {
            record["type"] == "function_enter" && record["function"] == "pdf::run_tesseract"
        })
        .collect();
    assert_eq!(entries.len(), 2);
    for (entry, expected_status) in entries
        .into_iter()
        .zip(["exit status: 0", "exit status: 7"])
    {
        let call_id = entry["call_id"].as_u64().expect("call_id numerico");
        assert!(entry.to_string().contains("stderr_capture"));
        let exit = trace_records
            .iter()
            .find(|record| {
                record["type"] == "function_exit"
                    && record["function"] == "pdf::run_tesseract"
                    && record["call_id"] == call_id
            })
            .expect("uscita correlata tramite call_id");
        assert!(exit["duration_ms"].as_u64().is_some());
        let output = &exit["result"]["value"]["output"]["value"];
        assert_eq!(output["status"], expected_status);
        assert!(
            output["stdout"]["value"]["value"]
                .as_str()
                .expect("stdout testuale")
                .contains("SYNTHETIC_TESSERACT_STDOUT")
        );
        assert!(
            output["stderr"]["value"]["value"]
                .as_str()
                .expect("stderr testuale")
                .contains("SYNTHETIC_TESSERACT_STDERR")
        );
    }

    let successful = write_tesseract_fixture(&directory, "tesseract-events", 0);
    let (events_session, events_path) = Session::create_in_directory_with_level(
        &directory.0.join("events"),
        Component::Core,
        Level::Events,
    )
    .expect("sessione Events sintetica");
    run_tesseract_with_timeout(
        &events_session,
        successful,
        directory.0.join("unused"),
        b"".to_vec(),
    )
    .expect("Tesseract sintetico Events");
    drop(events_session);
    let events_log = fs::read_to_string(events_path).expect("log Events sintetico");
    assert!(!events_log.contains("SYNTHETIC_TESSERACT_STDOUT"));
    assert!(!events_log.contains("SYNTHETIC_TESSERACT_STDERR"));
    assert!(!events_log.contains("function_enter"));
}

#[test]
fn medical_glossary_is_canonical_and_matches_the_baseline() {
    let glossary = include_str!("../../assets/ocr/tesseract-medical-it.user-words");
    let entries: Vec<&str> = glossary.lines().collect();

    assert!(glossary.ends_with('\n'));
    assert!(!glossary.starts_with('\u{feff}'));
    assert!(!entries.is_empty());
    assert!(
        entries
            .iter()
            .all(|entry| !entry.is_empty() && *entry == entry.trim())
    );
    let mut unique = entries.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), entries.len());
    assert!(entries.contains(&"HbA1c"));
    assert!(!entries.contains(&"HbAlc"));
}

#[test]
fn synthetic_trace_captures_pdf_text_and_ocr_boundary_while_events_omit_it() {
    use super::rgba_to_ppm;
    use crate::logging::{Component, Level, Session};

    let directory = TestDirectory::new();
    let pdf_path = {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            directory
                .0
                .join(std::ffi::OsString::from_vec(b"synthetic-\xff.pdf".to_vec()))
        }
        #[cfg(not(unix))]
        {
            directory.0.join("synthetic.pdf")
        }
    };
    fs::write(
        &pdf_path,
        synthetic_pdf(&[
            "Synthetic private PDF text with enough characters to avoid OCR; password = synthetic-pdf-secret; detail retained.",
        ]),
    )
    .expect("PDF sintetico");
    let record = document(&pdf_path);

    for (suffix, level) in [("events", Level::Events), ("trace", Level::Trace)] {
        let logs = directory.0.join(format!("logs-{suffix}"));
        let (session, log_path) =
            Session::create_in_directory_with_level(&logs, Component::Core, level)
                .expect("sessione sintetica");
        session.scoped(|| {
            let trace =
                crate::logging::trace_current(Component::Core, "pdf::synthetic_ocr_stdout", || {
                    vec![crate::logging::TraceField::new(
                        "stdout",
                        super::trace_pdf_bytes(
                            b"api_key = synthetic-ocr-secret; private OCR output retained",
                        ),
                    )]
                });
            if let Some(trace) = trace {
                let _ = trace.finish(crate::logging::TraceValue::data("completed"));
            }
        });
        let extracted = session.scoped(|| extract_pdf_text(&record));
        assert!(extracted.text.contains("Synthetic private PDF text"));
        assert_eq!(session.scoped(|| pdf_page_count(&record)), Some(1));
        let ppm = session
            .scoped(|| rgba_to_ppm(&[10, 20, 30, 255, 40, 50, 60, 255], 2, 1))
            .expect("PPM sintetico");
        let tesseract = session.scoped(|| {
            run_tesseract(
                Path::new("/missing/sanikey-test-tesseract"),
                Path::new("/missing/sanikey-test-glossary"),
                &ppm,
                1,
            )
        });
        assert!(tesseract.is_err());
        drop(session);

        let log = fs::read_to_string(log_path).expect("log sintetico");
        if level == Level::Trace {
            assert!(log.contains("pdf::extract_pdf_text"));
            assert!(log.contains("pdf::extract_digital_pdf_text"));
            assert!(log.contains("pdf::has_sufficient_pdf_text"));
            assert!(log.contains("pdf::pdf_page_count"));
            assert!(log.contains("pdf_file_bytes"));
            assert!(log.contains("external_component"));
            assert!(log.contains("Synthetic private PDF text"));
            assert!(log.contains("secret_omitted"));
            assert!(log.contains("detail retained"));
            assert!(log.contains("private OCR output retained"));
            assert!(!log.contains("synthetic-pdf-secret"));
            assert!(!log.contains("synthetic-ocr-secret"));
            assert!(log.contains("pdf::rgba_to_ppm"));
            assert!(log.contains("pdf::run_tesseract"));
            assert!(log.contains("stdin_ppm_bytes"));
            assert!(log.contains("stderr_capture"));
            #[cfg(unix)]
            {
                assert!(log.contains("unix_bytes"));
                assert!(log.contains("255"));
            }
        } else {
            assert!(!log.contains("function_enter"));
            assert!(!log.contains("Synthetic private PDF text"));
            assert!(!log.contains("synthetic-pdf-secret"));
            assert!(!log.contains("synthetic-"));
        }
    }
}

#[test]
fn synthetic_pdf_spans_match_full_arguments_outputs_ids_durations_and_omissions() {
    let directory = TestDirectory::new();
    let secret = "synthetic-pdf-trace-secret";
    let source_text = format!(
        "Synthetic digital report has enough characters for parsing; password = {secret}; retained."
    );
    let extracted_text = format!("{source_text}\n");
    let path = directory.write_pdf("trace-report.pdf", &synthetic_pdf(&[&source_text]));
    let safe_text = source_text.replace(secret, "[secret_omitted]");
    let safe_extracted_text = format!("{safe_text}\n");
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.0.join("trace-logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione PDF sintetica");
    let (document_id, extraction, empty, ppm, page_count, threshold) = session.scoped(|| {
        let extracted = extract_pdf_text(&path);
        let empty = super::empty_extraction("empty-synthetic-document");
        let ppm = super::rgba_to_ppm(&[10, 20, 30, 255, 40, 50, 60, 255], 2, 1)
            .expect("PPM sintetico valido");
        let page_count = pdf_page_count(&path);
        let threshold = super::has_sufficient_pdf_text(&source_text);
        (path.clone(), extracted, empty, ppm, page_count, threshold)
    });
    assert_eq!(extraction.text, extracted_text);
    assert!(extraction.warnings.is_empty());
    assert_eq!(
        extraction.page_spans,
        [TextPageSpan {
            page_number: 1,
            character_start: 0,
            character_end: extracted_text.chars().count(),
        }]
    );
    assert_eq!(
        empty,
        ExtractedText {
            document_id: "empty-synthetic-document".to_owned(),
            text: String::new(),
            warnings: Vec::new(),
            page_spans: Vec::new(),
        }
    );
    let expected_ppm = b"P6\n2 1\n255\n\x0a\x14\x1e\x28\x32\x3c".to_vec();
    assert_eq!(ppm, expected_ppm);
    assert_eq!(page_count, Some(1));
    assert!(threshold);
    drop(session);

    let records: Vec<Value> = fs::read_to_string(log_path)
        .expect("log PDF sintetico")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON valido"))
        .collect();
    let document = document_id;
    let extracted_projection = json!({
        "document_id": "synthetic-pdf-id",
        "text": safe_extracted_text,
        "warnings": [],
        "page_spans": expected_data(json!([{
            "page_number": 1,
            "character_start": 0,
            "character_end": extracted_text.chars().count(),
        }])),
    });
    let extracted_output = expected_output(extracted_projection);

    let extract_id = assert_pdf_span(
        &records,
        "pdf::extract_pdf_text",
        json!([{"name": "document", "content": expected_document(&document)}]),
        extracted_output.clone(),
    );
    let options_id = assert_pdf_span(
        &records,
        "pdf::extract_pdf_text_with_options",
        json!([
            {"name": "document", "content": expected_document(&document)},
            {"name": "options", "content": expected_data(json!("PdfExtractionOptions {\n    force_pdf_ocr: false,\n}"))},
        ]),
        extracted_output.clone(),
    );
    let digital_id = assert_pdf_span(
        &records,
        "pdf::extract_digital_pdf_text",
        json!([
            {"name": "document", "content": expected_document(&document)},
            {"name": "pdf_file_bytes", "content": {"kind": "omitted", "reason": "external_component"}},
        ]),
        extracted_output,
    );
    let threshold_arguments = |text: &str| {
        json!([{"name": "text", "content": expected_data(json!({
            "format": "text_redacted_known_secret_assignments",
            "value": text,
        }))}])
    };
    let threshold_ids = assert_pdf_spans(
        &records,
        "pdf::has_sufficient_pdf_text",
        vec![
            (
                threshold_arguments(&safe_extracted_text),
                expected_output(json!(true)),
            ),
            (
                threshold_arguments(&safe_extracted_text),
                expected_output(json!(true)),
            ),
            (
                threshold_arguments(&safe_text),
                expected_output(json!(true)),
            ),
        ],
    );
    let empty_output = expected_output(json!({
        "document_id": "empty-synthetic-document",
        "text": "",
        "warnings": [],
        "page_spans": expected_data(json!([])),
    }));
    let empty_id = assert_pdf_span(
        &records,
        "pdf::empty_extraction",
        json!([{"name": "document_id", "content": expected_data(json!("empty-synthetic-document"))}]),
        empty_output,
    );
    let ppm_id = assert_pdf_span(
        &records,
        "pdf::rgba_to_ppm",
        json!([
            {"name": "rgba", "content": expected_data(json!([10, 20, 30, 255, 40, 50, 60, 255]))},
            {"name": "width", "content": expected_data(json!(2))},
            {"name": "height", "content": expected_data(json!(1))},
        ]),
        expected_output(json!({"ppm_bytes": expected_ppm, "result": "ok"})),
    );
    let page_count_id = assert_pdf_span(
        &records,
        "pdf::pdf_page_count",
        json!([
            {"name": "document", "content": expected_document(&document)},
            {"name": "pdf_file_bytes", "content": {"kind": "omitted", "reason": "external_component"}},
        ]),
        expected_output(json!(1)),
    );
    assert_eq!(
        records
            .iter()
            .find(|record| record["call_id"] == options_id)
            .unwrap()["parent_call_id"],
        extract_id
    );
    assert_eq!(
        records
            .iter()
            .find(|record| record["call_id"] == digital_id)
            .unwrap()["parent_call_id"],
        options_id
    );
    assert_eq!(
        records
            .iter()
            .find(|record| record["call_id"] == threshold_ids[0])
            .unwrap()["parent_call_id"],
        digital_id
    );
    assert_eq!(
        records
            .iter()
            .find(|record| record["call_id"] == threshold_ids[1])
            .unwrap()["parent_call_id"],
        options_id
    );
    assert!(records.iter().any(|record| {
        record["call_id"] == threshold_ids[2] && record["parent_call_id"].is_null()
    }));
    for id in [empty_id, ppm_id, page_count_id] {
        assert!(
            records
                .iter()
                .any(|record| record["call_id"] == id && record["parent_call_id"].is_null())
        );
    }
    let serialized = serde_json::to_string(&records).expect("records serializzabili");
    assert!(serialized.contains("secret_omitted"));
    assert!(!serialized.contains(secret));

    let (events, event_path) = Session::create_in_directory_with_level(
        &directory.0.join("event-logs"),
        Component::Core,
        Level::Events,
    )
    .expect("sessione eventi sintetica");
    events.scoped(|| assert!(super::has_sufficient_pdf_text(&source_text)));
    drop(events);
    let event_log = fs::read_to_string(event_path).expect("log eventi PDF sintetico");
    assert!(!event_log.contains("function_enter"));
    assert!(!event_log.contains(secret));
}
